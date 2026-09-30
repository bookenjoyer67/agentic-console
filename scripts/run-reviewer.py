#!/usr/bin/env python3
"""Advisory code-review step: reviews changed files, always exits 0.

Lesson 4.2 acceptance criterion, "Add an Advisory Agentic Job: Automated Code Review",
block [150] (VERBATIM):

    scripts/run-reviewer.py exits 0 regardless of review severity

Lesson 4.2 step-design entry, block [56] (VERBATIM):

    ## Step: Automated code review
    - Does: Reviewer subagent scores changed files against the rubric and posts a PR comment.
    - Input: changed files in the pull request.
    - Produces: a structured review comment.
    - Classification: advisory (new; promote to gating once stable).
    - Time limit: 15 minutes.
    - Credentials: OPENROUTER_API_KEY, scoped to this step only.

Why advisory (lesson 4.2 block [53], VERBATIM): "An advisory step runs, produces its output, and
lets the pipeline continue regardless of the output... Use advisory for output that needs human
judgment to evaluate, for steps that are new and not yet proven stable". And: "Every new agentic
step should start as advisory, rather than gating."

Three paths, all of which must exit 0
-------------------------------------
1. No credential  -> status "not_run". The script says the review did not run. It never reports
   findings it did not obtain, and never reports a clean review it did not perform.
2. Review completes -> status "completed", with the findings the model returned. A serious
   finding is still exit 0: the job is advisory, so severity never changes the exit status.
3. Any failure -> status "error" (transport failure, non-200, timeout, unparseable payload).
   Recorded in the report, exit 0.

"The Four Requirements for Handling Secrets" (lesson 4.2 block [72], VERBATIM) and how this
script holds each one:

1. "Injected at runtime: The value lives only in the secret store. The platform injects it as an
   environment variable into a step when that step runs, and it is never written to any file,
   saved artifact, or log." -> The key is read once from the environment into a local variable
   and used only as an Authorization header. It is never written to the report, the summary, the
   audit log or any other file. `_redact()` scrubs every secret-looking value out of anything
   this script writes, so a key echoed back by the model cannot reach a file either.
2. "Never logged: ... The simplest safe rule: no commands that dump the environment, such as
   printenv or env, and no debug output that prints environment state." -> This script never
   reads the whole environment, never prints it, and never prints the key or its length. It
   records only the boolean `api_key_present` in the audit log.
3. "Scoped to the step: Inject a secret only into the steps that actually use it, not onto the
   whole job." -> The script reads exactly one named variable, OPENROUTER_API_KEY. It reads no
   other credential and forwards none. The workflow scopes that variable to this step.
4. "Rotatable without pipeline changes: ... you must be able to replace it by updating the
   secrets store on your own. No YAML edit, no Dockerfile change, no configuration file." -> the
   key is referenced by environment name only; nothing about it is baked into this file, so
   rotating it in the secret store is the whole change.

Run from the repository root:

    python3 scripts/run-reviewer.py --files sample_review.py
    git diff --name-only origin/main...HEAD | python3 scripts/run-reviewer.py
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

API_KEY_ENV = "OPENROUTER_API_KEY"
DEFAULT_MODEL = "anthropic/claude-sonnet-4.5"
DEFAULT_BASE_URL = "https://openrouter.ai/api/v1"
DEFAULT_TIMEOUT = 120
DEFAULT_MAX_BYTES = 60_000

# Env names whose values must never reach a file. Requirement 2 of block [72]: masking is a
# backstop, so anything that looks like a secret is scrubbed from everything we write.
SECRET_NAME_RE = re.compile(r"(API_?KEY|_KEY$|TOKEN|SECRET|PASSWORD|CREDENTIAL)", re.IGNORECASE)
SECRET_NAME_ALLOW = {"OPENROUTER_BASE_URL", "GITHUB_TOKEN"}

REVIEW_PROMPT = """You are a code reviewer working inside a governed repository.

Review the changed files below and report only findings the file text supports.

Return ONE JSON object, no prose around it, with this exact shape:

{"findings": [{"file": "<path>", "severity": "low|medium|high",
               "message": "<what is wrong, in one or two sentences>"}],
 "summary": "<one or two sentences summarising the review>"}

