#!/usr/bin/env python3
"""Self-test for the Module 4.1 enforcement layers of the storage and retrieval MCP servers.

Run it inside the sandbox container, with both servers already started:

    python3 mcp/allow_list_test.py

Every check calls a live server over streamable HTTP and reads the audit journal the servers write,
so nothing here is asserted from the code alone. The checks are:

  a) a granted role's ``write_entry`` succeeds
  b) an ungranted role's ``write_entry`` is refused with an ``authorization_denied`` error
  c) that refusal appended an audit record to the same log file that holds the success record
  d) an unknown role, and a call that names no role at all, are refused
  e) the classification ceiling: a role capped at ``internal`` cannot obtain the ``confidential``
     document however high a ceiling it asks for, and a role whose own cap permits it can
  f) a missing allow-list file stops a server at startup instead of letting it answer every role
  g) the two allow-list files are the exact projection of docs/routing-and-tool-grant-map.json

Check (e) needs a role whose ceiling permits the confidential document, and the routing map gives no
role such a ceiling. The test therefore starts a second retrieval server on port 8102 with a copy of
the map in which only the Reviewer ceiling is raised to ``confidential``, and reads both servers, so
the withholding on port 8002 is shown to come from the role's cap rather than from a blanket block.

Exit code 0 when every check passes, 1 otherwise. The last line is machine-readable.
"""

from __future__ import annotations

import asyncio
import json
import os
import socket
import subprocess
import sys
import time
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from mcp import ClientSession
from mcp.client.streamable_http import streamablehttp_client

WORKSPACE = Path(os.getenv("ALLOW_LIST_TEST_WORKSPACE", "/workspace"))
MEMORY_DIR = Path(os.getenv("MEMORY_DIR", str(WORKSPACE / ".memory")))
STORAGE_AUDIT = Path(os.getenv("STORAGE_AUDIT_PATH", str(MEMORY_DIR / "storage-audit.log")))
RETRIEVAL_AUDIT = Path(os.getenv("RETRIEVAL_AUDIT_PATH", str(MEMORY_DIR / "retrieval-audit.log")))
STORAGE_ALLOW_LIST = WORKSPACE / "mcp" / "storage" / "allow-list.json"
RETRIEVAL_ALLOW_LIST = WORKSPACE / "mcp" / "retrieval" / "allow-list.json"
ROUTING_MAP = WORKSPACE / "docs" / "routing-and-tool-grant-map.json"
STORAGE_SERVER = WORKSPACE / "mcp" / "storage" / "server.py"
RETRIEVAL_SERVER = WORKSPACE / "mcp" / "retrieval" / "server.py"

STORAGE_URL = os.getenv("STORAGE_MCP_URL", "http://127.0.0.1:8001/mcp")
RETRIEVAL_URL = os.getenv("RETRIEVAL_MCP_URL", "http://127.0.0.1:8002/mcp")
CONTROL_PORT = int(os.getenv("ALLOW_LIST_TEST_CONTROL_PORT", "8102"))
FAIL_LOUD_STORAGE_PORT = int(os.getenv("ALLOW_LIST_TEST_FAIL_LOUD_PORT", "8103"))
FAIL_LOUD_RETRIEVAL_PORT = int(os.getenv("ALLOW_LIST_TEST_FAIL_LOUD_PORT2", "8104"))
CONTROL_ROUTING_MAP = Path("/tmp/allow-list-test-routing-map.json")
CONTROL_AUDIT = Path("/tmp/allow-list-test-retrieval-audit.log")

PROJECT = "proj-console"
CONFIDENTIAL_DOC = "finance-hosting-costs.md"
COST_QUERY = (
    "What does hosting Komun cost per month, and what does the vendor contract commit us to?"
)
HIGH_CLASSIFICATIONS = ("confidential", "secret")
RUN_ID = uuid.uuid4().hex[:8]

RESULTS: list[tuple[str, bool, str]] = []


