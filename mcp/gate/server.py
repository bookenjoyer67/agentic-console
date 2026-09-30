#!/usr/bin/env python3
"""Quality-gate MCP server for this repository.

Exposes a fixed, allowlisted execution surface over streamable HTTP (FastMCP): eight named commands.
Seven are check-mode -- ``test``, ``clippy``, ``fmt``, ``policy``, ``conformance``, ``webcheck`` and
``webtest`` -- and ``run_gate`` runs those; one is write-mode, ``fmt-fix``, and ``run_fix`` alone
runs it. A caller names a command, never a command line: every argv comes from an ``agentic.config.json``
entry, ``shell`` is never used, and no caller-supplied string reaches a command line.

Each invocation returns the exit code, the captured stdout and stderr, the wall-clock duration and a
verdict, and appends one journal line naming the ``tool`` that ran it and its ``writes`` mode. A
refused call runs nothing and journals nothing, so the journal holds only executed commands.

The ``clippy`` gate carries the project's cache-hit guard, whose marker line and touched file are
read from ``toolchain.commands.clippy.guard``. cargo's second run over an unchanged tree prints
nothing and exits 0, which is indistinguishable from a clean lint, so the gate touches a file
under test before invoking cargo and then requires the configured marker line in the output.
``passed`` for clippy is therefore exit code 0 *and* a satisfied guard. This image runs cargo with
``CARGO_TERM_COLOR=always``, so status lines arrive wrapped in SGR escapes and the guard matches the
output with the escapes stripped; the returned stdout and stderr are stripped for the same reason.

Run inside the sandbox container:

    python3 mcp/gate/server.py --port 8003 --host 0.0.0.0
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

# --- The portability seam: the gate vocabulary lives in agentic.config.json -------------------
# `gate_vocabulary.py` reads the command names and the argv tuples, the mode of each, the clippy
# cache-hit guard and the container paths the commands run in from the config
# (`toolchain.commands` and `containers`), and falls back to its own embedded defaults, which are
# this repository's values. A run with no config file behaves as this server did before it existed.
_SCRIPTS = next(
    (parent / "scripts" for parent in Path(__file__).resolve().parents
     if (parent / "scripts" / "agentic_config.py").is_file()),
    Path(__file__).resolve().parents[2] / "scripts",
)
if str(_SCRIPTS) not in sys.path:
    sys.path.insert(0, str(_SCRIPTS))
_GATE_DIR = Path(__file__).resolve().parent
if str(_GATE_DIR) not in sys.path:
    sys.path.insert(0, str(_GATE_DIR))
from gate_vocabulary import (COMMANDS, FIX_COMMANDS, GATES, GUARD_MARKER_PATTERN,  # noqa: E402
                             MEMORY_DIR, SUMMARY_PATTERNS, WORKSPACE, AUDIT_PATH, TOOLS_IMAGE, print_config_if_requested)

# A supported entry point: it answers before the MCP and HTTP imports, so a host without fastmcp
# installed can still read what this server consumes. It runs no gate and reads no journal.
if print_config_if_requested(sys.argv[1:]):
    raise SystemExit(0)

from fastmcp import FastMCP  # noqa: E402 - imported after the flag above, which must work bare
from starlette.middleware import Middleware  # noqa: E402
from starlette.middleware.cors import CORSMiddleware  # noqa: E402

# --- Command vocabulary: the whole execution surface of this server ---------------------------
# The argv tuples are the only commands this process can ever run: nothing else is composed,
# interpolated or appended at call time, and the caller contributes no element of any argv. Each
# command's name, argv, description, guard and mode is the config's `toolchain.commands.<name>`
# block, read by `gate_vocabulary.py`, which splits it into `GATES` (check) and `FIX_COMMANDS`.

# cargo honours CARGO_TERM_COLOR=always in this image even when stderr is a pipe, so the status
# lines arrive wrapped in SGR escapes (a ``Checking`` status line arrives as
# "\x1b[1m\x1b[92m    Checking\x1b[0m <crate>"). The guard matches against output with the escapes
# removed; a naive substring search over the raw bytes finds nothing and would report every clippy
# run as a cache hit. The pattern is `gate_vocabulary.py`'s, from the config's marker_regex.
ANSI_ESCAPE = re.compile(r"\x1b\[[0-9;?]*[ -/]*[@-~]")

DEFAULT_TIMEOUT_SECONDS = int(os.getenv("GATE_TIMEOUT_SECONDS", "900"))
MIN_TIMEOUT_SECONDS = 60
MAX_TIMEOUT_SECONDS = 3600
MAX_OUTPUT_CHARS = 200_000

mcp = FastMCP("gate")


# --- Time, paths and output shaping ----------------------------------------------------------
def utc_now() -> str:
    """Return the current UTC instant as an ISO-8601 string."""
    return datetime.now(timezone.utc).isoformat()


def ensure_parent(path: str) -> None:
    """Create the parent directory of a runtime file and fail loudly if it is unwritable."""
    parent = Path(path).expanduser().parent
    parent.mkdir(parents=True, exist_ok=True)
    if not os.access(parent, os.W_OK):
        raise PermissionError(f"{parent} is not writable; the server cannot use {path}")


def strip_ansi(text: str) -> str:
    """Remove SGR/CSI escape sequences, so a status line can be matched as plain text."""
    return ANSI_ESCAPE.sub("", text or "")


def clamp_output(text: str) -> tuple[str, bool]:
    """Cap captured output at ``MAX_OUTPUT_CHARS``, keeping the head, and say whether it was cut."""
    if len(text) <= MAX_OUTPUT_CHARS:
        return text, False
    return text[:MAX_OUTPUT_CHARS] + "\n...[truncated by the gate server]...", True


# --- Validation -----------------------------------------------------------------------------
def validate_gate(gate: str) -> str:
    """Accept one allowlisted gate name and refuse everything else, including command strings."""
    if not isinstance(gate, str):
        raise ValueError("gate must be a string naming an allowlisted gate")
    if gate not in GATES:
        raise ValueError(
            f"refused: '{gate}' is not an allowlisted gate. This server runs only "
            f"{sorted(GATES)} by name; it accepts no command string, no extra arguments and no "
            "shell."
            + (
                f" '{gate}' is a write-mode command, refused here by mode as well as by name: "
                "run_gate runs check-mode gates only, and a write-mode command is reachable "
                "through run_fix alone."
                if gate in FIX_COMMANDS
                else ""
            )
        )
    return gate


def validate_fix(command: str) -> str:
    """Accept one allowlisted write-mode command name and refuse everything else.

    The mirror of ``validate_gate`` against ``FIX_COMMANDS``. A check-mode gate name is refused by
    reason as well as by name, and every other string -- a command line, an argument passthrough, a
    shell-injection attempt -- is refused for the same reason all of them are: it is not a key of
    the table, and the table is the only place an argv can come from.
    """
    if not isinstance(command, str):
        raise ValueError("command must be a string naming an allowlisted write-mode command")
    if command not in FIX_COMMANDS:
        raise ValueError(
            f"refused: '{command}' is not an allowlisted write-mode command. This server runs "
            f"only {sorted(FIX_COMMANDS)} by name; it accepts no command string, no extra "
            "arguments and no shell."
            + (
                f" '{command}' is a check-mode gate, refused here by mode as well as by name: "
                "run_fix runs write-mode commands only, and a check-mode gate is reachable "
                "through run_gate alone."
                if command in GATES
                else ""
            )
        )
    return command


def validate_timeout(timeout_seconds: int | None) -> int:
    """Clamp a caller-supplied timeout into the allowed window, or use the default."""
    if timeout_seconds is None:
        return DEFAULT_TIMEOUT_SECONDS
    if not isinstance(timeout_seconds, int) or isinstance(timeout_seconds, bool):
        raise ValueError("timeout_seconds must be an integer number of seconds")
    return max(MIN_TIMEOUT_SECONDS, min(MAX_TIMEOUT_SECONDS, timeout_seconds))


# --- The audit journal ----------------------------------------------------------------------
def append_audit_record(record: dict[str, Any]) -> None:
    """Append exactly one JSON object plus newline. The file is opened append-only."""
    ensure_parent(AUDIT_PATH)
    line = json.dumps(record, sort_keys=True) + "\n"
    with open(AUDIT_PATH, "a", encoding="utf-8") as handle:
        handle.write(line)
        handle.flush()
        os.fsync(handle.fileno())


def audit_invocation(
    *,
    tool: str,
    gate: str,
    argv: list[str] | None,
    exit_code: int | None,
    duration_seconds: float | None,
    passed: bool | None,
    timed_out: bool,
    guard_applied: bool,
    guard_satisfied: bool | None,
    writes: bool,
    calling_role: str,
    summary: dict[str, Any] | None,
) -> None:
    """Write one journal record for an executed command. A refused call never reaches here.

    ``tool`` names the operation that ran it and ``writes`` is that command's declared mode, so a
    reader of the journal can tell a check from a mutation from the record alone, without consulting
    the config. The record shape is the same for both tools, so the journal stays one line per
    executed invocation whichever tool produced it.

    ``summary`` is the configured output summary of that run, or None for a command that declares
    no summary rule, so the counts a gate reports stay readable from the journal row alone, without
    the payload they were counted in.
    """
    append_audit_record(
        {
            "timestamp": utc_now(),
            "tool": tool,
            "gate": gate,
            "argv": argv,
            "exit_code": exit_code,
            "duration_seconds": duration_seconds,
            "passed": passed,
            "timed_out": timed_out,
            "guard_applied": guard_applied,
            "guard_satisfied": guard_satisfied,
            "summary": summary,
            "writes": writes,
            "calling_role": calling_role or "unknown",
        }
    )


def read_audit_records(limit: int) -> list[dict[str, Any]]:
    """Return the last ``limit`` journal records, newest last, skipping unparseable lines."""
    path = Path(AUDIT_PATH)
    if not path.exists():
        return []
    records: list[dict[str, Any]] = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            records.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    return records[-limit:]


# --- The clippy cache-hit guard -------------------------------------------------------------
def apply_cache_hit_guard(command: str) -> dict[str, Any]:
    """Touch a file under test, so the command that follows cannot be a cached no-op.

    Only the declared ``touch_file`` of the command is touched, it is only given a fresh mtime, and
    a missing file is reported rather than guessed around.
    """
    guard = COMMANDS[command]["guard"]
    if guard is None:
        return {
            "applied": False,
            "satisfied": True,
            "marker": None,
            "touched": None,
            "detail": "no cache-hit guard applies to this gate",
        }

    touch_file = str(Path(WORKSPACE) / guard["touch_file"])
    try:
        os.utime(touch_file, None)
    except OSError as error:
        return {
            "applied": True,
            "satisfied": False,
            "marker": guard["marker"],
            "touched": touch_file,
            "detail": f"cache-hit guard could not touch {touch_file}: {error}",
        }
    return {
        "applied": True,
        "satisfied": False,  # set for real once the marker is seen in the output
        "marker": guard["marker"],
        "touched": touch_file,
        "detail": f"touched {touch_file} before running cargo; awaiting the marker line",
        "guard_reason": guard["reason"],
    }


def guard_satisfied(guard: dict[str, Any], combined_output: str) -> dict[str, Any]:
    """Require the gate's marker line in the output, and record why when it is absent."""
    if not guard["applied"]:
        return guard
    match = GUARD_MARKER_PATTERN.search(strip_ansi(combined_output))
    missing = (
        f"cache-hit guard not satisfied: no '{guard['marker']}' line in the output, so a clean "
        "lint cannot be told apart from a cached no-op"
    )
    guard["satisfied"] = bool(match)
    guard["detail"] = f"found '{guard['marker']}' in the cargo output" if match else missing
    return guard


