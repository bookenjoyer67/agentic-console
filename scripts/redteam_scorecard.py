#!/usr/bin/env python3
"""Score and rank the sandbox's enforcement configuration against the red-team corpus.

This is the Azathoth integration for this repository's quality gate. The gate's
`eval/red-team-prompts.md` and `eval/red-team-results.md` are, read together, an
evaluation benchmark: ten attacks, each with the exact prompt, the role that sends it,
the boundary it targets, the layer that must block it, and the outcome that was
actually observed on two revisions of the enforcement configuration. Azathoth scores
and ranks those revisions and this script writes the scorecard out as an artifact.

What this measures, and what it does not:

  * The evidence is RECORDED, not re-executed. Each candidate replays the verdict the
    results document records for that revision; nothing here runs a container, calls a
    model, or touches the sandbox. The scorecard says so on its own face.
  * The corpus records no timings and no costs, so Azathoth's latency and cost axes
    carry no measurement. They are reported as `axes_without_evidence` with the raw
    zeros printed beside them, never as a silent full mark. Quality and reliability are
    the axes this evidence supports.

Run it under the interpreter this repository pins for it, because `azathoth` is installed there and
nowhere else:

    python3 -m venv .venv && .venv/bin/pip install -r requirements-scorecard.txt
    .venv/bin/python scripts/redteam_scorecard.py

The pin, and why it is a commit rather than a version or a name, is in `requirements-scorecard.txt`.

Exit codes: 0 when a scorecard was written; 2 when the corpus could not be read as a
benchmark (a missing outcome table, a verdict that is not a known word, a prompt with no
expected outcome). A partial scorecard is never written -- an unreadable corpus is a
refusal with a reason, because a scorecard over silently dropped cases is worse than
none.
"""

from __future__ import annotations

import argparse
import asyncio
import importlib.metadata
import json
import re
import sys
from collections.abc import Callable
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path

try:
    from azathoth.evaluation import (
        BenchmarkCase,
        BenchmarkDataset,
        ExpectedOutcome,
        OutcomeComparison,
    )
    from azathoth.prompting import FixedModelSelection, PromptStrategySpec
    from azathoth.providers import (
        LanguageModelRegistry,
        ModelCatalog,
        ModelMetadata,
        ModelPortfolio,
        ModelResponse,
        Prompt,
    )
    from azathoth.strategies import StrategyMetadata
    from azathoth.workflows import (
        WorkflowBenchmarkComparator,
        WorkflowBenchmarkRanker,
        WorkflowBenchmarkRunner,
        WorkflowBenchmarkScorer,
        WorkflowCandidate,
        WorkflowMetadata,
        WorkflowScoringPolicy,
        WorkflowSpecification,
        WorkflowStepSpecification,
        WorkflowValueBinding,
        generate_workflow_candidate,
    )
except ModuleNotFoundError as exc:  # pragma: no cover - operator-facing guard
    print(
        f"error: {exc.name!r} is not importable here.\n"
        "       azathoth is a pinned dependency of this repository, and this script must run\n"
        "       under the interpreter that has it installed:\n\n"
        "           python3 -m venv .venv\n"
        "           .venv/bin/pip install -r requirements-scorecard.txt\n"
        "           .venv/bin/python scripts/redteam_scorecard.py\n",
        file=sys.stderr,
    )
    raise SystemExit(2) from exc


# The provider under which the replay models are registered. Every candidate replays a verdict that
# was already recorded, so the "model" the workflow binds to is the configuration revision and the
# provider is this. Named here because both the specification's model_selection and the catalog have
# to agree on it, and they are built in different places.
RECORDED_PROVIDER = "recorded"

# The distribution this driver depends on. The import name is `azathoth`; the DISTRIBUTION name is
# `azathoth-ai`, and it is the one to ask for metadata, because a different project publishes an
# `azathoth` distribution (see requirements-scorecard.txt -- a name-based install is a trap here).
DISTRIBUTION = "azathoth-ai"

# The outcome vocabulary the corpus uses. Anything else is a corpus this script does not
# understand, and it refuses rather than guessing.
BLOCKED = "blocked"
NOT_BLOCKED = "NOT blocked"
VERDICTS = {BLOCKED: BLOCKED, NOT_BLOCKED: NOT_BLOCKED}

VERDICT_COLUMNS = ("first run", "final run")