def check(name: str, passed: bool, detail: str = "") -> bool:
    """Record one PASS/FAIL check and print it immediately."""
    RESULTS.append((name, passed, detail))
    print(f"[{'PASS' if passed else 'FAIL'}] {name}" + (f" -- {detail}" if detail else ""), flush=True)
    return passed


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat()


def as_text(blob: Any) -> str:
    """Normalise captured process output to text, whatever the subprocess returned."""
    if blob is None:
        return ""
    if isinstance(blob, bytes):
        return blob.decode("utf-8", errors="replace")
    return str(blob)


def load_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


async def call_tool(session: ClientSession, name: str, arguments: dict) -> tuple[bool, Any, str]:
    """Call one tool and return ``(is_error, structured payload, text content)``."""
    result = await session.call_tool(name, arguments)
    text = "\n".join(
        getattr(item, "text", "")
        for item in result.content
        if getattr(item, "type", "") == "text"
    )
    payload: Any = result.structuredContent
    if isinstance(payload, dict) and set(payload) == {"result"}:
        payload = payload["result"]
    return bool(result.isError), payload, text


def documents(payload: Any) -> list[str]:
    """Return the source documents of a retrieve result, whatever shape it arrives in."""
    if not isinstance(payload, list):
        return []
    return [hit.get("source_document", "") for hit in payload if isinstance(hit, dict)]


def classifications(payload: Any) -> list[str]:
    if not isinstance(payload, list):
        return []
    return [hit.get("classification", "") for hit in payload if isinstance(hit, dict)]


def read_audit(path: Path, tail: int = 4000) -> list[dict]:
    """Return the last ``tail`` parseable JSON-Lines records of an audit journal."""
    if not path.is_file():
        return []
    records: list[dict] = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines()[-tail:]:
        line = line.strip()
        if not line:
            continue
        try:
            records.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    return records


def port_open(port: int, host: str = "127.0.0.1", timeout: float = 1.0) -> bool:
    with socket.socket() as probe:
        probe.settimeout(timeout)
        return probe.connect_ex((host, port)) == 0


def wait_for_port(port: int, timeout: float = 240.0) -> bool:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if port_open(port):
            return True
        time.sleep(1.0)
    return False


# --- check (g): the allow-list files are the routing map's own projection -------------------
def check_allow_lists_match_routing_map() -> None:
    """Prove the grants were derived rather than invented: recompute them from the map."""
    try:
        routing = load_json(ROUTING_MAP)
        storage = load_json(STORAGE_ALLOW_LIST)
        retrieval = load_json(RETRIEVAL_ALLOW_LIST)
    except (OSError, json.JSONDecodeError) as error:
        check("g. allow-list files are valid JSON beside the routing map", False, str(error))
        return

    check(
        "g1. both allow-list files parse as JSON and name all seven roles",
        sorted(storage.get("roles", {})) == sorted(routing.get("grants", {}))
        and sorted(retrieval.get("roles", {})) == sorted(routing.get("grants", {})),
        f"roles={sorted(storage.get('roles', {}))}",
    )

    def projected(server: str) -> dict[str, list[str]]:
        return {
            role: sorted(
                grant.split("__")[-1]
                for grant in grants
                if grant.startswith(f"mcp__{server}__")
            )
            for role, grants in sorted(routing["grants"].items())
        }

    expected_storage = projected("storage")
    expected_retrieval = projected("retrieval")
    actual_storage = {role: sorted(ops) for role, ops in sorted(storage["roles"].items())}
    actual_retrieval = {role: sorted(ops) for role, ops in sorted(retrieval["roles"].items())}

    check(
        "g2. storage grants equal grants[role] filtered to mcp__storage__*",
        actual_storage == expected_storage,
        f"file={actual_storage}",
    )
    check(
        "g3. retrieval grants equal grants[role] filtered to mcp__retrieval__*",
        actual_retrieval == expected_retrieval,
        f"file={actual_retrieval}",
    )
    check(
        "g4. delete_entry is granted to no role",
        all("delete_entry" not in ops for ops in actual_storage.values()),
        f"delete_entry holders={[r for r, ops in actual_storage.items() if 'delete_entry' in ops]}",
    )