# --- The configured output summary -----------------------------------------------------------
# The cache-hit guard one level up. The guard asks whether one configured marker line is present;
# this asks how many times each configured pattern matches. Both read their rule from the same
# `toolchain.commands.<name>` block, both report `applied` False with an explaining `detail` for a
# command that declares no rule, and neither one contributes to `passed` or to `verdict`.
#
# The rule exists because a gate's result can be unreadable from its own response. `fmt` exits 1
# and prints a six-figure diff; how many hunks and how many files that diff covers are facts only
# a reader who counts the payload can have, and the payload is clamped at MAX_OUTPUT_CHARS before
# it reaches the caller. Counting here, on the raw captured output and above the clamp, puts those
# numbers in the response and in the journal row, so a role reading either has them without
# re-deriving them and without an operator supplying them by hand.
#
# ANSI first. cargo runs with CARGO_TERM_COLOR=always in this image, so a `^\+` pattern matched
# against the raw bytes counts zero; the text is stripped before it is counted when the rule asks.
def compute_output_summary(command: str, stdout: str, stderr: str) -> dict[str, Any]:
    """Count what the command's configured summary rule asks for, in that command's own output.

    Returns ``applied`` False and no counts for a command declaring no rule, which is the ordinary
    case. Reads the rule and the pre-compiled patterns from ``gate_vocabulary``, never from the
    caller, so no caller-supplied string ever becomes a pattern.
    """
    rule = COMMANDS[command]["summary"]
    if rule is None:
        return {
            "applied": False,
            "counts": None,
            "reason": None,
            "detail": "no output summary rule applies to this gate",
        }

    captured = {"stdout": stdout or "", "stderr": stderr or ""}
    streams = tuple(rule["streams"])
    text = "\n".join(captured[name] for name in streams)
    if rule["strip_ansi"]:
        text = strip_ansi(text)
    lines = text.splitlines()

    counts: dict[str, int] = {}
    for label, count_rule in rule["counts"].items():
        pattern = SUMMARY_PATTERNS[command][label]
        if count_rule["mode"] == "count_unique_groups":
            group = count_rule["group"]
            counts[label] = len(
                {
                    match.group(group)
                    for match in (pattern.search(line) for line in lines)
                    if match is not None
                }
            )
        else:
            counts[label] = sum(1 for line in lines if pattern.search(line))

    clamped = any(len(strip_ansi(captured[name])) > MAX_OUTPUT_CHARS for name in streams)
    tally = ", ".join(f"{label}={value}" for label, value in counts.items())
    return {
        "applied": True,
        "counts": counts,
        "reason": rule["reason"],
        "detail": (
            f"counted {tally} over {len(lines)} lines of {' and '.join(streams)}, "
            "on the captured output before the clamp"
        ),
        "streams": list(streams),
        "input_chars": len(text),
        "input_truncated_by_the_clamp": clamped,
    }


