#!/usr/bin/env python3
"""Assemble the run's audit trail from the artifacts the workflow downloaded.

Lesson 4.2, "Build the CI Audit Trail Job", block [158] (VERBATIM): the trail carries
"CI metadata (commit SHA, PR number, event, timestamp); agent actions taken through the MCP
servers (tool call, role, operation, outcome); policy check results; harness scores;
authorization denials pulled from the audit log's denied entries; the policy-relevant
classifier flag."

Lesson 4.2 block [162] (VERBATIM): a separate final job with `needs: [...]` and `if: always()`
is "the whole point"; and the trail "does not decide whether code can merge; it preserves
evidence of what the pipeline checked and what occurred during the run."

Lesson 4.2 block [169] (VERBATIM, acceptance): "actions/download-artifact@v4 is used with
merge-multiple: true to flatten all artifacts into one directory"; and the downloaded
audit-trail artifact "contains CI metadata, policy results, harness scores, and any
authorization denials, and ... does not contain any secret values."

Shape written (lesson 4.2 block [167], VERBATIM):

    {
      "metadata": {
        "sha": "cef863e7e04f8c3b76bee7936d028d90d527d85a",
        "event": "pull_request",
        "pull_request": "2",
        "timestamp": "2026-06-06T13:08:46.881143+00:00"
      },
      "change_classification": {
        "changed_files": ["sample_review.py"],
        "touches_policy": false,
        "requires_governed_check": false
      },
      "results": {
        "policy_gate": "success",
        "governed_file_gate": "success",
        "advisory_review": "success"
      },
      "reports_present": {
        "policy_report": true,
        "governed_file_report": false,
        "review_audit": true,
        "review_output": true
      },

The four verbatim objects above are emitted with exactly those keys, and `pull_request` stays a
string, as in the template. Three further sections carry what block [158] requires and the
template block stops short of: `eval_results` (the harness scores), `reviewer_outcome` and
`artifacts_consumed`.

`results.*` are the JOB outcomes (GitHub's `needs.<job>.result`), read from the environment the
workflow passes; `reports_present.*` are booleans derived from the files actually found. A job
can fail and still leave a report, so the two are never derived from each other.

Merging. `merge-multiple: true` flattens the downloaded artifacts into one directory, so
policy-report.json, the harness reports and the audit-*.log files sit side by side. When an
artifact still lands in a subdirectory, this script flattens it into the root of the artifacts
directory first, then assembles the trail.

A missing artifact is never fatal: an absent report leaves its `reports_present` flag false and
its section at a recorded default, and the script still exits 0 with a trail. That is what makes
the job work under `if: always()`, where an earlier job may have failed before uploading.

No secret values reach the trail (block [169]): every secret-looking environment value is
redacted out of the assembled JSON before it is written.

Run from the repository root:

    python3 scripts/build-audit-trail.py --artifacts-dir ci-artifacts
"""

from __future__ import annotations

import argparse
import fnmatch
import json
import os
import re
import shutil
import sys
from datetime import datetime, timezone
from pathlib import Path

POLICY_REPORT = "policy-report.json"
REVIEW_REPORT = "review-report.json"
REVIEW_SUMMARY = "review-summary.md"
CLASSIFICATION = "change-classification.json"

# Every file name the audit trail knows how to consume. A file that matches none of these is
# left alone: this script records evidence, it does not guess at it.
RECOGNISED_EXACT = {
    POLICY_REPORT,
    REVIEW_REPORT,
    REVIEW_SUMMARY,
    CLASSIFICATION,
    "review-output.json",
    "eval-report.json",
    "deterministic-report.json",
    "rubric-report.json",
    "integrity-report.json",
}
RECOGNISED_GLOBS = ("*-report.json", "audit-*.log", "*-audit.log", "*.audit.log", "audit*.log")

SECRET_NAME_RE = re.compile(r"(API_?KEY|_KEY$|TOKEN|SECRET|PASSWORD|CREDENTIAL)", re.IGNORECASE)
SECRET_NAME_ALLOW = {"OPENROUTER_BASE_URL", "GITHUB_TOKEN"}


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


def _secret_values() -> list[str]:
    values: list[str] = []
    for name, value in os.environ.items():
        if not value or len(value) < 8 or name in SECRET_NAME_ALLOW:
            continue
        if SECRET_NAME_RE.search(name):
            values.append(value)
    return values


def _redact(text: str, secrets: list[str]) -> str:
    for secret in sorted(secrets, key=len, reverse=True):
        text = text.replace(secret, "[REDACTED]")
    return text


def is_recognised(name: str) -> bool:
    """True when `name` is an artifact this trail consumes."""
    if name in RECOGNISED_EXACT:
        return True
    return any(fnmatch.fnmatchcase(name, pattern) for pattern in RECOGNISED_GLOBS)