# --- checks (a) to (c): the guard, and one journal for denials and successes ----------------
async def check_write_paths(storage: ClientSession) -> None:
    allowed_roles = sorted(
        role
        for role, ops in load_json(STORAGE_ALLOW_LIST)["roles"].items()
        if "write_entry" in ops
    )

    # (a) a granted role writes
    is_error, payload, text = await call_tool(
        storage,
        "write_entry",
        {
            "project_id": PROJECT,
            "entry_type": "selftest",
            "title": f"allow-list selftest {RUN_ID}",
            "content": f"written by the allow-list self-test run {RUN_ID}",
            "classification": "public",
            "calling_role": "implementer",
        },
    )
    entry_id = payload.get("entry_id") if isinstance(payload, dict) else None
    check(
        "(a) granted role implementer: write_entry succeeds",
        not is_error and bool(entry_id),
        f"entry_id={entry_id}",
    )

    # (b) an ungranted role is refused, with the three parties named
    is_error, _, text = await call_tool(
        storage,
        "write_entry",
        {
            "project_id": PROJECT,
            "entry_type": "selftest",
            "title": f"denied write {RUN_ID}",
            "content": "this write must be refused",
            "classification": "public",
            "calling_role": "project-manager",
        },
    )
    names_allowed = all(role in text for role in allowed_roles)
    check(
        "(b) ungranted role project-manager: write_entry refused with authorization_denied",
        is_error
        and "authorization_denied" in text
        and "project-manager" in text
        and "write_entry" in text
        and names_allowed,
        text.splitlines()[0][:220] if text else "no error text",
    )
    check(
        "(b2) that refusal names every role that IS allowed to write",
        names_allowed,
        f"allowed_roles={allowed_roles}",
    )

    is_error, _, text = await call_tool(
        storage,
        "write_entry",
        {
            "project_id": PROJECT,
            "entry_type": "selftest",
            "title": f"denied write {RUN_ID}",
            "content": "the orchestrator holds no storage operation at all",
            "classification": "public",
            "calling_role": "orchestrator",
        },
    )
    check(
        "(b3) orchestrator, which holds no storage grant, is refused too",
        is_error and "authorization_denied" in text and "orchestrator" in text,
        text.splitlines()[0][:220] if text else "no error text",
    )

    # (c) both records, in the one journal file
    records = read_audit(STORAGE_AUDIT)
    lines = STORAGE_AUDIT.read_text(encoding="utf-8", errors="replace").splitlines()
    denial_lines = [
        index
        for index, line in enumerate(lines)
        if '"calling_role": "project-manager"' in line
        and '"allowed": false' in line
        and "authorization_denied" in line
    ]
    success_lines = [index for index, line in enumerate(lines) if entry_id and entry_id in line]
    check(
        "(c) the denial and the success are records in the SAME journal file",
        bool(denial_lines) and bool(success_lines),
        f"{STORAGE_AUDIT} denial_line={denial_lines[-1] if denial_lines else None} "
        f"success_line={success_lines[-1] if success_lines else None} "
        f"total_lines={len(lines)}",
    )
    check(
        "(c2) the denial was journalled after the success it is compared with",
        bool(denial_lines) and bool(success_lines) and denial_lines[-1] > success_lines[-1],
        f"denial at {denial_lines[-1] if denial_lines else None}, "
        f"success at {success_lines[-1] if success_lines else None}",
    )
    check(
        "(c3) exactly one storage audit journal exists under .memory (no second log file)",
        len(sorted(MEMORY_DIR.glob("storage-audit*"))) == 1,
        f"candidates={[p.name for p in sorted(MEMORY_DIR.glob('storage-audit*'))]}",
    )
    denial = next(
        (record for record in reversed(records) if record.get("calling_role") == "project-manager"),
        None,
    )
    check(
        "(c4) the journalled denial carries operation, role, allowed=false and the reason",
        bool(denial)
        and denial.get("operation") == "write_entry"
        and denial.get("allowed") is False
        and "authorization_denied" in (denial.get("reason") or ""),
        json.dumps(denial, sort_keys=True)[:300] if denial else "no denial record found",
    )