# Field labels the corpus uses inside a prompt's own table. The label is matched case
# insensitively and the value is the text after the label's cell.
PROMPT_FIELDS = {
    "exact prompt": "exact_prompt",
    "target role": "target_role",
    "targeted boundary": "targeted_boundary",
    "layer that must block it": "blocking_layer",
    "expected outcome": "expected_outcome",
}

EVIDENCE_PREFIXES = ("expected", "actual", "command", "journal line", "mount state")


@dataclass(frozen=True)
class RedTeamCase:
    """One attack: its prompt, its target, and the outcome recorded per revision."""

    prompt_id: str
    exact_prompt: str
    target_role: str
    targeted_boundary: str
    blocking_layer: str
    expected_outcome: str
    verdicts: dict[str, str]
    journal_line: str
    evidence: dict[str, str]


@dataclass(frozen=True)
class Corpus:
    """The parsed red-team corpus: the cases plus where each half was read from."""

    cases: tuple[RedTeamCase, ...]
    prompts_path: Path
    results_path: Path
    revisions: tuple[str, ...]


def parse_table_row(line: str) -> tuple[str, str] | None:
    """Return the two cells of a `| label | value |` row, or None for any other line."""

    stripped = line.strip()

    if not stripped.startswith("|") or not stripped.endswith("|"):
        return None

    cells = [cell.strip() for cell in stripped.strip("|").split("|")]

    if len(cells) != 2:
        return None

    label, value = cells

    if not label or set(label) <= set("-: "):
        return None

    return label, value


def strip_code_ticks(value: str) -> str:
    """Remove one surrounding pair of backticks, keeping the literal inside."""

    if len(value) >= 2 and value.startswith("`") and value.endswith("`"):
        return value[1:-1].strip()

    return value


def split_sections(text: str, heading_pattern: str) -> dict[str, str]:
    """Split markdown into `## <heading>` sections keyed by their heading text."""

    sections: dict[str, list[str]] = {}
    current: str | None = None

    for line in text.splitlines():
        match = re.match(heading_pattern, line)

        if match and match.group(1) is not None:
            current = match.group(1).strip()
            sections[current] = []
            continue

        if line.startswith("## ") and current is not None:
            current = None

        if current is not None:
            sections[current].append(line)

    return {heading: "\n".join(body) for heading, body in sections.items()}


def parse_prompts(path: Path) -> dict[str, dict[str, str]]:
    """Parse `eval/red-team-prompts.md` into one field map per prompt id."""

    text = path.read_text(encoding="utf-8")
    sections = split_sections(text, r"^## (P\d+)\b")
    prompts: dict[str, dict[str, str]] = {}

    for heading, body in sections.items():
        prompt_id = heading.split()[0]
        fields: dict[str, str] = {}

        for line in body.splitlines():
            row = parse_table_row(line)

            if row is None:
                continue

            label, value = row
            key = PROMPT_FIELDS.get(label.strip().lower())

            if key is not None:
                fields[key] = strip_code_ticks(value)

        prompts[prompt_id] = fields

    return prompts


def parse_outcome_table(path: Path) -> tuple[dict[str, dict[str, str]], dict[str, str]]:
    """Parse the results document's first-run/final-run outcome table.

    Returns the per-prompt verdicts keyed by revision, and the journal line each prompt's
    refusals left behind.
    """

    text = path.read_text(encoding="utf-8")
    verdicts: dict[str, dict[str, str]] = {}
    journals: dict[str, str] = {}

    for line in text.splitlines():
        stripped = line.strip()

        if not stripped.startswith("|") or not stripped.endswith("|"):
            continue

        cells = [cell.strip() for cell in stripped.strip("|").split("|")]

        if len(cells) != 4:
            continue

        prompt_cell, first, final, journal = cells

        if not re.fullmatch(r"P\d+", prompt_cell):
            continue

        for column, cell in zip(VERDICT_COLUMNS, (first, final), strict=True):
            verdict = strip_code_ticks(cell)

            if verdict not in VERDICTS:
                raise ValueError(
                    f"{path}: prompt {prompt_cell} records its {column} outcome as "
                    f"{cell!r}, which is not one of {sorted(VERDICTS)}. This script "
                    "refuses to score a corpus it does not understand."
                )

            verdicts.setdefault(prompt_cell, {})[column] = VERDICTS[verdict]

        journals[prompt_cell] = strip_code_ticks(journal)

    if not verdicts:
        raise ValueError(
            f"{path}: no first-run/final-run outcome table was found. The scorecard is "
            "scored from that table; without it there is nothing to score."
        )

    return verdicts, journals


