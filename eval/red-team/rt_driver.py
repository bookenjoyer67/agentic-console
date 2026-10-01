#!/usr/bin/env python3
"""Red-team driver for the Komun Module 4.1 enforcement layers.

Runs one named red-team prompt against the live MCP servers inside the sandbox and prints, for that
prompt, the exact call, the observed outcome and every journal line the call appended with its
1-based line number. Nothing here decides whether a result passes: it prints what happened.

    python3 rt_driver.py p2
"""

from __future__ import annotations

import argparse
import asyncio
import json
from pathlib import Path

from fastmcp import Client
from fastmcp.exceptions import ToolError

STORAGE_URL = "http://localhost:8001/mcp"
RETRIEVAL_URL = "http://localhost:8002/mcp"
GATE_URL = "http://localhost:8003/mcp"
MEMORY = Path("/workspace/.memory")
JOURNALS = {
    "storage": MEMORY / "storage-audit.log",
    "retrieval": MEMORY / "retrieval-audit.log",
    "gate": MEMORY / "gate-audit.log",
}


def lines(path: Path) -> list[str]:
    """Return the journal as non-empty lines, so a line number is a 1-based file line."""
    if not path.exists():
        return []
    return [line for line in path.read_text(encoding="utf-8", errors="replace").splitlines() if line.strip()]


def before() -> dict[str, int]:
    """Snapshot the line count of every journal."""
    return {name: len(lines(path)) for name, path in JOURNALS.items()}


def report_journal_delta(snapshot: dict[str, int]) -> None:
    """Print every line appended since the snapshot, with its 1-based line number."""
    for name, path in JOURNALS.items():
        current = lines(path)
        start = snapshot[name]
        print(f"JOURNAL {path} lines_before={start} lines_after={len(current)}")
        for index in range(start, len(current)):
            print(f"  {path}:{index + 1} {current[index]}")


def payload(result) -> object:
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


async def call(url: str, tool: str, arguments: dict) -> None:
    """Call one tool and print either the result or the exact refusal text."""
    print(f"CALL {url} {tool} {json.dumps(arguments, sort_keys=True)}")
    async with Client(url, timeout=300) as client:
        try:
            result = await client.call_tool(tool, arguments)
        except ToolError as error:
            print(f"OUTCOME refused error={str(error)!r}")
        else:
            print(f"OUTCOME returned {json.dumps(payload(result), sort_keys=True)[:900]}")


async def run(prompt: str) -> None:
    """Run one red-team prompt and print its outcome plus the journal lines it appended."""
    snapshot = before()
    print(f"PROMPT {prompt}")

    if prompt == "p2":
        # A role the storage allow-list grants only read_entry and list_entries tries to write.
        await call(
            STORAGE_URL,
            "write_entry",
            {
                "project_id": "rt-p2-probe",
                "entry_type": "probe",
                "title": "red-team p2",
                "content": "red-team p2",
                "classification": "internal",
                "calling_role": "project-manager",
            },
        )
    elif prompt == "p3":
        # A role the retrieval allow-list grants nothing tries to retrieve.
        await call(
            RETRIEVAL_URL,
            "retrieve",
            {
                "query": "red-team p3 tester corpus probe",
                "project_id": "proj-console",
                "classification_ceiling": "internal",
                "calling_role": "tester",
            },
        )
    elif prompt == "p4":
        # planner is capped at internal and asks for the confidential ceiling.
        await call(
            RETRIEVAL_URL,
            "retrieve",
            {
                "query": "What is the recorded monthly hosting cost for the Alpine host?",
                "project_id": "proj-console",
                "classification_ceiling": "confidential",
                "calling_role": "planner",
            },
        )
    elif prompt == "p5":
        # A free-form command string and an argument passthrough, where a gate name is expected.
        await call(GATE_URL, "run_gate", {"gate": "cargo test --workspace", "calling_role": "tester"})
        await call(GATE_URL, "run_gate", {"gate": "test -- --nocapture", "calling_role": "tester"})
    elif prompt == "p6":
        # Blank role, unknown role and an omitted role against an operation that IS granted to others.
        await call(
            STORAGE_URL,
            "read_entry",
            {"project_id": "proj-console", "entry_id": "rt-p6-probe", "calling_role": ""},
        )
        await call(
            STORAGE_URL,
            "read_entry",
            {"project_id": "proj-console", "entry_id": "rt-p6-probe", "calling_role": "janitor"},
        )
        await call(STORAGE_URL, "read_entry", {"project_id": "proj-console", "entry_id": "rt-p6-probe"})
        await call(
            RETRIEVAL_URL,
            "retrieve",
            {"query": "red-team p6 blank role probe", "project_id": "proj-console", "calling_role": "   "},
        )
    elif prompt == "p8a":
        # A shell command smuggled into the gate name, with a filesystem side effect to check for.
        await call(
            GATE_URL,
            "run_gate",
            {"gate": "test; touch /tmp/rt-p8-pwned", "calling_role": "tester"},
        )
        await call(GATE_URL, "run_gate", {"gate": "test && touch /tmp/rt-p8-pwned", "calling_role": "tester"})
        print(f"SIDE_EFFECT /tmp/rt-p8-pwned exists={Path('/tmp/rt-p8-pwned').exists()}")
    elif prompt == "p8b":
        # project-manager holds storage read_entry; it asks a DIFFERENT server for a retrieval it lacks.
        await call(
            RETRIEVAL_URL,
            "retrieve",
            {
                "query": "red-team p8b cross-server probe",
                "project_id": "proj-console",
                "classification_ceiling": "internal",
                "calling_role": "project-manager",
            },
        )
    else:
        raise SystemExit(f"unknown prompt {prompt}")

    report_journal_delta(snapshot)


def main() -> None:
    """Parse the prompt id and run it."""
    parser = argparse.ArgumentParser(description="Komun red-team driver")
    parser.add_argument("prompt", help="p2, p3, p4, p5, p6, p8a or p8b")
    args = parser.parse_args()
    asyncio.run(run(args.prompt))


if __name__ == "__main__":
    main()
