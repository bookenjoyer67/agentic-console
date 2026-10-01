"""Do the memory-layer guards hold on the one path that can actually write?

`CLAUDE.md` says `.memory/knowledge/` and `.memory/reference/` are human-maintained and read-only to
every role, and `.memory/SCOPE.md` repeats it. Two artifacts enforce that, against two different tool
families:

* `.claude/hooks/guard-readonly-memory.sh` — a `PreToolUse` hook on the harness's `Write`, `Edit` and
  `MultiEdit` tools, wired up in `.claude/settings.json`.
* `mcp/coursetools_server.py` — `writable_path()`, used by the `file_write` tool.

A hook sees harness tools and never sees a tool called over MCP, so the hook alone left the rule
enforceable on one path only: `mcp__coursetools__file_write`, which the routing map grants to
`implementer`, could write into either layer. `safe_path()` was no help, because it only asks whether a
path resolves inside the project root. Both halves are checked here, so a future edit that drops one is
caught.

`writable_path()` rather than `safe_path()` on the write path is deliberate, and checked below: reads
must keep working on these layers, because a role that may not write `coding-standards.md` still has to
be able to read it.

Run from the repository root, where the container runs its suites:

    python3 -m pytest eval/test_coursetools_paths.py -q

The module is loaded by path rather than by name: this repository's `mcp/` directory has no
`__init__.py`, and the MCP SDK installs a top-level `mcp` module of its own, so `import
mcp.coursetools_server` resolves to the SDK. A bare checkout has no SDK at all, which is why the loader
below substitutes a minimal `FastMCP` — the class is used only as a decorator factory.
"""

from __future__ import annotations

import importlib.util
import os
import sys
import types
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
SERVER = ROOT / "mcp" / "coursetools_server.py"

# Paths inside a layer a human maintains. None of these may be written, by any role.
READ_ONLY_LAYER_PATHS = (
    ".memory/knowledge/coding-standards.md",
    ".memory/knowledge/handoff-subagent-to-orchestrator.md",
    ".memory/knowledge/a-file-that-does-not-exist-yet.md",
    ".memory/knowledge/nested/deeper/still-refused.md",
    ".memory/reference/reference-gate-vocabulary.md",
    ".memory/reference/forged-document.md",
    ".memory/knowledge",
    ".memory/reference",
)

# Legitimate write targets, including one in `.memory` itself: the decision log is what the refusal
# message tells the agent to use instead.
WRITABLE_PATHS = (
    "docs/ok.md",
    "src/main.rs",
    ".memory/project/decisions/decision-002-something.md",
    ".memory/project/MEMORY_INDEX.md",
    ".memory/SCOPE.md",
)

# Traversal that lands back inside a layer. The guard resolves the path before it matches, so a path
# that never mentions a layer textually is still refused. A textual or prefix-matching guard misses
# this, which is why it is checked rather than assumed.
TRAVERSAL_INTO_A_LAYER = (
    "docs/../.memory/knowledge/coding-standards.md",
    "./.memory/knowledge/./coding-standards.md",
    "mcp/../.memory/reference/forged.md",
)

# Escapes: `..` out of the layer and `..` out of the root are both refused.
ESCAPING_PATHS = (
    "../../etc/passwd",
    ".memory/knowledge/../../../etc/passwd",
    "mcp/../../.memory/reference/forged.md",
)


@pytest.fixture(scope="module")
def cts():
    """The coursetools server module, loaded by path with the SDK absent-or-real."""
    try:
        import mcp.server.fastmcp  # noqa: F401
    except ModuleNotFoundError:
        package = types.ModuleType("mcp")
        package.__path__ = []  # type: ignore[attr-defined]
        subpackage = types.ModuleType("mcp.server")
        subpackage.__path__ = []  # type: ignore[attr-defined]
        fastmcp = types.ModuleType("mcp.server.fastmcp")

        class FastMCP:
            def __init__(self, *args, **kwargs) -> None:
                pass

            def tool(self, *args, **kwargs):
                def decorate(function):
                    return function

                return decorate

            def run(self) -> None:
                pass

        fastmcp.FastMCP = FastMCP  # type: ignore[attr-defined]
        sys.modules.setdefault("mcp", package)
        sys.modules.setdefault("mcp.server", subpackage)
        sys.modules["mcp.server.fastmcp"] = fastmcp

    os.environ.setdefault("COURSETOOLS_ROOT", str(ROOT))
    spec = importlib.util.spec_from_file_location("coursetools_under_test", SERVER)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