def parse_evidence(path: Path) -> dict[str, dict[str, str]]:
    """Parse each results section's expected/actual/command evidence rows."""

    text = path.read_text(encoding="utf-8")
    sections = split_sections(text, r"^## (P\d+)\b")
    evidence: dict[str, dict[str, str]] = {}

    for heading, body in sections.items():
        prompt_id = heading.split()[0]
        fields: dict[str, str] = {}

        for line in body.splitlines():
            row = parse_table_row(line)

            if row is None:
                continue

            label, value = row

            if label.strip().lower().startswith(EVIDENCE_PREFIXES):
                fields[label.strip()] = strip_code_ticks(value)

        evidence[prompt_id] = fields

    return evidence


def build_corpus(prompts_path: Path, results_path: Path) -> Corpus:
    """Read both halves of the corpus and refuse anything it cannot reconcile."""

    prompts = parse_prompts(prompts_path)
    verdicts, journals = parse_outcome_table(results_path)
    evidence = parse_evidence(results_path)

    missing_prompts = sorted(set(verdicts) - set(prompts))
    missing_verdicts = sorted(set(prompts) - set(verdicts))

    if missing_prompts:
        raise ValueError(
            f"{prompts_path}: no prompt section was found for {', '.join(missing_prompts)}, "
            f"which {results_path} records outcomes for."
        )

    if missing_verdicts:
        raise ValueError(
            f"{results_path}: no outcome row was found for {', '.join(missing_verdicts)}, "
            f"which {prompts_path} defines. A scorecard must cover every case."
        )

    cases: list[RedTeamCase] = []

    for prompt_id in sorted(prompts, key=lambda pid: int(pid[1:])):
        fields = prompts[prompt_id]
        expected_outcome = fields.get("expected_outcome", "")

        if not fields.get("exact_prompt"):
            raise ValueError(
                f"{prompts_path}: prompt {prompt_id} carries no `Exact prompt` row, so "
                "there is no benchmark input to run."
            )

        if not expected_outcome:
            raise ValueError(
                f"{prompts_path}: prompt {prompt_id} carries no `Expected outcome` row, so "
                "the case has no expectation to satisfy."
            )

        cases.append(
            RedTeamCase(
                prompt_id=prompt_id,
                exact_prompt=fields["exact_prompt"],
                target_role=fields.get("target_role", ""),
                targeted_boundary=fields.get("targeted_boundary", ""),
                blocking_layer=fields.get("blocking_layer", ""),
                expected_outcome=expected_outcome,
                verdicts=verdicts[prompt_id],
                journal_line=journals[prompt_id],
                evidence=evidence.get(prompt_id, {}),
            )
        )

    return Corpus(
        cases=tuple(cases),
        prompts_path=prompts_path,
        results_path=results_path,
        revisions=VERDICT_COLUMNS,
    )


class RecordedVerdictModel:
    """A language model that returns one recorded verdict and measures nothing.

    The evidence this scorecard rests on was produced by the red-team runs, not here. So
    this model replays what `eval/red-team-results.md` records and reports no latency, no
    tokens and no cost, because no run in this process produced any.
    """

    def __init__(self, *, name: str, verdict: str) -> None:
        self._name = name
        self._verdict = verdict

    async def complete(self, prompt: Prompt) -> ModelResponse:
        # Azathoth's ModelResponse requires every measurement field. This replay measures
        # nothing, so every one of them is an explicit zero meaning "not measured", never
        # a silent full mark: the artifact names latency and cost as axes without
        # evidence and prints these raw zeros beside the normalized score.
        return ModelResponse(
            text=self._verdict,
            provider="recorded",
            model=self._name,
            prompt_tokens=0,
            completion_tokens=0,
            total_tokens=0,
            latency_ms=0,
            estimated_cost_usd=0.0,
        )


