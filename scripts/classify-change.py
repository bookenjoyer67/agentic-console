#!/usr/bin/env python3
"""Classify a pull request's changed files as deterministic work or agentic work.

Lesson 4.2, "Identify the Agentic Steps to Add", block [123] (VERBATIM):

    files under `.agents/`, `.skills/`, `mcp-servers/`, `eval/` and
    `docs/governance-policy.md` can "directly change what agents do or what they are
    allowed to do" -> `requires-governed-check=true`;
    "Some files are especially sensitive because they control permissions and access
    boundaries. For those files, the workflow sets a second flag: touches-policy=true."

Lesson 4.2 job table, block [16] (VERBATIM row): "Change Classifier | Which files changed |
No | Every run | No | Job outputs" -- so this step is deterministic, never calls a model, and
produces the two job outputs `requires-governed-check` and `touches-policy` that the eval-gate
job keys off ("You confirmed the eval gate runs on an agent-affecting change--and skips on a
docs-only change.").

Rule, and the authority for each clause
---------------------------------------
A change is AGENTIC when it changes what an agent does or what an agent is allowed to do, or
weakens the evidence the pipeline keeps about either. Everything else is DETERMINISTIC work.

The lesson names the course repository's paths. This repository carries the same classes at
different paths, so the mapping below is the lesson's rule applied to this tree. Each row
names the artifact that makes the file agent-affecting.

| Lesson path              | This repo                                   | Why the same class                                                        |
|--------------------------|---------------------------------------------|---------------------------------------------------------------------------|
| `.agents/`               | `.claude/agents/`                           | role definitions: what each agent does (4.1 per-role policy entries)       |
| `.skills/`               | `.claude/skills/`                           | skill activation scope (the policy's `Skill activation scope` dimension)   |
| `mcp-servers/`           | `mcp/`                                      | tool and operation access (the `_authorize()` guards, 4.1 block [127])     |
| `eval/`                  | `eval/`                                     | the harnesses and suites that judge agent behaviour                        |
| `docs/governance-policy.md` | `docs/governance-policy.md`              | the policy itself                                                          |

Two artifacts this repo adds that the lesson's enumeration does not name but its criterion
covers, because they control permissions and access boundaries:

* `scripts/run-agent.sh` -- the 4.1 container-level enforcement layer.
* `docs/routing-and-tool-grant-map.md` and `.json` -- the grant map the policy is derived from
  (4.1 preamble: "This policy is derived from: The routing-and-tool-grant map").

One artifact the lesson's Pipeline Integrity row covers, because "the CI/CD workflow is now part
of the governed system" (4.2 block [177]):

* `.github/workflows/` -- the gates and the audit trail live here.
* `scripts/classify-change.py`, `scripts/run-reviewer.py`, `scripts/build-audit-trail.py` -- the
  classifier, the review step and the audit-trail assembler. Weakening any of them weakens the
  evidence the pipeline keeps, so they are `requires-governed-check=true`.

`touches-policy=true` is the strictly narrower set: only files that control permissions and
access boundaries. The allow-lists, the policy document, the grant map, the container launcher
and the workflow itself. `eval/` is agent-affecting but grants nothing, so it is
`requires-governed-check=true` and `touches-policy=false` -- the distinction is deliberate.

No model call, no network, no GitHub. Runs locally:

    python3 scripts/classify-change.py path/one.py path/two.md
    git diff --name-only origin/main...HEAD | python3 scripts/classify-change.py
    python3 scripts/classify-change.py --github-output            # appends to $GITHUB_OUTPUT
"""

from __future__ import annotations

import argparse
import fnmatch
import json
import os
import sys
from pathlib import Path

RULE_VERSION = "m4-4.2-v1"

# --- The portability seam: the two glob tables live in agentic.config.json --------------------
# `classification.governed_globs` and `classification.policy_globs` carry this repository's tables,
# so a fork reclassifies by editing the config instead of this file. The loader is stdlib only and
# falls back to its own embedded defaults, so the classifier behaves exactly as it does today when
# the config file is absent.
_SCRIPTS = next(
    (
        parent / "scripts"
        for parent in Path(__file__).resolve().parents
        if (parent / "scripts" / "agentic_config.py").is_file()
    ),
    Path(__file__).resolve().parents[1] / "scripts",
)
if str(_SCRIPTS) not in sys.path:
    sys.path.insert(0, str(_SCRIPTS))
import agentic_config  # noqa: E402 - the loader sits in the path inserted above


def _dig(data: object, dotted_key: str):
    """One value from a nested dict by dotted key, or None."""
    node = data
    for part in dotted_key.split("."):
        if not isinstance(node, dict) or part not in node:
            return None
        node = node[part]
    return node


def _glob_table(dotted_key: str) -> tuple[str, ...]:
    """One glob table from the config as a tuple of strings.

    A value that is not a list of strings would govern nothing, which is the dangerous direction, so
    the embedded default answers for it rather than leaving the gate unarmed.
    """
    configured = agentic_config.get(dotted_key)
    if not (isinstance(configured, (list, tuple)) and all(isinstance(e, str) for e in configured)):
        configured = _dig(agentic_config.DEFAULT, dotted_key)
    if not isinstance(configured, (list, tuple)):
        configured = ()
    return tuple(str(entry) for entry in configured)


# The classifier also enforces these policy globs, which `classification.policy_globs` does not
# The operation guards that decide who may call what, the container permission layer and the
# gates and the audit trail all control permissions and access boundaries exactly as the
# configured globs do. All of them now live in `classification.policy_globs`, because a fork
# reading that key must get the whole set: a glob left behind in this file is a seam the fork
# cannot see, and scripts/port-self-test.sh exists to catch exactly that. The alias stays so
# the history of this list is legible.
POLICY_GLOBS_EXTRA: tuple[str, ...] = ()