# --- the rule this file exists for ------------------------------------------------------------
@pytest.mark.parametrize("path", READ_ONLY_LAYER_PATHS)
def test_file_write_refuses_a_human_maintained_layer(cts, path: str) -> None:
    """Every layer path is refused on the write path, whether or not the file exists."""
    with pytest.raises(PermissionError) as refusal:
        cts.writable_path(path)
    assert "human-maintained" in str(refusal.value), str(refusal.value)
    # The refusal has to tell the agent where the change DOES belong, or it will retry the same call.
    assert ".memory/project/decisions/" in str(refusal.value)
    assert ".memory/project/MEMORY_INDEX.md" in str(refusal.value)


def test_write_denied_layers_name_both_layers(cts) -> None:
    """The covered set is both layers, stated once, not one layer in two places."""
    assert set(cts.WRITE_DENIED_LAYERS) == {".memory/knowledge", ".memory/reference"}


# --- no over-blocking: the legal targets and the reads -----------------------------------------
@pytest.mark.parametrize("path", WRITABLE_PATHS)
def test_file_write_allows_everything_outside_the_layers(cts, path: str) -> None:
    """A guard that blocks legal work is a guard that gets removed."""
    assert cts.writable_path(path) == (ROOT / path).resolve()


@pytest.mark.parametrize(
    "path",
    [
        ".memory/knowledge/coding-standards.md",
        ".memory/reference/reference-gate-vocabulary.md",
    ],
)
def test_reads_still_work_on_the_read_only_layers(cts, path: str) -> None:
    """The layers are read-only, not unreadable: `safe_path` admits what `writable_path` refuses."""
    assert cts.safe_path(path) == (ROOT / path).resolve()


# --- containment ------------------------------------------------------------------------------
@pytest.mark.parametrize("path", ESCAPING_PATHS)
def test_a_path_escaping_the_root_is_refused(cts, path: str) -> None:
    with pytest.raises(ValueError) as escape:
        cts.writable_path(path)
    assert "escapes the project root" in str(escape.value)


@pytest.mark.parametrize("path", TRAVERSAL_INTO_A_LAYER)
def test_traversal_that_lands_inside_a_layer_is_refused(cts, path: str) -> None:
    """The layer is decided by where the path lands, not by the text of the path.

    `docs/../.memory/knowledge/coding-standards.md` opens with `docs`, so a guard that matched the
    string from the start of the path would let it through.
    """
    with pytest.raises(PermissionError):
        cts.writable_path(path)


def test_traversal_that_leaves_a_layer_but_stays_in_the_root_is_allowed(cts) -> None:
    """...and the converse, so the guard cannot be tightened until it blocks legitimate work.

    `.memory/knowledge/../../knowledge/x.md` resolves to `<root>/knowledge/x.md`: outside both layers
    and inside the repository, so it is a legal write target.
    """
    assert cts.writable_path(".memory/knowledge/../../knowledge/x.md") == (
        ROOT / "knowledge" / "x.md"
    ).resolve()


def test_a_sibling_whose_name_merely_starts_with_the_root_is_not_contained(cts, tmp_path, monkeypatch) -> None:
    """The hole a `startswith`-based check leaves.

    With the root at `<base>/root`, a string-prefix test admits `<base>/root-old` as inside it. The
    check walks the resolved path's parents instead, so the sibling is refused.
    """
    (tmp_path / "root").mkdir()
    (tmp_path / "root-old").mkdir()
    monkeypatch.setattr(cts, "ROOT", tmp_path / "root")

    with pytest.raises(ValueError):
        cts.safe_path("../root-old/planted.md")

    # ...while the real root is still admitted, so the check is not simply refusing everything.
    assert cts.safe_path("inside.md") == (tmp_path / "root" / "inside.md").resolve()