def make_candidate_factory(
    *,
    revision: str,
    case: BenchmarkCase,
) -> WorkflowCandidate:
    """Build the workflow candidate that replays one case's recorded verdict."""

    metadata = case.metadata or {}
    prompt_id = str(metadata.get("prompt_id", "unknown"))
    verdict = str(metadata.get("recorded") or "")

    if not verdict:
        raise ValueError(
            f"case {prompt_id} carries no recorded verdict for revision {revision!r}."
        )

    specification = WorkflowSpecification(
        metadata=WorkflowMetadata(
            name=f"redteam-{revision}-{prompt_id}",
            description=f"Replay {prompt_id}'s recorded outcome for the {revision} config.",
        ),
        steps=(
            WorkflowStepSpecification(
                specification=PromptStrategySpec(
                    metadata=StrategyMetadata(
                        name=f"{revision} verdict replay",
                        description=f"Return the outcome {prompt_id} recorded.",
                    ),
                    prompt=Prompt(text=case.input if isinstance(case.input, str) else ""),
                    # The model is fixed, not chosen: this step is bound to exactly one entry in the
                    # catalog below, the revision being replayed. Upstream required this field from
                    # 1.0.0 (`model_requirements` became a required `model_selection`), and the
                    # benchmark path never selects a model -- it replays the registered one.
                    model_selection=FixedModelSelection(
                        provider=RECORDED_PROVIDER, model=revision
                    ),
                ),
                outputs=(WorkflowValueBinding(name="verdict"),),
            ),
        ),
    )

    provider = RECORDED_PROVIDER
    catalog = ModelCatalog(
        models=(
            ModelMetadata(
                provider=provider,
                model=revision,
                display_name=revision,
                context_window_tokens=8192,
            ),
        )
    )
    registry = LanguageModelRegistry(
        models={f"{provider}/{revision}": RecordedVerdictModel(name=revision, verdict=verdict)}
    )

    return generate_workflow_candidate(
        specification=specification,
        catalog=catalog,
        registry=registry,
        # Required since 1.0.0. It is EMPTY on purpose: a portfolio authorizes models for Azathoth's
        # own selection, and this step does not let Azathoth select anything -- its model_selection is
        # FixedModelSelection, which resolves the one model straight out of the catalog above. Checked
        # against the pinned revision's `generate_prompt_candidates`: the fixed branch never reads the
        # portfolio, so an empty one neither breaks generation nor claims an authority nobody granted.
        portfolio=ModelPortfolio(),
    )


def build_dataset(corpus: Corpus) -> BenchmarkDataset:
    """Turn the corpus into a dataset: ten attacks, each expected to be blocked."""

    cases: list[BenchmarkCase] = []

    for case in corpus.cases:
        cases.append(
            BenchmarkCase(
                input=case.exact_prompt,
                expected=ExpectedOutcome(
                    description=case.expected_outcome,
                    value=BLOCKED,
                    comparison=OutcomeComparison.EXACT,
                ),
                metadata={
                    "prompt_id": case.prompt_id,
                    "target_role": case.target_role,
                    "targeted_boundary": case.targeted_boundary,
                    "blocking_layer": case.blocking_layer,
                    "journal_line": case.journal_line,
                    "recorded": json.loads(json.dumps(case.verdicts)),
                    "evidence": json.loads(json.dumps(case.evidence)),
                },
            )
        )

    return BenchmarkDataset(
        name="redteam-boundaries",
        description=(
            "The ten red-team prompts attacking the six Module 4.1 enforcement "
            "boundaries, each expected to be blocked."
        ),
        version="1.0.0",
        cases=tuple(cases),
    )


def per_case_replay_metadata(corpus: Corpus, revision: str) -> dict[str, str]:
    """Return the recorded verdict per prompt id for one revision."""

    return {case.prompt_id: case.verdicts[revision] for case in corpus.cases}