# --- check (d): unknown roles, and a call that names no role --------------------------------
async def check_unknown_roles(storage: ClientSession, retrieval: ClientSession) -> None:
    is_error, _, text = await call_tool(
        storage,
        "write_entry",
        {
            "project_id": PROJECT,
            "entry_type": "selftest",
            "title": f"unknown role {RUN_ID}",
            "content": "an unrecognised role must be refused",
            "classification": "public",
            "calling_role": "intern",
        },
    )
    check(
        "(d1) unknown role 'intern' refused on storage, and named as unknown",
        is_error and "authorization_denied" in text and "intern" in text and "unknown role" in text,
        text.splitlines()[0][:220] if text else "no error text",
    )

    is_error, _, text = await call_tool(
        storage,
        "read_entry",
        {"project_id": PROJECT, "entry_id": "0f6b7b1e-0000-4000-8000-000000000000"},
    )
    check(
        "(d2) a call that names NO role is refused, not defaulted to an allowed role",
        is_error and "authorization_denied" in text and "unknown" in text,
        text.splitlines()[0][:220] if text else "no error text",
    )

    is_error, _, text = await call_tool(
        retrieval,
        "retrieve",
        {
            "query": COST_QUERY,
            "project_id": PROJECT,
            "top_k": 3,
            "classification_ceiling": "internal",
            "calling_role": "intern",
        },
    )
    check(
        "(d3) unknown role 'intern' refused on retrieval too",
        is_error and "authorization_denied" in text and "intern" in text,
        text.splitlines()[0][:220] if text else "no error text",
    )

    is_error, _, text = await call_tool(
        retrieval,
        "retrieve",
        {"query": COST_QUERY, "project_id": PROJECT, "top_k": 3, "calling_role": "tester"},
    )
    check(
        "(d4) tester, which is refused retrieval outright, is refused with the allowed roles named",
        is_error
        and "authorization_denied" in text
        and "tester" in text
        and all(role in text for role in ("implementer", "planner", "reviewer")),
        text.splitlines()[0][:220] if text else "no error text",
    )


# --- check (e): the role ceiling, on both servers -------------------------------------------
async def check_ceiling(player: ClientSession, label: str, control_map: dict[str, str]) -> None:
    """Run the escalation matrix against one retrieval server."""
    escalation = [
        ("implementer", "public"),
        ("implementer", "internal"),
        ("implementer", "confidential"),
        ("implementer", "secret"),
        ("planner", "confidential"),
        ("reviewer", "secret"),
    ]
    for role, requested in escalation:
        is_error, payload, text = await call_tool(
            player,
            "retrieve",
            {
                "query": COST_QUERY,
                "project_id": PROJECT,
                "top_k": 5,
                "classification_ceiling": requested,
                "calling_role": role,
            },
        )
        found = classifications(payload)
        leaked = [value for value in found if value in HIGH_CLASSIFICATIONS]
        check(
            f"(e) {label}: role {role} capped at {control_map[role]} asking for {requested} "
            f"gets no confidential document",
            not is_error and CONFIDENTIAL_DOC not in documents(payload) and not leaked,
            f"docs={documents(payload)} classifications={found}",
        )

    # a granted role inside its own ceiling still returns results
    is_error, payload, text = await call_tool(
        player,
        "retrieve",
        {
            "query": COST_QUERY,
            "project_id": PROJECT,
            "top_k": 5,
            "classification_ceiling": "public",
            "calling_role": "implementer",
        },
    )
    found = classifications(payload)
    check(
        f"(e) {label}: implementer asking for public still gets results, all at or below public",
        not is_error and bool(found) and set(found) <= {"public"},
        f"count={len(found)} classifications={sorted(set(found))}",
    )