# --- Execution ------------------------------------------------------------------------------
def resolve_cargo() -> str:
    """Return the absolute path of cargo, or fail loudly before any gate pretends to run.

    A pre-flight check only: the executed argv is the configured argv verbatim (``cargo`` resolving
    through the inherited PATH), so the recorded argv can be compared to the allowlist directly.
    """
    found = shutil.which("cargo")
    if found is None:
        raise RuntimeError(
            "cargo is not on PATH in this environment, so no gate can run; use "
            f"{TOOLS_IMAGE}"
        )
    return found


def execute_gate(
    command: str, table: dict[str, dict[str, Any]], timeout_seconds: int
) -> dict[str, Any]:
    """Run one allowlisted command and return its exit code, output, duration and verdict.

    ``table`` is the caller's own vocabulary -- ``GATES`` for ``run_gate``, ``FIX_COMMANDS`` for
    ``run_fix`` -- and the argv is read from it and only from it, so neither tool can reach a
    command the other owns even by name. ``shell`` stays False, ``cwd`` is the workspace the server
    was started with, and nothing caller-supplied is appended to the argv. A timeout kills the
    process and is reported rather than raised.
    """
    resolve_cargo()
    definition = table[command]
    argv = list(definition["argv"])
    guard = apply_cache_hit_guard(command)

    started = time.monotonic()
    timed_out = False
    try:
        completed = subprocess.run(  # noqa: S603 - a fixed argv, never a caller-supplied string
            argv,
            cwd=WORKSPACE,
            capture_output=True,
            text=True,
            timeout=timeout_seconds,
            check=False,
        )
        exit_code: int | None = completed.returncode
        raw_stdout = completed.stdout or ""
        raw_stderr = completed.stderr or ""
    except subprocess.TimeoutExpired as expired:
        timed_out = True
        exit_code = -1
        raw_stdout = _decode(expired.stdout)
        raw_stderr = _decode(expired.stderr)
    duration_seconds = round(time.monotonic() - started, 3)

    guard = guard_satisfied(guard, raw_stdout + "\n" + raw_stderr)
    # Counted on the raw capture, above the clamp below, so the counts describe the whole run and
    # not the head of it. The summary never enters `passed` or `verdict`, which follow unchanged.
    summary = compute_output_summary(command, raw_stdout, raw_stderr)
    passed = exit_code == 0 and bool(guard["satisfied"])
    if timed_out:
        verdict = f"fail: killed after {timeout_seconds}s without finishing"
    elif exit_code != 0:
        verdict = f"fail: exit code {exit_code}"
    elif not guard["satisfied"]:
        verdict = f"fail: {guard['detail']}"
    else:
        verdict = "pass"

    stdout, stdout_truncated = clamp_output(strip_ansi(raw_stdout))
    stderr, stderr_truncated = clamp_output(strip_ansi(raw_stderr))
    return {
        "gate": command,
        "argv": argv,
        "exit_code": exit_code,
        "passed": passed,
        "verdict": verdict,
        "timed_out": timed_out,
        "timeout_seconds": timeout_seconds,
        "duration_seconds": duration_seconds,
        "writes": bool(definition["writes"]),
        "mode": "write" if definition["writes"] else "check",
        "guard": guard,
        "summary": summary,
        "stdout": stdout,
        "stderr": stderr,
        "stdout_truncated": stdout_truncated,
        "stderr_truncated": stderr_truncated,
        "output_ansi_stripped": True,
    }