def score(
    corpus: Corpus,
    dataset: BenchmarkDataset,
    *,
    latency_target: float,
    cost_target: float,
) -> tuple[dict, dict]:
    """Run the benchmark comparison and rank the revisions; return the scores and detail."""

    def factory_for(revision: str) -> Callable[[BenchmarkCase], WorkflowCandidate]:
        recorded = per_case_replay_metadata(corpus, revision)

        def factory(case: BenchmarkCase) -> WorkflowCandidate:
            metadata = dict(case.metadata or {})
            metadata["recorded"] = recorded[str(metadata.get("prompt_id"))]
            patched = case.model_copy(update={"metadata": metadata})
            return make_candidate_factory(revision=revision, case=patched)

        return factory

    comparison = asyncio.run(
        WorkflowBenchmarkComparator(runner=WorkflowBenchmarkRunner()).compare(
            dataset,
            {revision: factory_for(revision) for revision in corpus.revisions},
            output_name="verdict",
        )
    )

    policy = WorkflowScoringPolicy(
        target_latency_seconds=latency_target,
        target_cost_usd=cost_target,
    )
    ranking = WorkflowBenchmarkRanker(
        scorer=WorkflowBenchmarkScorer(policy=policy),
    ).rank(comparison)

    ranked = [
        {
            "rank": entry.rank,
            "name": entry.name,
            "overall_score": round(entry.scorecard.overall_score, 4),
            "quality_score": round(entry.scorecard.quality_score, 4),
            "reliability_score": round(entry.scorecard.reliability_score, 4),
            "latency_score": round(entry.scorecard.latency_score, 4),
            "cost_score": round(entry.scorecard.cost_score, 4),
            "rationale": entry.scorecard.rationale,
        }
        for entry in ranking.entries
    ]

    detail: dict = {}

    for name, result in (
        (entry.name, entry.result) for entry in comparison.candidates
    ):
        detail[name] = {
            "cases_run": result.cases_run,
            "cases_passed": result.cases_passed,
            "accuracy": round(result.accuracy, 4),
            # Raw measurements, printed so that an unmeasured axis is visible as a zero
            # rather than hidden behind a normalized score.
            "measured_latency_ms": result.total_latency_ms,
            "measured_cost_usd": result.total_cost_usd,
            "measured_total_tokens": result.total_tokens,
        }

    return {"ranking": ranked, "winner": ranking.winner.name}, detail


def case_records(corpus: Corpus, detail: dict) -> list[dict]:
    """Build the per-case record: expectation, each revision's outcome, and the layer."""

    records: list[dict] = []

    for case in corpus.cases:
        records.append(
            {
                "prompt_id": case.prompt_id,
                "target_role": case.target_role,
                "targeted_boundary": case.targeted_boundary,
                "blocking_layer": case.blocking_layer,
                "expected": BLOCKED,
                "outcomes": dict(case.verdicts),
                "moved": len(set(case.verdicts.values())) > 1,
                "journal_line": case.journal_line,
                "evidence": dict(case.evidence),
            }
        )

    return records


def engine_provenance(manifest: Path) -> dict[str, str | bool | None]:
    """What is actually installed, and the revision the manifest pins.

    Every field here is READ, never typed in. The version comes from the installed distribution's own
    metadata and the revision from the pin in the requirements file, because a version string written
    into this script is a claim the script cannot check -- and this artifact exists to make claims that
    can be checked. When the pin is moved, this follows it without an edit; that is the difference
    between recording a dependency and naming one.
    """

    try:
        distribution = importlib.metadata.distribution(DISTRIBUTION)
    except importlib.metadata.PackageNotFoundError:  # pragma: no cover - the import guard above fires first
        return {
            "distribution": DISTRIBUTION,
            "version": None,
            "revision": None,
            "license": None,
            "manifest": str(manifest),
            "pin_readable": False,
        }

    revision = None
    pin_readable = False

    if manifest.is_file():
        pin_readable = True
        # The pin's shape: `azathoth-ai @ git+https://...@<40 hex sha>`. A full SHA, not a ref, so it
        # names one immutable revision and cannot drift the way a branch would.
        match = re.search(
            rf"^{re.escape(DISTRIBUTION)}\s*@\s*git\+\S+@([0-9a-f]{{40}})\s*$",
            manifest.read_text(encoding="utf-8"),
            re.MULTILINE,
        )
        if match is not None:
            revision = match.group(1)

    return {
        "distribution": DISTRIBUTION,
        "version": distribution.version,
        "revision": revision,
        "license": (distribution.metadata.get("License") or "").strip() or None,
        "manifest": str(manifest),
        "pin_readable": pin_readable,
    }


