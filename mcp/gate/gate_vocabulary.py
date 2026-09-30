#!/usr/bin/env python3
"""The command vocabulary this repository's gate server exposes, read from ``agentic.config.json``.

``mcp/gate/server.py`` is the execution surface; everything a fork must change about it -- the
command names, the argv tuple of each, the mode each runs in, the clippy cache-hit guard, the
container paths the commands run in -- is read here, from the config's ``toolchain.commands`` and
``containers`` blocks, instead of being written into the server. The command names under
``toolchain.commands`` ARE the vocabulary: a fork adds a command by adding one entry there and
changing no Python. Nothing in the vocabulary is caller-supplied: it is composed once, at import
time, from the config alone.

Each command declares its mode with ``writes``. The vocabulary is split on that boolean into two
disjoint tables: ``GATES``, the check-mode commands ``run_gate`` may run, and ``FIX_COMMANDS``, the
write-mode commands ``run_fix`` may run. Neither tool can reach the other's table, so a write-mode
command can never be run by the check surface and a check-mode command can never be run by the fix
surface. ``COMMANDS`` holds both and is what ``list_gates`` publishes.

Each command may also declare an output-summary rule under ``summary``, read and normalised here
exactly as the guard is. The rule names what to count in the command's own output, so a gate whose
numbers exist only inside a six-figure payload can report them; its patterns are compiled at import
time, and a command that declares no rule carries ``summary`` None, which is most of them.

The loader is stdlib only and falls back to its own embedded defaults, which are this repository's
values, so the server behaves exactly as it did before the config existed when the config file is
absent, unreadable or not valid JSON.

``print_config_if_requested()`` answers the server's ``--print-config`` flag: a JSON object of
exactly the config keys this vocabulary consumes, sorted, with no command run and no journal read.
"""

from __future__ import annotations

import json
import os
import re
import sys
from pathlib import Path
from typing import Any

# --- The loader, found from this file --------------------------------------------------------
_SCRIPTS = next(
    (
        parent / "scripts"
        for parent in Path(__file__).resolve().parents
        if (parent / "scripts" / "agentic_config.py").is_file()
    ),
    Path(__file__).resolve().parents[2] / "scripts",
)
if str(_SCRIPTS) not in sys.path:
    sys.path.insert(0, str(_SCRIPTS))
import agentic_config  # noqa: E402 - the loader sits in the path inserted above

# What this vocabulary consumes. `--print-config` prints exactly these keys, sorted. The `writes`
# key of every command is consumed too: it is what splits the vocabulary into the check-mode and
# write-mode tables below, so a fork that changes one changes which tool may run that command.
CONFIG_KEYS: tuple[str, ...] = (
    "toolchain.commands.test.argv",
    "toolchain.commands.test.description",
    "toolchain.commands.test.summary",
    "toolchain.commands.test.writes",
    "toolchain.commands.clippy.argv",
    "toolchain.commands.clippy.description",
    "toolchain.commands.clippy.guard.marker",
    "toolchain.commands.clippy.guard.marker_regex",
    "toolchain.commands.clippy.guard.touch_file",
    "toolchain.commands.clippy.guard.reason",
    "toolchain.commands.clippy.summary",
    "toolchain.commands.clippy.writes",
    "toolchain.commands.fmt.argv",
    "toolchain.commands.fmt.description",
    "toolchain.commands.fmt.summary",
    "toolchain.commands.fmt.writes",
    "toolchain.commands.policy.argv",
    "toolchain.commands.policy.description",
    "toolchain.commands.policy.summary",
    "toolchain.commands.policy.writes",
    "toolchain.commands.conformance.argv",
    "toolchain.commands.conformance.description",
    "toolchain.commands.conformance.summary",
    "toolchain.commands.conformance.writes",
    "toolchain.commands.fmt-fix.argv",
    "toolchain.commands.fmt-fix.description",
    "toolchain.commands.fmt-fix.summary",
    "toolchain.commands.fmt-fix.writes",
    "toolchain.commands.webcheck.argv",
    "toolchain.commands.webcheck.description",
    "toolchain.commands.webcheck.summary",
    "toolchain.commands.webcheck.writes",
    "toolchain.commands.webtest.argv",
    "toolchain.commands.webtest.description",
    "toolchain.commands.webtest.summary",
    "toolchain.commands.webtest.writes",
    "containers.workspace",
    "containers.memory_dir",
    "containers.tools_image",
)

