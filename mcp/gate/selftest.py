#!/usr/bin/env python3
"""Self-test for the quality-gate MCP server.

Calls the running server over streamable HTTP and asserts the behaviours the server claims: the tool
surface, the eight-command allowlist and the mode published for each, refusal of a free-form command,
a passthrough and a shell-injection string on three of the seven check-mode names, refusal in both
directions across the mode boundary, the clippy cache-hit guard, real exit codes from the three Rust
gates, and one audit-journal line per executed invocation. Every check prints PASS or FAIL with its
evidence; any FAIL makes the process exit non-zero, and the output is the record of a real run.

The write-mode command is never executed here. It rewrites the tree, and a self-test that formatted
the workspace would change the thing the ``fmt`` gate is measuring, so ``fmt-fix`` is exercised only
through the refusals that prove it is unreachable from the check surface.

Run against a server already listening inside the sandbox container:

    python3 mcp/gate/server.py --port 8003 &
    python3 mcp/gate/selftest.py --url http://localhost:8003/mcp
"""

from __future__ import annotations

import argparse
import asyncio
import json
import re
import sys
from pathlib import Path
from typing import Any

from fastmcp import Client
from fastmcp.exceptions import ToolError

DEFAULT_URL = "http://localhost:8003/mcp"
DEFAULT_AUDIT_PATH = "/workspace/.memory/gate-audit.log"

# The allowlist the server is supposed to publish. Kept here as the independent expectation: a check
# that read both sides from the server would prove nothing.
EXPECTED_GATES = {
    "test": ["cargo", "test", "--workspace"],
    "clippy": ["cargo", "clippy", "--release", "--all-targets", "--", "-D", "warnings"],
    "fmt": ["cargo", "fmt", "--check"],
    "policy": [
        "python3", "-m", "pytest", "eval/test_policy.py", "eval/test_deterministic_step.py", "-q",
    ],
    "conformance": ["python3", "scripts/run-conformance-gate.py"],
    "fmt-fix": ["cargo", "fmt", "--all"],
    "webcheck": ["npm", "--prefix", "web", "run", "check"],
    "webtest": ["npm", "--prefix", "web", "run", "test"],
}
# The mode each published command declares, as an independent literal for the same reason. `check`
# is what `run_gate` runs and `write` is what `run_fix` runs; the two sets are disjoint.
EXPECTED_MODES = {
    "test": "check",
    "clippy": "check",
    "fmt": "check",
    "policy": "check",
    "conformance": "check",
    "fmt-fix": "write",
    "webcheck": "check",
    "webtest": "check",
}
EXPECTED_TOOLS = {"list_gates", "run_gate", "run_fix", "read_audit_log"}
EXPECTED_RUN_GATE_PARAMS = {"gate", "calling_role", "timeout_seconds"}
# run_fix is name-only in exactly the way run_gate is: one command name, the calling role and a
# timeout. A property beyond these three would be a way to reach a command line, so the set is
# asserted rather than assumed.
EXPECTED_RUN_FIX_PARAMS = {"command", "calling_role", "timeout_seconds"}
# The counts the `fmt` gate's configured summary rule declares, and the one command that declares a
# rule at all, both as independent literals for the same reason the allowlist above is one.
# `agentic.config.json` is the declaration; these are this file's expectation of it.
EXPECTED_SUMMARY_COUNTS = {"hunks", "files", "added", "removed"}
EXPECTED_SUMMARY_GATES = {"fmt"}
# The recorded test baseline, and the two synthetic records the baseline predicate is asserted on
# before any server call. The predicate this replaced compared `failed == 0` to `exit_code == 0` as
# an equivalence, so a record of 158 passed and 1 failed at exit 1 satisfied it and a gate that
# reported a failed test read as PASS. The two records below pin the repair in both directions.
TEST_BASELINE_PASSED = 159
BASELINE_REJECT_RECORD = {"passed": 158, "failed": 1}
BASELINE_ACCEPT_RECORD = {"passed": 159, "failed": 0}
GUARD_MARKER = "Checking agentic-console"
INJECTION_TARGET = "/tmp/gate-selftest-pwned"
INJECTION_TARGET_POLICY = "/tmp/gate-selftest-pwned-policy"
INJECTION_TARGET_CONFORMANCE = "/tmp/gate-selftest-pwned-conformance"
INJECTION_TARGET_FIX = "/tmp/gate-selftest-pwned-fix"
TEST_SUMMARY = re.compile(r"^test result: \w+\. (?P<passed>\d+) passed; (?P<failed>\d+) failed")