async def check_positive_control() -> None:
    """A second server whose map permits the Reviewer to reach confidential, as the control."""
    routing = load_json(ROUTING_MAP)
    control_routing = json.loads(json.dumps(routing))
    control_routing["retrieval_ceiling"]["reviewer"] = "confidential"
    CONTROL_ROUTING_MAP.write_text(json.dumps(control_routing, indent=2) + "\n", encoding="utf-8")
    if CONTROL_AUDIT.exists():
        CONTROL_AUDIT.unlink()

    environment = {
        **os.environ,
        "RETRIEVAL_ROUTING_MAP_PATH": str(CONTROL_ROUTING_MAP),
        "RETRIEVAL_AUDIT_PATH": str(CONTROL_AUDIT),
    }
    log = open("/tmp/allow-list-test-control-server.log", "w", encoding="utf-8")
    process = subprocess.Popen(  # noqa: S603 - a fixed argv naming the repository's own server
        [
            sys.executable,
            str(RETRIEVAL_SERVER),
            "--port",
            str(CONTROL_PORT),
            "--host",
            "127.0.0.1",
            "--chunking",
            "paragraph",
            "--routing-map-path",
            str(CONTROL_ROUTING_MAP),
            "--audit-path",
            str(CONTROL_AUDIT),
        ],
        cwd=str(WORKSPACE),
        env=environment,
        stdout=log,
        stderr=subprocess.STDOUT,
    )
    try:
        ready = wait_for_port(CONTROL_PORT, timeout=240)
        check(
            "(e4) control server started with the Reviewer ceiling raised to confidential",
            ready,
            f"port={CONTROL_PORT} routing_map={CONTROL_ROUTING_MAP}",
        )
        if not ready:
            return

        async with streamablehttp_client(f"http://127.0.0.1:{CONTROL_PORT}/mcp") as (
            read,
            write,
            _,
        ):
            async with ClientSession(read, write) as session:
                await session.initialize()
                is_error, payload, _ = await call_tool(
                    session,
                    "retrieve",
                    {
                        "query": COST_QUERY,
                        "project_id": PROJECT,
                        "top_k": 5,
                        "classification_ceiling": "confidential",
                        "calling_role": "reviewer",
                    },
                )
                check(
                    "(e5) with Reviewer capped at confidential, the same query DOES return the "
                    "confidential document",
                    not is_error and CONFIDENTIAL_DOC in documents(payload),
                    f"docs={documents(payload)}",
                )

                is_error, payload, _ = await call_tool(
                    session,
                    "retrieve",
                    {
                        "query": COST_QUERY,
                        "project_id": PROJECT,
                        "top_k": 5,
                        "classification_ceiling": "confidential",
                        "calling_role": "implementer",
                    },
                )
                check(
                    "(e6) on that same server the implementer, still capped at internal, does NOT",
                    not is_error and CONFIDENTIAL_DOC not in documents(payload),
                    f"docs={documents(payload)}",
                )
    finally:
        process.terminate()
        try:
            process.wait(timeout=15)
        except subprocess.TimeoutExpired:
            process.kill()
        log.close()


def check_withholding_journal() -> None:
    """The withheld call must be journalled, in the one retrieval journal."""
    records = read_audit(RETRIEVAL_AUDIT)
    withheld = [
        record
        for record in records
        if record.get("calling_role") in {"implementer", "planner", "reviewer"}
        and record.get("requested_ceiling") in HIGH_CLASSIFICATIONS
        and record.get("role_ceiling") == "internal"
        and record.get("effective_ceiling") == "internal"
    ]
    check(
        "(e7) the escalation attempt is journalled as a withholding, effective ceiling internal",
        any(record.get("decision") == "withheld_ceiling" and record.get("withheld") is True
            for record in withheld),
        json.dumps(withheld[-1], sort_keys=True)[:320] if withheld else "no withholding record",
    )
    denied = [record for record in records if record.get("decision") == "denied"]
    check(
        "(d5) retrieval denials are journalled in the same retrieval journal",
        any(record.get("calling_role") == "intern" for record in denied),
        json.dumps(denied[-1], sort_keys=True)[:320] if denied else "no denied record",
    )
    allowed = [record for record in records if record.get("decision") == "allowed"]
    check(
        "(e8) allowed retrieval calls are journalled beside the denials, with their ceiling",
        bool(allowed) and all(record.get("ceiling") for record in allowed),
        json.dumps(allowed[-1], sort_keys=True)[:320] if allowed else "no allowed record",
    )