def _decode(blob: str | bytes | None) -> str:
    """Turn whatever the timeout exception carried into text."""
    if blob is None:
        return ""
    if isinstance(blob, bytes):
        return blob.decode("utf-8", errors="replace")
    return blob


# --- Operations -----------------------------------------------------------------------------
@mcp.tool
def list_gates() -> list[dict]:
    """List every allowlisted command with the exact argv and mode of each. Runs nothing.

    Both modes are published from one table, so a caller can see which tool runs a command before
    naming it: ``mode`` is ``check`` for a command ``run_gate`` runs and ``write`` for one
    ``run_fix`` runs, and ``writes`` is the same fact as the boolean the config declares.
    """
    return [
        {
            "gate": name,
            "argv": list(definition["argv"]),
            "description": definition["description"],
            "writes": definition["writes"],
            "mode": "write" if definition["writes"] else "check",
            "cache_hit_guard": (
                {
                    "marker": definition["guard"]["marker"],
                    "touch_file": definition["guard"]["touch_file"],
                }
                if definition["guard"]
                else None
            ),
        }
        for name, definition in COMMANDS.items()
    ]


@mcp.tool
def run_gate(gate: str, calling_role: str = "unknown", timeout_seconds: int | None = None) -> dict:
    """Run one allowlisted check-mode gate by name and return its exit code, output and verdict.

    The name is resolved against the check-mode table alone, so a write-mode command is refused
    however it is spelled. No parameter carries an argv element, a path, a flag or a shell.
    """
    validate_gate(gate)
    effective_timeout = validate_timeout(timeout_seconds)
    result = execute_gate(gate, GATES, effective_timeout)
    audit_invocation(
        tool="run_gate",
        gate=gate,
        argv=result["argv"],
        exit_code=result["exit_code"],
        duration_seconds=result["duration_seconds"],
        passed=result["passed"],
        timed_out=result["timed_out"],
        guard_applied=bool(result["guard"]["applied"]),
        guard_satisfied=result["guard"]["satisfied"],
        writes=False,
        calling_role=calling_role,
        summary=result["summary"],
    )
    return result