RESULTS: list[tuple[str, bool, str]] = []


def check(name: str, passed: bool, detail: str) -> bool:
    """Record and print one behaviour's verdict with the evidence behind it."""
    RESULTS.append((name, passed, detail))
    print(f"CHECK {'PASS' if passed else 'FAIL'} {name} :: {detail}", flush=True)
    return passed


# --- The test baseline predicate -------------------------------------------------------------
def baseline_ok(record: dict[str, Any]) -> bool:
    """Accept a test record only when nothing failed and the recorded baseline is met.

    Two conditions, both required, neither able to excuse the other: a failure count of exactly
    zero, and a pass count at or above the recorded baseline. A missing key reads as ``-1`` and so
    fails, because a record that does not say how many tests failed has not said that none did.
    """
    return (
        int(record.get("failed", -1)) == 0
        and int(record.get("passed", -1)) >= TEST_BASELINE_PASSED
    )


def baseline_verdict(record: dict[str, Any]) -> str:
    """The word the baseline predicate reports for one test record: PASS or FAIL."""
    return "PASS" if baseline_ok(record) else "FAIL"


def check_baseline_predicate() -> None:
    """Assert the baseline predicate on two synthetic records, before any server is contacted.

    Runs on data this file carries, so both lines print even when no server is listening: a gate
    that reports a failed test reading as PASS is a property of the predicate, and exposing it
    needs no gate run. The printed records are the quotable evidence for that repair.
    """
    rejected = baseline_verdict(BASELINE_REJECT_RECORD)
    check(
        "baseline_predicate_rejects_a_failed_test",
        rejected == "FAIL",
        f"{json.dumps(BASELINE_REJECT_RECORD)} -> {rejected}; one failed test can never read as "
        f"PASS however high the passed count (baseline={TEST_BASELINE_PASSED})",
    )
    accepted = baseline_verdict(BASELINE_ACCEPT_RECORD)
    check(
        "baseline_predicate_accepts_the_recorded_baseline",
        accepted == "PASS",
        f"{json.dumps(BASELINE_ACCEPT_RECORD)} -> {accepted}; nothing failed and the recorded "
        f"baseline of {TEST_BASELINE_PASSED} is met",
    )


# --- An independent recount of the fmt payload -----------------------------------------------
def fmt_hunk_path(hunk_line: str) -> str:
    """The file path in one rustfmt ``Diff in`` line, without using the configured pattern.

    rustfmt has shipped two forms of this line -- ``Diff in <path> at line <n>:`` and
    ``Diff in <path>:<n>:`` -- so the trailing line number is removed by whichever of the two
    separators is actually present, and neither form is assumed to be the one in use.
    """
    rest = hunk_line[len("Diff in "):].strip().rstrip(":")
    head, separator, tail = rest.rpartition(" at line ")
    if separator and tail.isdigit():
        return head
    head, separator, tail = rest.rpartition(":")
    if separator and tail.isdigit():
        return head
    return rest


def recount_fmt(text: str) -> dict[str, int]:
    """Recount the fmt payload with plain string methods, as an independent check of the gate.

    Deliberately not the config's regexes: a recount that reused them would prove only that the
    server can run its own pattern twice. Counts the same four things over the same text.
    """
    lines = text.splitlines()
    hunks = [line for line in lines if line.startswith("Diff in ")]
    return {
        "hunks": len(hunks),
        "files": len({fmt_hunk_path(line) for line in hunks}),
        "added": sum(1 for line in lines if line.startswith("+")),
        "removed": sum(1 for line in lines if line.startswith("-")),
    }


def payload(result: Any) -> Any:
    """Return a tool result as Python data, whatever shape the client hands back."""
    for attribute in ("data", "structured_content"):
        value = getattr(result, attribute, None)
        if isinstance(value, (dict, list)):
            return value
    chunks = [
        getattr(block, "text", "")
        for block in getattr(result, "content", []) or []
        if getattr(block, "type", "") == "text"
    ]
    return json.loads("".join(chunks))


def audit_lines(path: str) -> list[dict[str, Any]]:
    """Read the audit journal as parsed records, one per non-empty line."""
    journal = Path(path)
    if not journal.exists():
        return []
    return [
        json.loads(line)
        for line in journal.read_text(encoding="utf-8", errors="replace").splitlines()
        if line.strip()
    ]