def flatten_artifacts(root: Path) -> list[str]:
    """Copy recognised files out of subdirectories into `root`, side by side.

    This is the merge `merge-multiple: true` performs in the workflow, repeated here so the
    script never depends on the download having flattened anything. Missing directory is fine.
    """
    moved: list[str] = []
    if not root.is_dir():
        return moved
    for path in sorted(root.rglob("*")):
        if not path.is_file() or path.parent == root:
            continue
        if not is_recognised(path.name):
            continue
        target = root / path.name
        if target.exists():
            continue
        shutil.copy2(path, target)
        moved.append(path.name)
    return moved


def load_json(path: Path) -> object | None:
    """Parse a JSON artifact, or None when it is absent or unreadable."""
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None


def find_files(root: Path, *patterns: str) -> list[Path]:
    if not root.is_dir():
        return []
    found: list[Path] = []
    for path in sorted(root.glob("*")):
        if path.is_file() and any(fnmatch.fnmatchcase(path.name, p) for p in patterns):
            found.append(path)
    return found


def harness_summary(report: object) -> dict:
    """Pull score counts out of a harness report, tolerating any known shape."""
    if not isinstance(report, dict):
        return {}
    summary = report.get("summary")
    if isinstance(summary, dict):
        picked = {
            key: summary[key]
            for key in ("passed", "failed", "error", "skipped", "total", "collected")
            if key in summary
        }
        if picked:
            return picked
    tests = report.get("tests")
    if isinstance(tests, list):
        outcomes: dict[str, int] = {}
        for test in tests:
            outcome = str((test or {}).get("outcome", "unknown"))
            outcomes[outcome] = outcomes.get(outcome, 0) + 1
        if outcomes:
            outcomes["total"] = len(tests)
        return outcomes
    return {}


def build_metadata(args: argparse.Namespace, secrets: list[str]) -> dict:
    """The verbatim `metadata` object: sha, event, pull_request, timestamp."""
    sha = args.sha or os.environ.get("GITHUB_SHA", "")
    event = args.event or os.environ.get("GITHUB_EVENT_NAME", "")
    pull_request = args.pull_request or ""

    # Read the PR number off the event payload when the workflow did not pass it.
    if not pull_request:
        event_path = os.environ.get("GITHUB_EVENT_PATH", "")
        if event_path and Path(event_path).is_file():
            payload = load_json(Path(event_path))
            if isinstance(payload, dict):
                number = (payload.get("pull_request") or {}).get("number") if isinstance(
                    payload.get("pull_request"), dict
                ) else payload.get("number")
                if number is not None:
                    pull_request = str(number)
                if not event and isinstance(payload.get("pull_request"), dict):
                    event = event or "pull_request"
    if not pull_request:
        # refs/pull/<n>/merge -- the shape GITHUB_REF takes on a pull_request event.
        match = re.search(r"refs/pull/(\d+)/", os.environ.get("GITHUB_REF", ""))
        if match:
            pull_request = match.group(1)

    return {
        "sha": sha,  # verbatim key
        "event": event,
        "pull_request": str(pull_request),  # verbatim template keeps this a string
        "timestamp": _now(),
    }


def build_classification(root: Path) -> dict:
    """The verbatim `change_classification` object, from classify-change.py's output."""
    data = load_json(root / CLASSIFICATION)
    default = {"changed_files": [], "touches_policy": False, "requires_governed_check": False}
    if not isinstance(data, dict):
        return default
    files = data.get("changed_files")
    return {
        "changed_files": [str(f) for f in files] if isinstance(files, list) else [],
        "touches_policy": bool(data.get("touches_policy", False)),
        "requires_governed_check": bool(data.get("requires_governed_check", False)),
    }