@mcp.tool
def run_fix(command: str, calling_role: str = "unknown", timeout_seconds: int | None = None) -> dict:
    """Run one allowlisted write-mode command by name. It rewrites files in the workspace.

    The write-mode counterpart of ``run_gate`` and its exact mirror: the name is resolved against
    the write-mode table alone, so a check-mode gate is refused however it is spelled, and the argv
    comes from that table and nowhere else. No parameter carries an argv element, a path, a flag, a
    cwd or a shell, so this tool widens what the server can run by exactly one configured command
    and by nothing else. The journal records it as a mutation.
    """
    validate_fix(command)
    effective_timeout = validate_timeout(timeout_seconds)
    result = execute_gate(command, FIX_COMMANDS, effective_timeout)
    audit_invocation(
        tool="run_fix",
        gate=command,
        argv=result["argv"],
        exit_code=result["exit_code"],
        duration_seconds=result["duration_seconds"],
        passed=result["passed"],
        timed_out=result["timed_out"],
        guard_applied=bool(result["guard"]["applied"]),
        guard_satisfied=result["guard"]["satisfied"],
        writes=True,
        calling_role=calling_role,
        summary=result["summary"],
    )
    return result


@mcp.tool
def read_audit_log(limit: int = 20) -> list[dict]:
    """Read the last ``limit`` audit records, newest last. Read-only; the journal is append-only."""
    if not isinstance(limit, int) or isinstance(limit, bool) or limit < 1 or limit > 200:
        raise ValueError("limit must be an integer between 1 and 200")
    return read_audit_records(limit)