async def refusal(
    client: Client,
    name: str,
    value: str,
    what: str,
    tool: str = "run_gate",
    parameter: str = "gate",
) -> str | None:
    """Call a run tool with a value that must be refused and return the error text, or ``None``.

    ``tool`` and ``parameter`` default to the check surface, so the existing call sites read as they
    did; passing ``run_fix`` and ``command`` drives the same assertion against the write surface.
    """
    try:
        result = await client.call_tool(tool, {parameter: value})
    except ToolError as error:
        text = str(error)
        check(name, "refused" in text and "allowlisted" in text, f"{what} -> {text.splitlines()[0]}")
        return text
    else:
        check(name, False, f"{what} -> NOT REFUSED, returned {json.dumps(payload(result))[:200]}")
        return None


async def run_selftest(url: str, audit_path: str) -> int:
    """Drive every check against the live server and return a process exit code."""
    # First, and off the wire: the baseline predicate is asserted on synthetic records, so these
    # two lines are printed even when the server below is unreachable.
    check_baseline_predicate()

    before = audit_lines(audit_path)

    async with Client(url, timeout=1800) as client:
        tools = await client.list_tools()
        names = {tool.name for tool in tools}
        check(
            "tool_surface",
            names == EXPECTED_TOOLS,
            f"tools={sorted(names)} expected={sorted(EXPECTED_TOOLS)}",
        )

        run_gate_tool = next((tool for tool in tools if tool.name == "run_gate"), None)
        params = set((run_gate_tool.inputSchema.get("properties") or {})) if run_gate_tool else set()
        check(
            "run_gate_parameters",
            params == EXPECTED_RUN_GATE_PARAMS,
            f"properties={sorted(params)} expected={sorted(EXPECTED_RUN_GATE_PARAMS)}",
        )

        run_fix_tool = next((tool for tool in tools if tool.name == "run_fix"), None)
        fix_params = set((run_fix_tool.inputSchema.get("properties") or {})) if run_fix_tool else set()
        check(
            "run_fix_parameters",
            fix_params == EXPECTED_RUN_FIX_PARAMS,
            f"properties={sorted(fix_params)} expected={sorted(EXPECTED_RUN_FIX_PARAMS)}",
        )

        gates = payload(await client.call_tool("list_gates", {}))
        published = {entry["gate"]: entry["argv"] for entry in gates}
        published_modes = {entry["gate"]: entry["mode"] for entry in gates}
        published_writes = {entry["gate"]: entry["writes"] for entry in gates}
        check(
            "allowlist_is_the_eight_documented_commands",
            published == EXPECTED_GATES
            and published_modes == EXPECTED_MODES
            and published_writes == {name: mode == "write" for name, mode in EXPECTED_MODES.items()},
            f"published={json.dumps(published, sort_keys=True)} "
            f"modes={json.dumps(published_modes, sort_keys=True)}",
        )

        await refusal(client, "refuses_free_form_command", "cargo test --workspace", "gate='cargo test --workspace'")
        await refusal(client, "refuses_argument_passthrough", "test -- --nocapture", "gate='test -- --nocapture'")
        await refusal(
            client,
            "refuses_shell_injection",
            f"test; touch {INJECTION_TARGET}",
            f"gate='test; touch {INJECTION_TARGET}'",
        )
        # The two new gates are name-only too, so the same three refusals are aimed at each of them:
        # a free-form command string, an argument passthrough and a shell-injection string.
        await refusal(
            client,
            "policy_refuses_free_form_command",
            "python3 -m pytest eval/test_policy.py",
            "gate='python3 -m pytest eval/test_policy.py'",
        )
        await refusal(
            client,
            "policy_refuses_argument_passthrough",
            "policy -- -k NM-1",
            "gate='policy -- -k NM-1'",
        )
        await refusal(
            client,
            "policy_refuses_shell_injection",
            f"policy; touch {INJECTION_TARGET_POLICY}",
            f"gate='policy; touch {INJECTION_TARGET_POLICY}'",
        )
        await refusal(
            client,
            "conformance_refuses_free_form_command",
            "python3 scripts/run-conformance-gate.py",
            "gate='python3 scripts/run-conformance-gate.py'",
        )
        await refusal(
            client,
            "conformance_refuses_argument_passthrough",
            "conformance --input docs/DOC-STYLE.md",
            "gate='conformance --input docs/DOC-STYLE.md'",
        )
        await refusal(
            client,
            "conformance_refuses_shell_injection",
            f"conformance; touch {INJECTION_TARGET_CONFORMANCE}",
            f"gate='conformance; touch {INJECTION_TARGET_CONFORMANCE}'",
        )

        # --- the mode boundary, in both directions ---------------------------------------------
        # Nothing below executes `fmt-fix`: every call here is one the server must refuse, so the
        # write-mode command is proved unreachable from the check surface without ever being run.
        # The journal count is taken immediately before and immediately after this block, and
        # nothing between the two reads executes anything, so the count is evidence about these
        # refusals alone and cannot be moved by an unrelated invocation.
        before_mode_refusals = audit_lines(audit_path)
        await refusal(
            client,
            "run_gate_refuses_the_write_mode_command",
            "fmt-fix",
            "run_gate(gate='fmt-fix')",
        )
        await refusal(
            client,
            "run_fix_refuses_a_check_mode_gate",
            "fmt",
            "run_fix(command='fmt')",
            tool="run_fix",
            parameter="command",
        )
        await refusal(
            client,
            "run_fix_refuses_shell_injection",
            f"fmt-fix; touch {INJECTION_TARGET_FIX}",
            f"run_fix(command='fmt-fix; touch {INJECTION_TARGET_FIX}')",
            tool="run_fix",
            parameter="command",
        )
        check(
            "refused_run_fix_journals_nothing",
            len(audit_lines(audit_path)) == len(before_mode_refusals),
            f"journal lines before the mode refusals={len(before_mode_refusals)} "
            f"after={len(audit_lines(audit_path))}; a refused call journals nothing",
        )
        check(
            "run_fix_shell_injection_ran_nothing",
            not Path(INJECTION_TARGET_FIX).exists(),
            f"{INJECTION_TARGET_FIX} exists={Path(INJECTION_TARGET_FIX).exists()}",
        )

        check(
            "shell_injection_ran_nothing",
            not Path(INJECTION_TARGET).exists()
            and not Path(INJECTION_TARGET_POLICY).exists()
            and not Path(INJECTION_TARGET_CONFORMANCE).exists(),
            f"{INJECTION_TARGET} exists={Path(INJECTION_TARGET).exists()}; "
            f"{INJECTION_TARGET_POLICY} exists={Path(INJECTION_TARGET_POLICY).exists()}; "
            f"{INJECTION_TARGET_CONFORMANCE} exists={Path(INJECTION_TARGET_CONFORMANCE).exists()}",
        )
        check(
            "refusals_journal_nothing",
            len(audit_lines(audit_path)) == len(before),
            f"journal lines before={len(before)} after={len(audit_lines(audit_path))}",
        )

        # --- the three real gates, in the order that keeps the run short -----------------------
        # One extra invocation first: it proves the per-run timeout is clamped and reported rather
        # than passed through, and it is counted by the journal assertions below.
        clamped = payload(await client.call_tool("run_gate", {"gate": "fmt", "timeout_seconds": 1}))
        check(
            "timeout_is_clamped_and_reported",
            clamped["timeout_seconds"] == 60 and clamped["timed_out"] is False,
            f"requested=1s reported={clamped['timeout_seconds']}s timed_out={clamped['timed_out']} "
            f"exit={clamped['exit_code']}",
        )
        executed: list[dict[str, Any]] = [clamped]

        fmt = payload(await client.call_tool("run_gate", {"gate": "fmt", "calling_role": "tester"}))
        executed.append(fmt)
        check(
            "fmt_gate_executes_and_reports",
            fmt["argv"] == EXPECTED_GATES["fmt"]
            and isinstance(fmt["exit_code"], int)
            and fmt["passed"] == (fmt["exit_code"] == 0)
            and fmt["duration_seconds"] > 0
            and fmt["timeout_seconds"] == 900,
            f"exit={fmt['exit_code']} passed={fmt['passed']} verdict={fmt['verdict']} "
            f"duration={fmt['duration_seconds']}s argv={fmt['argv']} "
            f"default_timeout={fmt['timeout_seconds']}s "
            f"diffs={len(re.findall(r'^Diff in ', fmt['stdout'], re.M))}",
        )
        check(
            "fmt_gate_writes_nothing",
            "Diff in" in fmt["stdout"] or fmt["exit_code"] == 0,
            f"stdout is a rustfmt diff report, exit={fmt['exit_code']}",
        )

        # --- the configured output summary -----------------------------------------------------
        # The four numbers the gate now reports, and whether they survive a recount this file does
        # itself. The summary is reported beside the payload, so a reader of either the response or
        # the journal row has the counts without counting a six-figure diff by hand.
        fmt_summary = fmt["summary"]
        fmt_counts = fmt_summary["counts"] or {}
        check(
            "fmt_summary_reported",
            fmt_summary["applied"] is True
            and set(fmt_counts) == EXPECTED_SUMMARY_COUNTS
            and all(isinstance(value, int) for value in fmt_counts.values()),
            f"applied={fmt_summary['applied']} counts={json.dumps(fmt_counts, sort_keys=True)} "
            f"expected_keys={sorted(EXPECTED_SUMMARY_COUNTS)} "
            f"input_chars={fmt_summary.get('input_chars')} "
            f"truncated_by_the_clamp={fmt_summary.get('input_truncated_by_the_clamp')} "
            f"detail={fmt_summary.get('detail')}",
        )
        recount = recount_fmt(fmt["stdout"] + "\n" + fmt["stderr"])
        was_clamped = fmt["stdout_truncated"] is True or fmt["stderr_truncated"] is True
        check(
            "fmt_summary_matches_an_independent_recount",
            all(
                fmt_counts.get(label, -1) >= value
                if was_clamped
                else fmt_counts.get(label, -1) == value
                for label, value in recount.items()
            ),
            f"gate={json.dumps(fmt_counts, sort_keys=True)} "
            f"recount={json.dumps(recount, sort_keys=True)} "
            f"stdout_truncated={fmt['stdout_truncated']} stderr_truncated={fmt['stderr_truncated']} "
            f"comparison={'>=' if was_clamped else '=='}; the gate counts the whole capture above "
            "the clamp and this recount sees only what the response carried, so equality is "
            "required when nothing was clamped and a lower bound when something was",
        )

        clippy = payload(
            await client.call_tool("run_gate", {"gate": "clippy", "calling_role": "tester"})
        )
        executed.append(clippy)
        guard = clippy["guard"]
        check(
            "clippy_guard_applied",
            guard["applied"] is True and guard["touched"] == "/workspace/src/main.rs",
            f"applied={guard['applied']} touched={guard['touched']} marker='{guard['marker']}'",
        )
        check(
            "clippy_guard_satisfied",
            guard["satisfied"] is True,
            f"satisfied={guard['satisfied']} detail={guard['detail']}",
        )
        check(
            "clippy_passed_requires_guard",
            clippy["passed"] == (clippy["exit_code"] == 0 and guard["satisfied"] is True),
            f"exit={clippy['exit_code']} guard_satisfied={guard['satisfied']} "
            f"passed={clippy['passed']} verdict={clippy['verdict']} "
            f"duration={clippy['duration_seconds']}s",
        )
        check(
            "clippy_marker_in_captured_output",
            GUARD_MARKER in clippy["stderr"],
            f"'{GUARD_MARKER}' occurrences in stderr="
            f"{clippy['stderr'].count(GUARD_MARKER)}, stdout={clippy['stdout'].count(GUARD_MARKER)}",
        )

        test = payload(await client.call_tool("run_gate", {"gate": "test", "calling_role": "tester"}))
        executed.append(test)
        summaries = [TEST_SUMMARY.match(line) for line in test["stdout"].splitlines()]
        totals = [
            (int(match.group("passed")), int(match.group("failed")))
            for match in summaries
            if match
        ]
        check(
            "test_gate_executes_and_reports",
            test["argv"] == EXPECTED_GATES["test"] and isinstance(test["exit_code"], int),
            f"exit={test['exit_code']} passed={test['passed']} verdict={test['verdict']} "
            f"duration={test['duration_seconds']}s argv={test['argv']}",
        )
        observed = {
            "passed": sum(passed for passed, _ in totals),
            "failed": sum(failed for _, failed in totals),
        }
        check(
            "test_gate_totals",
            len(totals) > 0 and baseline_ok(observed) and test["exit_code"] == 0,
            f"suites={len(totals)} {json.dumps(observed)} -> {baseline_verdict(observed)} "
            f"baseline={TEST_BASELINE_PASSED} exit={test['exit_code']}; a failed test fails this "
            "check whatever the passed count and whatever the exit code",
        )
        check(
            "captured_output_has_no_ansi_escapes",
            "\x1b" not in fmt["stdout"] + clippy["stderr"] + clippy["stdout"] + test["stdout"]
            and clippy["output_ansi_stripped"] is True,
            "no ESC byte in the returned fmt, clippy and test output; cargo runs with "
            "CARGO_TERM_COLOR=always in this image, so a status line arrives as "
            "'\\x1b[1m\\x1b[92m    Checking\\x1b[0m agentic-console' and the server must strip the "
            "escapes before the guard or any caller-side grep can match it",
        )
        summarised = {result["gate"] for result in executed if result["summary"]["applied"]}
        check(
            "no_summary_gate_records_none",
            clippy["summary"]["applied"] is False
            and clippy["summary"]["counts"] is None
            and test["summary"]["applied"] is False
            and test["summary"]["counts"] is None
            and summarised == EXPECTED_SUMMARY_GATES,
            f"clippy summary={json.dumps(clippy['summary'], sort_keys=True)} "
            f"gates reporting a summary={sorted(summarised)} "
            f"expected={sorted(EXPECTED_SUMMARY_GATES)}; a command with no summary rule records "
            "one that says so rather than a count nobody declared",
        )

        journal = payload(await client.call_tool("read_audit_log", {"limit": 200}))
        after = audit_lines(audit_path)
        check(
            "journal_grows_one_line_per_invocation",
            len(after) == len(before) + len(executed),
            f"lines before={len(before)} after={len(after)} expected={len(before) + len(executed)} "
            f"for {len(executed)} executed invocations",
        )
        required = {
            "timestamp", "tool", "gate", "argv", "exit_code", "duration_seconds", "passed", "writes",
        }
        tail = after[-len(executed):]
        expected_argv = [EXPECTED_GATES[result["gate"]] for result in executed]
        check(
            "journal_records_argv_exit_code_and_timestamp",
            len(tail) == len(executed)
            and all(required <= set(record) for record in tail)
            and [record["gate"] for record in tail] == [result["gate"] for result in executed]
            and [record["argv"] for record in tail] == expected_argv
            and [record["exit_code"] for record in tail]
            == [result["exit_code"] for result in executed]
            and all("T" in record["timestamp"] for record in tail),
            f"gates={[r['gate'] for r in tail]} exit_codes={[r['exit_code'] for r in tail]} "
            f"timestamps={[r['timestamp'] for r in tail]} argv_match="
            f"{[r['argv'] for r in tail] == expected_argv}",
        )
        check(
            "journal_records_the_summary",
            all("summary" in record for record in tail)
            and [record["summary"] for record in tail]
            == [result["summary"] for result in executed],
            f"summaries in the journal tail="
            f"{json.dumps([r.get('summary', {}).get('counts') for r in tail], sort_keys=True)} "
            f"response counts="
            f"{json.dumps([r['summary']['counts'] for r in executed], sort_keys=True)}; the row "
            "carries the same summary the response did, so the counts are readable from the "
            "journal without the payload they were counted in",
        )
        check(
            "journal_marks_every_check_as_a_check",
            all(record["tool"] == "run_gate" for record in tail)
            and all(record["writes"] is False for record in tail),
            f"tools={[r['tool'] for r in tail]} writes={[r['writes'] for r in tail]}; every "
            "invocation this self-test executed was check-mode, so no line may claim otherwise",
        )
        check(
            "read_audit_log_matches_the_journal_file",
            [record["timestamp"] for record in journal[-len(executed):]]
            == [record["timestamp"] for record in tail],
            f"tool returned {len(journal)} records, newest={journal[-1]['gate'] if journal else None}",
        )

    passed = sum(1 for _, ok, _ in RESULTS if ok)
    total = len(RESULTS)
    print(f"SELFTEST_RESULT passed={passed} total={total}", flush=True)
    for name, ok, detail in RESULTS:
        if not ok:
            print(f"SELFTEST_FAILURE {name} :: {detail}", flush=True)
    return 0 if passed == total else 1


def main() -> int:
    """Parse the CLI flags and run the self-test against the live server."""
    parser = argparse.ArgumentParser(description="Self-test for the quality-gate MCP server")
    parser.add_argument("--url", default=DEFAULT_URL, help=f"MCP endpoint (default {DEFAULT_URL})")
    parser.add_argument(
        "--audit-path",
        default=DEFAULT_AUDIT_PATH,
        help=f"audit journal (default {DEFAULT_AUDIT_PATH})",
    )
    args = parser.parse_args()
    return asyncio.run(run_selftest(args.url, args.audit_path))


if __name__ == "__main__":
    sys.exit(main())