# --- Runtime paths: container defaults, every one overridable for a local run ----------------
WORKSPACE = os.getenv("GATE_WORKSPACE", str(agentic_config.get("containers.workspace")))
MEMORY_DIR = os.getenv("MEMORY_DIR", str(agentic_config.get("containers.memory_dir")))
AUDIT_PATH = os.getenv("GATE_AUDIT_PATH", str(Path(MEMORY_DIR) / "gate-audit.log"))
TOOLS_IMAGE = str(agentic_config.get("containers.tools_image"))

# The output-summary modes this vocabulary recognises. `count_matching_lines` reports how many
# lines the pattern matches; `count_unique_groups` reports how many distinct values its named
# group takes. A rule naming any other mode is dropped whole rather than partly honoured.
SUMMARY_MODES: frozenset[str] = frozenset({"count_matching_lines", "count_unique_groups"})
SUMMARY_STREAMS: tuple[str, ...] = ("stdout", "stderr")


def _writes(command: dict[str, Any], embedded: dict[str, Any]) -> bool:
    """Whether a command rewrites files, as a strict bool: the config, the embedded default, False.

    Only a real ``True`` makes a command write-mode. A truthy string, a 1 or a missing key all read
    as check mode, because the consequence of reading a check-mode command as write-mode -- or the
    reverse -- is a command reaching the wrong tool. Check mode is the safe default and is never
    inferred from anything but a boolean.
    """
    for source in (command, embedded):
        declared = source.get("writes")
        if isinstance(declared, bool):
            return declared
    return False


def _summary(command: dict[str, Any], embedded: dict[str, Any]) -> dict[str, Any] | None:
    """One command's output-summary rule, normalised, or None when it declares none.

    Read exactly as ``guard`` is read: the config's rule when it is well formed, the embedded
    default's when the config leaves it out, and None otherwise. None is the ordinary case --
    seven of this repository's eight commands declare ``"summary": null`` -- so a command with no
    rule is not an error and never becomes one.

    A rule is taken whole or not at all. An unrecognised mode, a missing or empty pattern, a
    pattern that will not compile, and a ``count_unique_groups`` rule naming a group its own
    pattern does not define each drop the whole summary to None, because a partly honoured rule
    would report a number nobody declared. Every pattern is compiled here, at import time, so a
    pattern a fork breaks is found at startup rather than inside a gate run.

    The returned rule is JSON-safe: it carries the pattern as the string the config wrote, and the
    compiled objects live in ``SUMMARY_PATTERNS`` below, beside ``GUARD_MARKER_PATTERN``.
    """
    declared = command.get("summary")
    if not isinstance(declared, dict):
        declared = embedded.get("summary")
    if not isinstance(declared, dict):
        return None
    counts = declared.get("counts")
    if not isinstance(counts, dict) or not counts:
        return None
    normalised: dict[str, dict[str, Any]] = {}
    for label, rule in counts.items():
        if not isinstance(rule, dict):
            return None
        mode = rule.get("mode")
        pattern = rule.get("pattern")
        if mode not in SUMMARY_MODES or not isinstance(pattern, str) or not pattern:
            return None
        try:
            compiled = re.compile(pattern)
        except re.error:  # a fork's pattern that will not compile: the rule is dropped whole
            return None
        group = rule.get("group")
        if mode == "count_unique_groups":
            if not isinstance(group, str) or group not in compiled.groupindex:
                return None
        else:
            group = None
        normalised[str(label)] = {"mode": str(mode), "pattern": pattern, "group": group}
    declared_streams = declared.get("streams")
    if not isinstance(declared_streams, (list, tuple)):
        declared_streams = SUMMARY_STREAMS
    streams = tuple(name for name in SUMMARY_STREAMS if name in declared_streams)
    strip = declared.get("strip_ansi")
    return {
        "reason": str(declared.get("reason") or ""),
        "streams": streams or SUMMARY_STREAMS,
        "strip_ansi": strip if isinstance(strip, bool) else True,
        "counts": normalised,
    }


