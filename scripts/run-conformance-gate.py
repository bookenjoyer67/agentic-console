#!/usr/bin/env python3
"""The ``conformance`` gate: run the deterministic prose-and-citation check, fail on NEW drift only.

Why this file exists
--------------------
``scripts/validate_doc_conformance_deterministic.py`` implements the converted step, but inside the
workflow nothing could run it: the gate server is the only execution path any role holds, and it
exposed three name-only Rust gates. The ``conformance`` gate closes that gap. It is invoked by name,
with no arguments, from the gate vocabulary in ``agentic.config.json``:

    "conformance": {
      "argv": ["python3", "scripts/run-conformance-gate.py"],
      ...
    }

The wrapper takes no argument of its own. Any argument is refused with exit 2 and one line on
stderr, so there is no flag, no path and no mode a caller can contribute. The prose file set comes
from the config key ``gates.conformance.files`` (``agentic.config.json``), not from a caller and not
from a literal inside this file; the embedded default below is this repository's copy of that list,
so a run with no config file behaves exactly as a run with it.

The verdict: new drift only
---------------------------
The repository's ten configured files carry 135 pre-existing findings at HEAD, so a gate that failed
on any finding would be red on every run and would be switched off. This gate compares, per file, the
working-tree findings against the same file at ``HEAD`` -- exactly the comparison the CI fmt gate
makes between base and current -- and fails only when a rule's finding count INCREASES:

* exit 0 when every rule's count is equal to, or lower than, its count at HEAD;
* exit 1 when any rule (R1, R2, R3, R4 or CIT) finds more than it found at HEAD;
* exit 2 when the invocation itself is wrong: an argument, a missing checker, a config file naming a
  file the tree does not have, or a tree that is not a git checkout.

What is compared, and what is not
---------------------------------
* R1-R4 and the citation findings are compared as per-file counts against ``HEAD``. A count that
  falls is a repair and passes; a count that rises is new drift and fails. Pre-existing findings
  never fail the gate.
* The ``HEAD`` copy of a file is materialised into a scratch directory with ``git show HEAD:<path>``.
  The checker resolves citations against the repository root it is handed, and BOTH passes are handed
  the same root. The honest consequence, printed as ``citation_comparison`` in every report: a
  citation that points into another file resolves identically in both passes, and only the file's own
  citation LIST can differ; a citation a file makes into ITSELF therefore resolves against the
  working-tree copy in the base pass too. So a citation added or removed by the change under test is
  visible as new drift, and a line shifted inside the file under test is not. The document rule set
  R1-R4 is compared on the file's own text in both passes.
* A file ``HEAD`` does not carry has no baseline. It is reported with ``base_comparable: false`` and
  its findings are not counted as new, because there is nothing for them to have drifted from; the
  reason is printed next to the file.
* Only the per-rule counts and the new findings are reported; the full checker report is discarded.
  The summary holds no timestamp, so two runs over one tree print the same object.
"""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
CHECKER = ROOT / "scripts" / "validate_doc_conformance_deterministic.py"
# Reading a file's `HEAD` revision needs git to trust the tree, and the sandbox container mounts this
# repository owned by another uid, so every git call carries `-c safe.directory=<root>` for this
# script's own root -- never for a caller-supplied path, which the empty argv rules out anyway.
GIT: tuple[str, ...] = ("git", "-c", f"safe.directory={ROOT}", "-C", str(ROOT))
CONFIG_KEY = "gates.conformance.files"
RULES: tuple[str, ...] = ("R1", "R2", "R3", "R4", "CIT")

# This repository's copy of `gates.conformance.files`, so the gate runs unchanged on a tree with no
# config file. Keep it in step with `agentic.config.json` the way `scripts/agentic_config.py` does.
DEFAULT_FILES: tuple[str, ...] = (
    "AGENTS.md",
    "CLAUDE.md",
    "docs/DOC-STYLE.md",
    "docs/governance-policy.md",
    "docs/routing-and-tool-grant-map.md",
    "docs/policy-reconciliation.md",
    "docs/step-classification.md",
    "docs/iteration-log.md",
    "docs/memory-architecture.md",
    "docs/orchestration-diagram.md",
)