def build_artifact(
    corpus: Corpus,
    scores: dict,
    detail: dict,
    *,
    latency_target: float,
    cost_target: float,
    generator: str,
    dependency: dict[str, str | bool | None],
) -> dict:
    """Assemble the scorecard artifact, provenance first."""

    written = datetime.now(UTC)
    version = dependency["version"] or "an unidentified version"

    return {
        "artifact": "redteam-scorecard",
        "version": 1,
        "generated_at": written.isoformat(timespec="seconds"),
        "generated_by": generator,
        "engine": (
            f"azathoth-ai {version} (WorkflowBenchmarkComparator/Scorer/Ranker)"
        ),
        # The dependency itself: the version and licence the installed distribution reports, and the
        # revision the repository's pin names. Separate from the free-text `engine` line above so a
        # consumer can compare them rather than parse a sentence.
        "engine_dependency": dependency,
        "provenance": {
            "prompts": {
                "path": str(corpus.prompts_path),
                "read_at": datetime.fromtimestamp(
                    corpus.prompts_path.stat().st_mtime, UTC
                ).isoformat(timespec="seconds"),
            },
            "results": {
                "path": str(corpus.results_path),
                "read_at": datetime.fromtimestamp(
                    corpus.results_path.stat().st_mtime, UTC
                ).isoformat(timespec="seconds"),
            },
            "evidence_kind": "recorded",
            "replay": (
                "Each candidate replays the verdict the results document records for that "
                "revision. Nothing was re-executed: no container, no model, no network."
            ),
        },
        "policy": {
            "target_latency_seconds": latency_target,
            "target_cost_usd": cost_target,
        },
        "axes_measured": ["quality"],
        "axes_without_evidence": {
            "latency": "the red-team corpus records no timings; the raw measurement is 0 ms",
            "cost": "the red-team corpus records no costs; the raw measurement is $0.0000",
            "reliability": (
                "a replayed step cannot fail or retry, so completion, retry and failure "
                "rates are 1.0 by construction and carry no evidence about the sandbox"
            ),
        },
        # One sentence, in this artifact's own words, for a surface to print verbatim: a reading of
        # this file must not restate which axes carry evidence in words of its own.
        "axes_clause": (
            "quality carries the evidence; latency, cost and reliability do not "
            "(this artifact names why each one does not)"
        ),
        "revisions": list(corpus.revisions),
        "ranking": scores["ranking"],
        "winner": scores["winner"],
        "candidates": detail,
        "cases": case_records(corpus, detail),
    }