def _gate(name: str) -> dict[str, Any]:
    """One command's argv, description, guard, summary and mode, from ``toolchain.commands.<name>``.

    The embedded defaults answer for anything the config leaves out, so a missing config, a missing
    key and a complete config all yield a usable command. A command the config names but gives no
    argv to is reported as the config error it is: an empty argv would be a command that runs
    nothing, which is the failure this table exists to make unrepresentable. ``argv`` is a tuple of
    strings, ``guard`` is None or exactly the three fields the server's cache-hit guard reads,
    ``summary`` is None or exactly the count rules the server's output summary reads, and ``writes``
    is the strict bool that decides which of the two tables below the command lands in, whatever
    else the config carries beside them.
    """
    embedded = agentic_config.DEFAULT["toolchain"]["commands"].get(name, {})
    command = agentic_config.get(f"toolchain.commands.{name}")
    if not isinstance(command, dict):
        command = embedded
    argv = command.get("argv")
    if not isinstance(argv, (list, tuple)) or not argv:
        argv = embedded.get("argv")
    if not isinstance(argv, (list, tuple)) or not argv:
        raise ValueError(
            f"gate '{name}' names no argv: give it one under "
            f"toolchain.commands.{name}.argv in agentic.config.json"
        )
    guard = command.get("guard")
    if isinstance(guard, dict) and guard.get("marker"):
        guard = {field: str(guard.get(field, "")) for field in ("marker", "touch_file", "reason")}
    else:
        guard = None
    return {
        "argv": tuple(str(element) for element in argv),
        "description": str(command.get("description") or embedded.get("description", "")),
        "guard": guard,
        "summary": _summary(command, embedded),
        "writes": _writes(command, embedded),
    }


def _command_names() -> tuple[str, ...]:
    """The command names, in the order the config lists them under ``toolchain.commands``.

    The command names are the vocabulary, so a fork adds or removes a command by editing the config
    and nothing else. A config that names no command falls back to the embedded defaults, which are
    this repository's eight names: seven check-mode and one write-mode.
    """
    commands = agentic_config.get("toolchain.commands")
    if not isinstance(commands, dict) or not commands:
        commands = agentic_config.DEFAULT["toolchain"]["commands"]
    return tuple(str(name) for name in commands)


# --- Command vocabulary: the whole execution surface of the gate server ----------------------
# The argv tuples below are the only commands the server can ever run. Nothing else is composed,
# interpolated or appended at call time, and the caller contributes no element of any argv. The
# names are the config's `toolchain.commands` keys, so the table below is built rather than written.
#
# `COMMANDS` is every command, each carrying its `writes` mode, and is what `list_gates` publishes.
# `GATES` and `FIX_COMMANDS` partition it on that mode and are disjoint by construction: `run_gate`
# resolves a name against `GATES` alone and `run_fix` against `FIX_COMMANDS` alone, so neither tool
# can name a command belonging to the other. `GATE_NAMES` stays the names `run_gate` accepts.
COMMAND_NAMES: tuple[str, ...] = _command_names()
COMMANDS: dict[str, dict[str, Any]] = {name: _gate(name) for name in COMMAND_NAMES}
GATES: dict[str, dict[str, Any]] = {
    name: definition for name, definition in COMMANDS.items() if not definition["writes"]
}
FIX_COMMANDS: dict[str, dict[str, Any]] = {
    name: definition for name, definition in COMMANDS.items() if definition["writes"]
}
GATE_NAMES: tuple[str, ...] = tuple(GATES)
FIX_COMMAND_NAMES: tuple[str, ...] = tuple(FIX_COMMANDS)

# The clippy cache-hit guard's marker pattern. cargo honours CARGO_TERM_COLOR=always in the sandbox
# image even when stderr is a pipe, so the server matches this pattern against output with the SGR
# escapes stripped; a fork that renames the crate names its own crate here.
try:
    GUARD_MARKER_PATTERN = re.compile(
        str(agentic_config.get("toolchain.commands.clippy.guard.marker_regex"))
    )
except re.error:  # a fork's pattern that will not compile: the embedded default answers
    GUARD_MARKER_PATTERN = re.compile(
        str(agentic_config.DEFAULT["toolchain"]["commands"]["clippy"]["guard"]["marker_regex"])
    )

# The compiled output-summary patterns, one sub-table per command that declares a rule. `_summary`
# already compiled each of these once to validate it, so nothing here can raise: a pattern that
# would not compile left its command's summary None and kept the command out of this table.
SUMMARY_PATTERNS: dict[str, dict[str, re.Pattern[str]]] = {
    name: {
        label: re.compile(rule["pattern"])
        for label, rule in definition["summary"]["counts"].items()
    }
    for name, definition in COMMANDS.items()
    if definition["summary"]
}


def print_config_if_requested(argv: list[str]) -> bool:
    """Print what this vocabulary consumes when ``argv`` asks for it, and say whether it did."""
    if "--print-config" not in list(argv):
        return False
    print(
        json.dumps({key: agentic_config.get(key) for key in CONFIG_KEYS}, indent=2, sort_keys=True)
    )
    return True