CITATION_COMPARISON = (
    "Both passes resolve citations against the repository root, so only the checked file's own "
    "citation list can differ between the base pass and the current pass; a citation that points "
    "into another file resolves identically in both, and a citation a file makes into itself "
    "resolves against the working-tree copy in the base pass too."
)


def refuse() -> int:
    """Refuse any argument this gate is handed, with one line on stderr and exit 2."""
    print(
        "conformance gate: this gate takes no argument; it is invoked by name with an empty argv",
        file=sys.stderr,
    )
    return 2


def fail(message: str) -> int:
    """Report an invocation that cannot be evaluated, with one line on stderr and exit 2."""
    print(f"conformance gate: {message}", file=sys.stderr)
    return 2


def listed_files() -> tuple[list[str], str]:
    """The prose files to check, from the config key, or the embedded default list."""
    _scripts = ROOT / "scripts"
    if str(_scripts) not in sys.path:
        sys.path.insert(0, str(_scripts))
    import agentic_config  # noqa: PLC0415 - the loader sits in the path inserted above

    configured = agentic_config.get(CONFIG_KEY)
    if (
        isinstance(configured, list)
        and configured
        and all(isinstance(entry, str) and entry.strip() for entry in configured)
    ):
        return [entry.strip() for entry in configured], str(agentic_config.source())
    return list(DEFAULT_FILES), "embedded-defaults"


def run_checker(target: Path, report_path: Path) -> tuple[dict[str, Any] | None, str | None]:
    """Check one file and return its parsed report, or None and the reason it could not be read."""
    completed = subprocess.run(  # noqa: S603 - a fixed argv, never a caller-supplied string
        [
            sys.executable,
            str(CHECKER),
            "--input",
            str(target),
            "--output",
            str(report_path),
            "--repo",
            str(ROOT),
        ],
        cwd=str(ROOT),
        capture_output=True,
        text=True,
        check=False,
    )
    if completed.returncode == 2:
        return None, completed.stderr.strip() or "the checker refused the invocation"
    try:
        return json.loads(report_path.read_text(encoding="utf-8")), None
    except (OSError, ValueError):
        return None, f"the checker wrote no readable report (exit code {completed.returncode})"


def counts(report: dict[str, Any]) -> dict[str, int]:
    """One report's findings per rule, with a zero for every rule that found nothing."""
    tally = {rule: 0 for rule in RULES}
    for finding in report["inputs"][0]["violations"]:
        tally[finding["rule"]] = tally.get(finding["rule"], 0) + 1
    return tally


def identity(finding: dict[str, Any]) -> tuple[str, str, str, str]:
    """What makes two findings the same finding: its rule, code, citation and quoted literal."""
    return (
        str(finding.get("rule", "")),
        str(finding.get("code", "")),
        str(finding.get("citation", "")),
        str(finding.get("literal", "")),
    )