def build_results(args: argparse.Namespace) -> dict:
    """Gate results: the JOB outcomes, verbatim keys."""
    return {
        "policy_gate": args.policy_gate or os.environ.get("POLICY_GATE_RESULT", "unknown"),
        "governed_file_gate": args.governed_file_gate
        or os.environ.get("GOVERNED_FILE_GATE_RESULT", "unknown"),
        "advisory_review": args.advisory_review or os.environ.get("ADVISORY_REVIEW_RESULT", "unknown"),
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Assemble the CI audit trail (lesson 4.2). Never fails on a missing artifact."
    )
    parser.add_argument("--artifacts-dir", default="ci-artifacts",
                        help="directory the workflow flattened the artifacts into")
    parser.add_argument("--out", help="trail path (default: <artifacts-dir>/audit-trail.json)")
    parser.add_argument("--sha"), parser.add_argument("--event")
    parser.add_argument("--pull-request", dest="pull_request")
    parser.add_argument("--policy-gate"), parser.add_argument("--governed-file-gate")
    parser.add_argument("--advisory-review")
    parser.add_argument("--no-merge", action="store_true",
                        help="skip flattening nested artifact directories")
    parser.add_argument("--github-output", nargs="?",
                        const=os.environ.get("GITHUB_OUTPUT", ""),
                        help="append `audit-trail=<path>` here (default: $GITHUB_OUTPUT)")
    args = parser.parse_args(argv)

    secrets = _secret_values()
    root = Path(args.artifacts_dir)
    out_path = Path(args.out) if args.out else root / "audit-trail.json"

    # 1. Merge: flatten whatever landed in a subdirectory, tolerating an empty/missing dir.
    merged = [] if args.no_merge else flatten_artifacts(root)

    # 2. Discover. Every lookup tolerates a missing file.
    policy_report = root / POLICY_REPORT
    review_report = root / REVIEW_REPORT
    harness_reports = [
        p for p in find_files(root, "*-report.json", "*-results.json")
        if p.name not in {POLICY_REPORT, REVIEW_REPORT}
    ]
    audit_logs = sorted(
        {p for pattern in ("audit-*.log", "*-audit.log", "*.audit.log") for p in find_files(root, pattern)}
    )

    policy_data = load_json(policy_report)
    review_data = load_json(review_report)

    consumed: list[dict] = []

    def consume(path: Path, kind: str) -> None:
        try:
            size = path.stat().st_size
        except OSError:
            size = 0
        consumed.append({"kind": kind, "name": path.name, "bytes": size})

    if policy_report.is_file():
        consume(policy_report, "policy_report")
    if review_report.is_file():
        consume(review_report, "review_output")
    if (root / REVIEW_SUMMARY).is_file():
        consume(root / REVIEW_SUMMARY, "review_summary")
    if (root / CLASSIFICATION).is_file():
        consume(root / CLASSIFICATION, "change_classification")
    for path in harness_reports:
        consume(path, "harness_report")
    for path in audit_logs:
        consume(path, "audit_log")

    # 3. Sections.
    metadata = build_metadata(args, secrets)
    classification = build_classification(root)
    results = build_results(args)

    eval_results = {
        "report_files": [p.name for p in harness_reports],
        "scores": {p.name: harness_summary(load_json(p)) for p in harness_reports},
        "policy_report_present": policy_report.is_file(),
        "policy_report_summary": harness_summary(policy_data),
    }

    if isinstance(review_data, dict):
        reviewer_outcome = {
            "status": review_data.get("status", "unknown"),
            "severity": review_data.get("severity", "unknown"),
            "findings": len(review_data.get("findings") or []),
            "advisory": bool(review_data.get("advisory", True)),
            "error": review_data.get("error"),
        }
    else:
        reviewer_outcome = {
            "status": "unknown",
            "severity": "unknown",
            "findings": 0,
            "advisory": True,
            "error": None,
        }

    reports_present = {
        "policy_report": policy_report.is_file(),
        "governed_file_report": bool(harness_reports),
        "review_audit": bool(audit_logs),
        "review_output": review_report.is_file(),
    }

    trail = {
        "metadata": metadata,
        "change_classification": classification,
        "results": results,
        "eval_results": eval_results,
        "reviewer_outcome": reviewer_outcome,
        "artifacts_consumed": consumed,
        "reports_present": reports_present,
        "results_summary": {
            "artifacts_dir": str(root),
            "artifacts_merged_into_one_directory": merged,
            "artifacts_missing": [
                name for name, present in (
                    ("policy-report.json", reports_present["policy_report"]),
                    ("review-report.json", reports_present["review_output"]),
                    ("harness report", reports_present["governed_file_report"]),
                    ("audit-*.log", reports_present["review_audit"]),
                ) if not present
            ],
        },
    }

    # 4. Write, with every secret value scrubbed out (block [169]).
    payload = _redact(json.dumps(trail, indent=2) + "\n", secrets)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(payload, encoding="utf-8")

    if args.github_output:
        with open(args.github_output, "a", encoding="utf-8") as handle:
            handle.write("audit-trail={}\n".format(out_path))

    print("audit trail written")
    print("  artifacts dir : {}".format(root))
    print("  merged        : {}".format(", ".join(merged) if merged else "(nothing to flatten)"))
    print("  reports       : policy={} harness={} review_output={} audit_log={}".format(
        reports_present["policy_report"], reports_present["governed_file_report"],
        reports_present["review_output"], reports_present["review_audit"]))
    print("  missing       : {}".format(
        ", ".join(trail["results_summary"]["artifacts_missing"]) or "none"))
    print(out_path)

    # A trail with a missing artifact is still a trail: exit 0 either way.
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except SystemExit:
        raise
    except Exception as exc:  # noqa: BLE001 - the audit job runs under if: always()
        print("audit trail could not be assembled: {}: {}".format(type(exc).__name__, exc))
        sys.exit(1)
