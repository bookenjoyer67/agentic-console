"""Dummy MCP server for the Komun scoped-agent orchestration exercise (Agentic Engineer 3.1/3.2).

This server is a safe stand-in for real course tools. It is deliberately small and it does NOT
execute anything: ``shell`` returns a canned string, ``test_runner`` returns a recorded gate
baseline, and ``web_search`` returns a fixed answer. The only behaviour that is real is the
role allow-list check.

Authoritative reference for the tool set, the server name and the allow-list mechanism:
``/tmp/lcae/module_3/mcp/coursetools_server.py`` in the course package. The canned content is
Komun's, not the course's (the course mocks a CSV-escaping question and a generic test suite).

Each tool expects a ``role`` argument so students can verify that denied calls fail with an
authorization error. Denied calls raise ``PermissionError`` naming the role, the tool and the
roles that are allowed to call it.

The allow-list is loaded from ``mcp/roles.allowlist.json`` (override with ``COURSETOOLS_ALLOWLIST``);
relative paths resolve against the current working directory first, then against this file's
directory, because Claude Code spawns the server as
``python /workspace/mcp/coursetools_server.py`` with the repo root as the working directory.

Use it only for the local governed-orchestration exercises.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
from typing import Any

from mcp.server.fastmcp import FastMCP

SERVER_NAME = "coursetools"
mcp = FastMCP(SERVER_NAME)

HERE = Path(__file__).resolve().parent
ROOT = Path(os.environ.get("COURSETOOLS_ROOT", ".")).resolve()


def _resolve(path_value: str) -> Path:
    candidate = Path(path_value)
    if candidate.is_absolute() or candidate.exists():
        return candidate
    return HERE / candidate


ALLOWLIST_PATH = _resolve(os.environ.get("COURSETOOLS_ALLOWLIST", "mcp/roles.allowlist.json"))

# Mirrors mcp/roles.allowlist.json. The file wins when it is present; this copy keeps the
# denial check working if the file is missing.
DEFAULT_ALLOWLIST: dict[str, list[str]] = {
    "file_read": ["planner", "implementer", "tester", "reviewer", "orchestrator"],
    "file_write": ["implementer", "orchestrator"],
    "codebase_search": ["planner", "implementer", "reviewer"],
    "shell": [],
    "test_runner": ["tester"],
    "task_tracker": ["project-manager"],
    "web_search": ["researcher"],
}


def load_allowlist() -> dict[str, list[str]]:
    if ALLOWLIST_PATH.exists():
        return json.loads(ALLOWLIST_PATH.read_text(encoding="utf-8"))
    return DEFAULT_ALLOWLIST


ALLOWLIST = load_allowlist()


def authorize(tool_name: str, role: str) -> None:
    """Raise PermissionError unless role is on the allow-list for tool_name."""
    allowed_roles = ALLOWLIST.get(tool_name, [])
    if role not in allowed_roles:
        raise PermissionError(
            f"Authorization error: role '{role}' is not on the allow-list for {tool_name}. "
            f"Allowed roles: {allowed_roles or 'none'}."
        )


# The layers a human maintains. Every role may READ them; no role may write them, whatever grant it
# holds. The rule is stated in `.memory/SCOPE.md` and `CLAUDE.md`, and `.memory/knowledge` is chmod 444
# — but the container's agent runs as root and root ignores a file mode, so the rule is enforced here,
# where the write actually happens, and again at the tool layer by
# `.claude/hooks/guard-readonly-memory.sh`.
WRITE_DENIED_LAYERS = (".memory/knowledge", ".memory/reference")


def safe_path(path: str) -> Path:
    """Resolve a path under the project root, or refuse it.

    Containment is checked by walking the resolved path's parents rather than by comparing string
    prefixes: `startswith` accepts a sibling whose name merely begins with the root's (`/workspace`
    admitting `/workspace-old`), which is the failure mode this guard exists to prevent.
    """
    candidate = (ROOT / path).resolve()
    if not candidate.is_relative_to(ROOT):
        raise ValueError(f"Path escapes the project root: {path}")
    return candidate


def writable_path(path: str) -> Path:
    """Resolve a write target, refusing anything inside a human-maintained memory layer.

    Applied by ``file_write`` alone. Reads must keep working on these layers, so this is not folded
    into ``safe_path``: the check belongs to the operation that writes.
    """
    target = safe_path(path)
    for layer in WRITE_DENIED_LAYERS:
        if target == (ROOT / layer) or target.is_relative_to(ROOT / layer):
            raise PermissionError(
                f"refused: {path} is inside {layer}/, which is human-maintained and read-only to "
                "every role. Record the change as a decision entry in .memory/project/decisions/ and "
                "register it in .memory/project/MEMORY_INDEX.md instead, or ask the human to make "
                "the edit."
            )
    return target


# Komun's real gates (AGENTS.md:196-198, docs/DEVELOPMENT.md:173-189). Nothing here is run:
# these strings are the last recorded baseline, returned verbatim so the exercise output is
# deterministic.
KOMUN_GATES: list[str] = [
    "cargo test --workspace",
    "cargo clippy --release -- -D warnings",
    "cargo fmt --check",
    "cd web && npm run check",
    "cd web && npx vitest run",
]

RECORDED_GATE_RESULTS: dict[str, str] = {
    "cargo test --workspace": "138 passed, 0 failed, 0 ignored (agentic-console)",
    "cargo clippy --release -- -D warnings": "0 warnings",
    "cargo fmt --check": "clean, 0 diffs",
    "cd web && npm run check": "0 errors, 0 warnings",
    "cd web && npx vitest run": "82 tests in 7 files, all passing",
}

DEFAULT_SUITE = "cargo test --workspace"


@mcp.tool()
def file_read(role: str, path: str) -> str:
    """Read a UTF-8 text file from the project root."""
    authorize("file_read", role)
    target = safe_path(path)
    if not target.exists() or not target.is_file():
        raise FileNotFoundError(path)
    return target.read_text(encoding="utf-8")


@mcp.tool()
def file_write(role: str, path: str, content: str) -> str:
    """Write a UTF-8 text file under the project root.

    Refuses a target inside a human-maintained memory layer. The tool-layer hook
    (`.claude/hooks/guard-readonly-memory.sh`) covers the harness's Write/Edit tools; this check covers
    the same rule for a write that arrives through this server, which that hook never sees.
    """
    authorize("file_write", role)
    target = writable_path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(content, encoding="utf-8")
    return f"Wrote {path} ({len(content)} bytes)."


@mcp.tool()
def codebase_search(role: str, query: str, root: str = ".", max_results: int = 20) -> list[dict[str, Any]]:
    """Search text files under the project root for a query string."""
    authorize("codebase_search", role)
    search_root = safe_path(root)
    results: list[dict[str, Any]] = []
    ignored_dirs = {".git", "node_modules", ".venv", "__pycache__", "target"}

    for path in search_root.rglob("*"):
        if len(results) >= max_results:
            break
        if any(part in ignored_dirs for part in path.parts):
            continue
        if not path.is_file():
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue
        line_matches = []
        for number, line in enumerate(text.splitlines(), start=1):
            if query.lower() in line.lower():
                line_matches.append({"line": number, "text": line.strip()})
                if len(line_matches) >= 3:
                    break
        if line_matches:
            results.append({"path": str(path.relative_to(ROOT)), "matches": line_matches})
    return results


@mcp.tool()
def shell(role: str, command: str) -> str:
    """Simulate a shell command. This dummy server does not execute arbitrary commands."""
    authorize("shell", role)
    return (
        f"Simulated shell command only; nothing executed: {command}\n"
        "Real Komun gates, run by hand inside the container: "
        + "; ".join(KOMUN_GATES)
        + "."
    )


@mcp.tool()
def test_runner(role: str, suite: str = DEFAULT_SUITE) -> str:
    """Return a recorded Komun gate result for the requested suite. Nothing is executed."""
    authorize("test_runner", role)
    if suite not in RECORDED_GATE_RESULTS:
        known = ", ".join(RECORDED_GATE_RESULTS)
        return f"PASS: no recorded baseline for suite '{suite}'. Known suites: {known}."
    return f"PASS: recorded baseline for '{suite}' -> {RECORDED_GATE_RESULTS[suite]}."


@mcp.tool()
def task_tracker(role: str, ticket_id: str, status: str = "done", note: str = "") -> str:
    """Simulate updating a shared work ticket."""
    authorize("task_tracker", role)
    return f"Ticket {ticket_id} updated to {status}. Note: {note or 'No note provided.'}"


@mcp.tool()
def web_search(role: str, question: str) -> dict[str, Any]:
    """Return a deterministic, example research answer; the answer is about this repo's stack."""
    authorize("web_search", role)
    return {
        "answer": (
            "Komun adds a SQLx migration as a new numbered file under migrations/ and applies it "
            "through sqlx::migrate! at startup, so a schema change never edits an already-applied "
            "migration."
        ),
        "key_facts": [
            "Migrations are numbered files: 001_schema.sql, 002_directory_open_registration.sql, "
            "003_drop_matches_message.sql (ls migrations).",
            "The server embeds them with sqlx::migrate!(\"../../migrations\").",
            "Add a new file for a new change; never rewrite an applied migration, because the "
            "recorded version checksum stops matching.",
            "Keep the Rust query and the generated frontend types in step, then re-run "
            "cargo test --workspace and cd web && npm run check.",
        ],
        "sources": [
            "sqlx migration guide (embedded migrations, version checksums)",
            "SvelteKit + Axum integration notes: the Axum route and the SvelteKit load function "
            "change together",
        ],
        "question": question,
    }


if __name__ == "__main__":
    mcp.run()