def new_findings(current: list[dict[str, Any]], base: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """The findings the current pass holds that the base pass did not, matched by identity."""
    remaining: dict[tuple[str, str, str, str], int] = {}
    for finding in base:
        key = identity(finding)
        remaining[key] = remaining.get(key, 0) + 1
    fresh: list[dict[str, Any]] = []
    for finding in current:
        key = identity(finding)
        if remaining.get(key, 0):
            remaining[key] -= 1
            continue
        fresh.append(finding)
    return fresh


def head_revision(relative: str) -> tuple[str | None, str | None]:
    """The file's text at HEAD, or None and why it could not be read."""
    completed = subprocess.run(  # noqa: S603 - a fixed argv, never a caller-supplied string
        [*GIT, "show", f"HEAD:{relative}"],
        capture_output=True,
        text=True,
        check=False,
    )
    if completed.returncode != 0:
        detail = completed.stderr.strip().splitlines()
        return None, detail[0] if detail else f"git show HEAD:{relative} failed"
    return completed.stdout, None


def main() -> int:
    """Check every configured file against its HEAD revision and print the JSON verdict."""
    if len(sys.argv) > 1:
        return refuse()
    if not CHECKER.is_file():
        return fail(f"{CHECKER.relative_to(ROOT)} is missing, so there is nothing to run")

    probe = subprocess.run(  # noqa: S603 - a fixed argv, never a caller-supplied string
        [*GIT, "rev-parse", "--verify", "--quiet", "HEAD^{commit}"],
        capture_output=True,
        text=True,
        check=False,
    )
    if probe.returncode != 0:
        return fail(f"{ROOT} is not a git checkout with a HEAD commit, so no base can be compared")

    files, config_source = listed_files()
    missing = [name for name in files if not (ROOT / name).is_file()]
    if missing:
        return fail(f"{CONFIG_KEY} names a file the tree does not have: {missing[0]}")

    results: list[dict[str, Any]] = []
    with tempfile.TemporaryDirectory(prefix="conformance-gate-") as scratch_name:
        scratch = Path(scratch_name)
        for name in files:
            current_report, current_error = run_checker(ROOT / name, scratch / "current.json")
            if current_report is None:
                return fail(f"the checker could not read {name}: {current_error}")
            current = counts(current_report)
            current_findings = current_report["inputs"][0]["violations"]

            entry: dict[str, Any] = {"file": name, "current": current}
            base_text, base_error = head_revision(name)
            if base_text is None:
                entry.update(
                    {
                        "base_comparable": False,
                        "base": None,
                        "reason": f"no baseline in HEAD: {base_error}",
                        "new_findings": [],
                        "verdict": "pass",
                    }
                )
                results.append(entry)
                continue

            base_copy = scratch / name.replace("/", "__")
            base_copy.write_text(base_text, encoding="utf-8")
            base_report, base_check_error = run_checker(base_copy, scratch / "base.json")
            if base_report is None:
                return fail(f"the checker could not read HEAD:{name}: {base_check_error}")
            base = counts(base_report)

            fresh = new_findings(current_findings, base_report["inputs"][0]["violations"])
            risen = {rule: current[rule] - base[rule] for rule in RULES if current[rule] > base[rule]}
            entry.update(
                {
                    "base_comparable": True,
                    "base": base,
                    "new_findings": fresh,
                    "verdict": "fail" if risen else "pass",
                }
            )
            if risen:
                entry["reason"] = "; ".join(
                    f"{rule} found {current[rule]} where HEAD found {base[rule]}" for rule in risen
                )
            else:
                entry["reason"] = "no rule found more than its HEAD count"
            results.append(entry)

    failed = [entry for entry in results if entry["verdict"] == "fail"]
    if failed:
        verdict = "fail"
        reason = "new conformance drift against HEAD: " + "; ".join(
            f"{entry['file']} ({entry['reason']})" for entry in failed
        )
    else:
        verdict = "pass"
        reason = (
            "no rule's finding count rose against HEAD"
            if all(entry["base_comparable"] for entry in results)
            else "no rule's finding count rose against HEAD where a HEAD baseline exists"
        )

    summary = {
        "gate": "conformance",
        "verdict": verdict,
        "reason": reason,
        "base_revision": "HEAD",
        "config_key": CONFIG_KEY,
        "config_source": config_source,
        "citation_comparison": CITATION_COMPARISON,
        "files": results,
        "totals": {
            "base": sum(
                entry["base"][rule]
                for entry in results
                if entry["base"] is not None
                for rule in RULES
            ),
            "current": sum(entry["current"][rule] for entry in results for rule in RULES),
            "files_checked": len(results),
            "files_without_a_baseline": sum(1 for entry in results if not entry["base_comparable"]),
        },
    }
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