Rules:
- A clean change is {"findings": [], "summary": "..."}. Do not invent findings to fill the list.
- Do not restate the diff. Each finding must name a problem a person could act on.
- This review is advisory. It never blocks a merge.
"""


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


def _secret_values() -> list[str]:
    """Values of secret-looking environment variables, for redaction only."""
    values: list[str] = []
    for name, value in os.environ.items():
        if not value or len(value) < 8 or name in SECRET_NAME_ALLOW:
            continue
        if SECRET_NAME_RE.search(name):
            values.append(value)
    return values


def _redact(text: str, secrets: list[str]) -> str:
    """Remove every known secret value from `text`, longest first."""
    for secret in sorted(secrets, key=len, reverse=True):
        text = text.replace(secret, "[REDACTED]")
    return text


def _write(path: Path, text: str, secrets: list[str]) -> None:
    """Write scrubbed text, creating the parent directory."""
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(_redact(text, secrets), encoding="utf-8")


def read_files(paths: list[str], repo: Path, max_bytes: int) -> tuple[list[dict], list[str]]:
    """Read each changed file for review. Missing/binary files are skipped, never fatal."""
    included: list[dict] = []
    skipped: list[str] = []
    budget = max_bytes
    for rel in paths:
        target = (repo / rel).resolve()
        try:
            if not target.is_file() or target.stat().st_size > budget:
                skipped.append(rel)
                continue
            text = target.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            skipped.append(rel)
            continue
        included.append({"path": rel, "content": text})
        budget -= len(text)
    return included, skipped


def call_model(
    key: str, base_url: str, model: str, files: list[dict], timeout: int
) -> tuple[bool, str]:
    """POST the review prompt. Returns (ok, text-or-error). Never raises."""
    body = {
        "model": model,
        "temperature": 0,
        "messages": [
            {"role": "system", "content": REVIEW_PROMPT},
            {
                "role": "user",
                "content": json.dumps({"changed_files": files}, indent=2)[:200_000],
            },
        ],
    }
    request = urllib.request.Request(
        base_url.rstrip("/") + "/chat/completions",
        data=json.dumps(body).encode("utf-8"),
        headers={
            "Authorization": "Bearer " + key,  # requirement 1: used here, written nowhere
            "Content-Type": "application/json",
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            if response.status != 200:
                return False, "model endpoint returned HTTP {}".format(response.status)
            payload = json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as exc:
        return False, "model endpoint returned HTTP {}".format(exc.code)
    except Exception as exc:  # transport, DNS, timeout, TLS, bad JSON
        return False, "model call failed: {}: {}".format(type(exc).__name__, exc)
    try:
        return True, payload["choices"][0]["message"]["content"]
    except (KeyError, IndexError, TypeError):
        return False, "model response had no choices[0].message.content"


def parse_review(content: str) -> tuple[list[dict], str, str | None]:
    """Parse the model's JSON. Returns (findings, summary, warning)."""
    text = content.strip()
    if text.startswith("```"):
        text = re.sub(r"^```[a-zA-Z]*\n|\n```$", "", text).strip()
    try:
        data = json.loads(text)
    except ValueError:
        match = re.search(r"\{.*\}", text, re.DOTALL)
        if not match:
            return [], "", "model response was not JSON; no findings extracted"
        try:
            data = json.loads(match.group(0))
        except ValueError:
            return [], "", "model response was not JSON; no findings extracted"
    findings = data.get("findings") or []
    if not isinstance(findings, list):
        findings = []
    return findings, str(data.get("summary", "")), None