# Files that can change what agents do or what they are allowed to do, or that carry the
# evidence about either. Lesson 4.2 block [123], applied to this repository's layout: the mapping
# from the lesson's paths to this tree's paths (`.agents/` -> `.claude/agents/`, `.skills/` ->
# `.claude/skills/`, `mcp-servers/` -> `mcp/`) and the reason each path is agent-affecting is the
# table in the module docstring above. The values are `classification.governed_globs`.
GOVERNED_GLOBS: tuple[str, ...] = _glob_table("classification.governed_globs")

# The strictly narrower subset: files that control permissions and access boundaries.
# Lesson 4.2 block [123]: "Some files are especially sensitive because they control
# permissions and access boundaries. For those files, the workflow sets a second flag:
# touches-policy=true." The whole set is `classification.policy_globs`.
POLICY_GLOBS: tuple[str, ...] = _glob_table("classification.policy_globs") + POLICY_GLOBS_EXTRA

# What this classifier consumes, for `--print-config` and for scripts/port-self-test.sh.
CONFIG_KEYS: tuple[str, ...] = (
    "classification.governed_globs",
    "classification.policy_globs",
)
if "--print-config" in sys.argv[1:]:
    print(
        json.dumps(
            {
                "classification.governed_globs": list(GOVERNED_GLOBS),
                "classification.policy_globs": list(POLICY_GLOBS),
            },
            indent=2,
            sort_keys=True,
        )
    )
    raise SystemExit(0)


def _matches(path: str, globs: tuple[str, ...]) -> bool:
    """True when `path` matches any of `globs`, on a repo-relative POSIX path."""
    return any(fnmatch.fnmatchcase(path, pattern) for pattern in globs)


def normalise(raw: str) -> str:
    """Turn a git/`gh` path into a repo-relative POSIX path, or "" when unusable."""
    path = raw.strip()
    if not path:
        return ""
    # `git diff --name-status` prefixes a status column; `--name-only` does not.
    if len(path) > 2 and path[1] == "\t":
        path = path[2:]
    path = path.replace("\\", "/")
    while path.startswith("./"):
        path = path[2:]
    return path.lstrip("/")


def classify_file(path: str) -> tuple[str, bool, bool]:
    """Return (class, requires_governed_check, touches_policy) for one repo-relative path."""
    governed = _matches(path, GOVERNED_GLOBS)
    policy = _matches(path, POLICY_GLOBS)
    # A policy file is by definition agent-affecting, whatever the globs say.
    governed = governed or policy
    return ("agentic" if governed else "deterministic", governed, policy)


def classify(paths: list[str]) -> dict:
    """Classify every path and assemble the machine-readable result."""
    agentic: list[str] = []
    deterministic: list[str] = []
    policy_files: list[str] = []
    reasons: dict[str, str] = {}

    for path in sorted(set(paths)):
        kind, governed, policy = classify_file(path)
        if governed:
            agentic.append(path)
            reasons[path] = (
                "touches-policy: controls permissions or access boundaries"
                if policy
                else "requires-governed-check: changes what agents do or are allowed to do"
            )
        else:
            deterministic.append(path)
            reasons[path] = "deterministic work: changes no agent capability or permission"
        if policy:
            policy_files.append(path)

    return {
        "rule_version": RULE_VERSION,
        "changed_files": sorted(set(paths)),
        "file_count": len(set(paths)),
        "change_type": "agentic" if agentic else "deterministic",
        "agentic_files": agentic,
        "deterministic_files": deterministic,
        "policy_files": policy_files,
        "touches_policy": bool(policy_files),
        "requires_governed_check": bool(agentic),
        "reasons": reasons,
    }


def read_paths(argv: list[str]) -> list[str]:
    """Read the file list from argv when given, otherwise from stdin."""
    if argv:
        source = argv
    else:
        if sys.stdin.isatty():
            return []
        source = sys.stdin.read().splitlines()
    return [p for p in (normalise(line) for line in source) if p]


def write_github_output(result: dict, path: str) -> None:
    """Append the two lesson-named job outputs: requires-governed-check, touches-policy."""
    with open(path, "a", encoding="utf-8") as handle:
        handle.write(
            "requires-governed-check={}\n".format(
                "true" if result["requires_governed_check"] else "false"
            )
        )
        handle.write(
            "touches-policy={}\n".format("true" if result["touches_policy"] else "false")
        )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Classify changed files as deterministic or agentic work (lesson 4.2)."
    )
    parser.add_argument("paths", nargs="*", help="changed file paths; stdin when omitted")
    parser.add_argument("--out", help="also write the JSON result to this path")
    parser.add_argument(
        "--github-output",
        nargs="?",
        const=os.environ.get("GITHUB_OUTPUT", ""),
        help="append the two job outputs to this file (default: $GITHUB_OUTPUT)",
    )
    parser.add_argument(
        "--quiet", action="store_true", help="write no JSON to stdout"
    )
    args = parser.parse_args(argv)

    paths = read_paths(args.paths)
    result = classify(paths)

    payload = json.dumps(result, indent=2, sort_keys=False)
    if args.out:
        with open(args.out, "w", encoding="utf-8") as handle:
            handle.write(payload + "\n")
    if args.github_output:
        write_github_output(result, args.github_output)
    if not args.quiet:
        print(payload)
    return 0


if __name__ == "__main__":
    sys.exit(main())