def build_app():
    """Build the Starlette application that serves the MCP endpoint at ``/mcp``."""
    return mcp.http_app(
        stateless_http=True,
        middleware=[
            Middleware(
                CORSMiddleware,
                allow_origins=["*"],
                allow_methods=["*"],
                allow_headers=["*"],
            )
        ],
    )


def main() -> None:
    """Parse the CLI flags and serve the gate server on the requested port."""
    global WORKSPACE, AUDIT_PATH

    parser = argparse.ArgumentParser(description="Quality-gate MCP server")
    parser.add_argument("--port", type=int, default=8003, help="HTTP port (default 8003)")
    parser.add_argument("--host", default="0.0.0.0", help="bind address (default 0.0.0.0)")
    parser.add_argument(
        "--workspace", default=WORKSPACE, help=f"repository root the gates run in (default {WORKSPACE})"
    )
    parser.add_argument(
        "--audit-path", default=AUDIT_PATH, help=f"audit journal (default {AUDIT_PATH})"
    )
    args = parser.parse_args()

    WORKSPACE = args.workspace
    AUDIT_PATH = args.audit_path
    ensure_parent(AUDIT_PATH)
    print(f"gate workspace: {WORKSPACE}", flush=True)
    print(f"gate audit log: {AUDIT_PATH}", flush=True)
    print(f"gates: {sorted(GATES)} (timeout {DEFAULT_TIMEOUT_SECONDS}s)", flush=True)
    print(f"fix commands: {sorted(FIX_COMMANDS)} (run_fix only)", flush=True)

    import uvicorn

    uvicorn.run(build_app(), host=args.host, port=args.port)


if __name__ == "__main__":
    main()