# --- check (f): a missing allow-list stops the server ---------------------------------------
def run_fail_loud(name: str, argv: list[str], env_extra: dict[str, str], port: int) -> None:
    """Start a server against a missing allow-list and require a clear startup refusal."""
    missing = "/workspace/mcp/selftest-missing-allow-list.json"
    environment = {**os.environ, **env_extra}
    started = time.monotonic()
    try:
        completed = subprocess.run(  # noqa: S603 - a fixed argv naming the repository's own server
            argv,
            cwd=str(WORKSPACE),
            env=environment,
            capture_output=True,
            text=True,
            timeout=90,
            check=False,
        )
        output = as_text(completed.stdout) + as_text(completed.stderr)
        exit_code: int | None = completed.returncode
    except subprocess.TimeoutExpired as expired:
        output = as_text(expired.stdout) + as_text(expired.stderr)
        exit_code = None
    elapsed = round(time.monotonic() - started, 2)

    check(
        f"(f) {name}: a missing allow-list file exits non-zero instead of serving",
        exit_code not in (0, None),
        f"exit_code={exit_code} after {elapsed}s",
    )
    check(
        f"(f) {name}: the refusal is a clear error naming the missing path",
        "ERROR:" in output
        and "allow-list file not found" in output
        and missing in output
        and "refusing to start" in output,
        output.strip().splitlines()[-1][:250] if output.strip() else "no output",
    )
    check(
        f"(f) {name}: nothing is listening on port {port} afterwards",
        not port_open(port),
        f"port={port} open={port_open(port)}",
    )


def check_fail_loud() -> None:
    missing = "/workspace/mcp/selftest-missing-allow-list.json"
    run_fail_loud(
        "storage (env STORAGE_ALLOW_LIST_PATH)",
        [sys.executable, str(STORAGE_SERVER), "--port", str(FAIL_LOUD_STORAGE_PORT)],
        {"STORAGE_ALLOW_LIST_PATH": missing},
        FAIL_LOUD_STORAGE_PORT,
    )
    run_fail_loud(
        "retrieval (--allowlist-path)",
        [
            sys.executable,
            str(RETRIEVAL_SERVER),
            "--port",
            str(FAIL_LOUD_RETRIEVAL_PORT),
            "--allowlist-path",
            missing,
        ],
        {},
        FAIL_LOUD_RETRIEVAL_PORT,
    )


async def main() -> int:
    print(f"allow-list self-test run {RUN_ID} at {utc_now()}")
    print(f"storage   {STORAGE_URL}   journal {STORAGE_AUDIT}")
    print(f"retrieval {RETRIEVAL_URL}   journal {RETRIEVAL_AUDIT}")
    print()

    check_allow_lists_match_routing_map()

    async with streamablehttp_client(STORAGE_URL) as (read, write, _):
        async with ClientSession(read, write) as storage:
            await storage.initialize()
            async with streamablehttp_client(RETRIEVAL_URL) as (rread, rwrite, _):
                async with ClientSession(rread, rwrite) as retrieval:
                    await retrieval.initialize()
                    await check_write_paths(storage)
                    await check_unknown_roles(storage, retrieval)
                    await check_ceiling(
                        retrieval,
                        "live server 8002",
                        {
                            role: ceiling
                            for role, ceiling in load_json(ROUTING_MAP)["retrieval_ceiling"].items()
                        },
                    )

    await check_positive_control()
    check_withholding_journal()
    check_fail_loud()

    passed = sum(1 for _, ok, _ in RESULTS if ok)
    total = len(RESULTS)
    rate = round(100.0 * passed / total, 1) if total else 0.0
    print()
    for name, ok, detail in RESULTS:
        if not ok:
            print(f"FAILED: {name} -- {detail}")
    print(f"SELFTEST_RESULT passed={passed} total={total} rate={rate}")
    return 0 if passed == total else 1


if __name__ == "__main__":
    raise SystemExit(asyncio.run(main()))