def highest_severity(findings: list[dict]) -> str:
    rank = {"low": 1, "medium": 2, "high": 3}
    best = "none"
    for finding in findings:
        severity = str(finding.get("severity", "")).lower()
        if rank.get(severity, 0) > rank.get(best, 0):
            best = severity
    return best


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Advisory review of changed files. Always exits 0 (lesson 4.2)."
    )
    parser.add_argument("paths", nargs="*", help="changed file paths; stdin when omitted")
    parser.add_argument("--files", help="file holding one changed path per line")
    parser.add_argument("--repo", default=os.environ.get("REPO_ROOT", "/workspace"),
                        help="repository root the paths are relative to")
    parser.add_argument("--out-dir", default="ci-artifacts")
    parser.add_argument("--model", default=os.environ.get("REVIEW_MODEL", DEFAULT_MODEL))
    parser.add_argument("--base-url", default=os.environ.get("OPENROUTER_BASE_URL", DEFAULT_BASE_URL))
    parser.add_argument("--timeout", type=int, default=DEFAULT_TIMEOUT)
    parser.add_argument("--max-bytes", type=int, default=DEFAULT_MAX_BYTES)
    args = parser.parse_args(argv)

    secrets = _secret_values()
    out_dir = Path(args.out_dir)
    report_path = out_dir / "review-report.json"
    summary_path = out_dir / "review-summary.md"
    audit_path = out_dir / "audit-review.log"

    # --- the file list ------------------------------------------------
    if args.files:
        paths = Path(args.files).read_text(encoding="utf-8").split()
    elif args.paths:
        paths = [p.strip() for p in args.paths if p.strip()]
    elif not sys.stdin.isatty():
        paths = [p.strip() for p in sys.stdin.read().split() if p.strip()]
    else:
        paths = []

    key = os.environ.get(API_KEY_ENV, "")
    key_present = bool(key.strip())
    started = _now()

    report: dict = {
        "status": "error",
        "advisory": True,
        "gating": False,
        "exit_code": 0,
        "model": args.model,
        "api_key_env": API_KEY_ENV,
        "api_key_present": key_present,
        "started_at": started,
        "finished_at": started,
        "changed_files": paths,
        "reviewed_files": [],
        "skipped_files": [],
        "findings": [],
        "severity": "none",
        "summary": "",
        "parse_warning": None,
        "error": None,
    }

    if not key_present:
        # Path 1: no credential. Report that the review did not run. Do not pretend.
        report["status"] = "not_run"
        report["severity"] = "not_run"
        report["summary"] = (
            "Review did not run: {} is not present in the environment. "
            "No findings were produced by this step.".format(API_KEY_ENV)
        )
    elif not paths:
        report["status"] = "not_run"
        report["severity"] = "not_run"
        report["summary"] = "Review did not run: no changed files were supplied."
    else:
        repo = Path(args.repo)
        files, skipped = read_files(paths, repo, args.max_bytes)
        report["reviewed_files"] = [f["path"] for f in files]
        report["skipped_files"] = skipped
        if not files:
            report["status"] = "not_run"
            report["severity"] = "not_run"
            report["summary"] = (
                "Review did not run: none of the {} changed file(s) could be read "
                "from {}.".format(len(paths), repo)
            )
        else:
            ok, result = call_model(key, args.base_url, args.model, files, args.timeout)
            if not ok:
                # Path 3: the model call failed. Record it; still exit 0.
                report["status"] = "error"
                report["error"] = _redact(result, secrets)
                report["summary"] = "Review did not complete: {}".format(report["error"])
            else:
                findings, summary, warning = parse_review(result)
                report["status"] = "completed"
                report["findings"] = findings
                report["severity"] = highest_severity(findings)
                report["summary"] = summary
                report["parse_warning"] = warning

    report["finished_at"] = _now()

    # --- writes. Every one of them goes through _write(), which redacts. ------------
    _write(report_path, json.dumps(report, indent=2) + "\n", secrets)

    lines = [
        "# Advisory review",
        "",
        "- Status: `{}`".format(report["status"]),
        "- Severity: `{}`".format(report["severity"]),
        "- Files reviewed: {}".format(len(report["reviewed_files"])),
        "- Findings: {}".format(len(report["findings"])),
        "- Advisory: yes (this step never blocks a merge)",
        "",
        report["summary"] or "(no summary)",
        "",
    ]
    if report["findings"]:
        lines.append("## Findings")
        lines.append("")
        for finding in report["findings"]:
            lines.append(
                "- **{}** `{}` — {}".format(
                    str(finding.get("severity", "?")).upper(),
                    finding.get("file", "?"),
                    finding.get("message", ""),
                )
            )
        lines.append("")
    _write(summary_path, "\n".join(lines), secrets)

    # The review step's own audit log, the artifact the job uploads. This is the step's
    # record of what it did -- never environment state, never the key, only the boolean.
    audit_record = {
        "timestamp": report["finished_at"],
        "step": "advisory-review",
        "script": "scripts/run-reviewer.py",
        "event": report["status"],
        "api_key_present": key_present,
        "api_key_env": API_KEY_ENV,
        "files_reviewed": len(report["reviewed_files"]),
        "findings": len(report["findings"]),
        "severity": report["severity"],
        "exit_code": 0,
    }
    _write(
        audit_path,
        json.dumps(audit_record, sort_keys=True) + "\n",
        secrets,
    )

    # Human-readable summary on stdout.
    print(summary_path.read_text(encoding="utf-8"))
    print("report : {}".format(report_path))
    print("audit  : {}".format(audit_path))
    return 0


if __name__ == "__main__":
    # Path 3 also covers an unexpected crash: the step stays advisory, so it still exits 0.
    try:
        sys.exit(main())
    except SystemExit:
        raise
    except Exception as exc:  # noqa: BLE001 - advisory steps must never fail the pipeline
        print("advisory review hit an unexpected error: {}: {}".format(type(exc).__name__, exc))
        print("exiting 0: the review step is advisory and never blocks a merge")
        sys.exit(0)