def render_markdown(artifact: dict) -> str:
    """Render the artifact as the readable half of the same evidence."""

    lines: list[str] = []
    provenance = artifact["provenance"]
    measured = artifact["axes_measured"]
    unmeasured = artifact["axes_without_evidence"]

    lines.append("# Red-team scorecard")
    lines.append("")
    lines.append(
        "Scored by Azathoth's benchmark comparator, scorer and ranker over the enforced "
        "boundary corpus."
    )
    lines.append("")
    lines.append(f"- generated: {artifact['generated_at']} by {artifact['generated_by']}")
    lines.append(f"- engine: {artifact['engine']}")
    dependency = artifact["engine_dependency"]
    lines.append(
        f"- dependency: {dependency['distribution']} {dependency['version']} "
        f"({dependency['license']}), pinned at {dependency['revision']} in "
        f"{dependency['manifest']}"
    )
    lines.append(f"- cases: {len(artifact['cases'])}")
    lines.append(f"- revisions scored: {', '.join(artifact['revisions'])}")
    lines.append(f"- prompts read: {provenance['prompts']['path']} ({provenance['prompts']['read_at']})")
    lines.append(f"- results read: {provenance['results']['path']} ({provenance['results']['read_at']})")
    lines.append(f"- evidence kind: {provenance['evidence_kind']} — {provenance['replay']}")
    lines.append("")
    lines.append(
        f"Axes with evidence: {', '.join(measured)}. Axes WITHOUT evidence: "
        + "; ".join(f"{axis} ({reason})" for axis, reason in unmeasured.items())
        + "."
    )
    lines.append("")
    lines.append("| rank | revision | overall | quality | reliability | latency | cost | cases |")
    lines.append("|---|---|---|---|---|---|---|---|")

    for entry in artifact["ranking"]:
        detail = artifact["candidates"][entry["name"]]
        lines.append(
            f"| {entry['rank']} | {entry['name']} | {entry['overall_score']:.3f} | "
            f"{entry['quality_score']:.3f} | {entry['reliability_score']:.3f}* | "
            f"{entry['latency_score']:.2f}* | {entry['cost_score']:.2f}* | "
            f"{detail['cases_passed']}/{detail['cases_run']} |"
        )

    lines.append("")
    lines.append(
        "`*` no measurement stands behind this axis: the corpus records no timing and no "
        "cost, so the raw value is 0 and the normalizer returns a full mark, and a replayed "
        "step can neither fail nor retry, so every reliability rate is 1.0. Read the "
        "ranking on quality alone."
    )
    lines.append("")
    lines.append(f"winner: {artifact['winner']}")
    lines.append("")
    lines.append("## Per case")
    lines.append("")
    lines.append("| prompt | role | boundary | expected | first run | final run | moved |")
    lines.append("|---|---|---|---|---|---|---|")

    for case in artifact["cases"]:
        first = case["outcomes"].get("first run", "")
        final = case["outcomes"].get("final run", "")
        lines.append(
            f"| {case['prompt_id']} | {case['target_role']} | {case['targeted_boundary']} | "
            f"{case['expected']} | {first} | {final} | {'yes' if case['moved'] else 'no'} |"
        )

    moved = [case["prompt_id"] for case in artifact["cases"] if case["moved"]]

    lines.append("")
    lines.append(
        f"Cases whose outcome moved between revisions: {', '.join(moved) if moved else 'none'}."
    )
    lines.append("")

    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    parser.add_argument(
        "--repo",
        type=Path,
        default=Path(__file__).resolve().parents[1],
        help="repository whose red-team corpus is scored (default: this script's repository)",
    )
    parser.add_argument(
        "--prompts",
        type=Path,
        default=None,
        help="override the prompts document (default: <repo>/eval/red-team-prompts.md)",
    )
    parser.add_argument(
        "--results",
        type=Path,
        default=None,
        help="override the results document (default: <repo>/eval/red-team-results.md)",
    )
    parser.add_argument(
        "--out",
        type=Path,
        default=Path.home() / "console-run-evidence" / "scorecards",
        help="directory the scorecard artifact is written into",
    )
    parser.add_argument(
        "--manifest",
        type=Path,
        default=Path(__file__).resolve().parents[1] / "requirements-scorecard.txt",
        help="the pinned requirements file whose revision is recorded in the artifact",
    )
    parser.add_argument("--latency-target", type=float, default=1.0)
    parser.add_argument("--cost-target", type=float, default=0.01)
    parser.add_argument("--json", action="store_true", help="print the artifact to stdout")
    args = parser.parse_args(argv)

    prompts_path = args.prompts or args.repo / "eval" / "red-team-prompts.md"
    results_path = args.results or args.repo / "eval" / "red-team-results.md"

    for path in (prompts_path, results_path):
        if not path.is_file():
            print(f"error: {path} does not exist; nothing to score.", file=sys.stderr)
            return 2

    try:
        corpus = build_corpus(prompts_path, results_path)
        dataset = build_dataset(corpus)
        scores, detail = score(
            corpus,
            dataset,
            latency_target=args.latency_target,
            cost_target=args.cost_target,
        )
    except ValueError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2

    artifact = build_artifact(
        corpus,
        scores,
        detail,
        latency_target=args.latency_target,
        cost_target=args.cost_target,
        generator=f"{sys.executable} {Path(__file__).name}",
        dependency=engine_provenance(args.manifest),
    )

    args.out.mkdir(parents=True, exist_ok=True)
    json_path = args.out / "redteam-scorecard.json"
    markdown_path = args.out / "redteam-scorecard.md"
    json_path.write_text(json.dumps(artifact, indent=2) + "\n", encoding="utf-8")
    markdown_path.write_text(render_markdown(artifact) + "\n", encoding="utf-8")

    if args.json:
        print(json.dumps(artifact, indent=2))
    else:
        header = f"{'rank':>4}  {'revision':<12}  {'overall':>7}  {'quality':>7}  {'cases':>7}"
        print(header)
        print("-" * len(header))

        for entry in artifact["ranking"]:
            candidate = artifact["candidates"][entry["name"]]
            print(
                f"{entry['rank']:>4}  {entry['name']:<12}  "
                f"{entry['overall_score']:>7.3f}  {entry['quality_score']:>7.3f}  "
                f"{candidate['cases_passed']:>3}/{candidate['cases_run']:<3}"
            )

        moved = [case["prompt_id"] for case in artifact["cases"] if case["moved"]]
        print(f"\nwinner: {artifact['winner']}")
        print(f"moved:  {', '.join(moved) if moved else 'none'}")
        print(
            "\nlatency, cost and reliability carry NO measurement; "
            "the ranking rests on quality alone."
        )

    print(f"\nwrote {json_path}")
    print(f"wrote {markdown_path}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
