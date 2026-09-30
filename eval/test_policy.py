"""Does the governance policy still describe the artifacts that enforce it?

Protects the agreement between `docs/governance-policy.md` and the artifacts that carry its
rules in a form a program can read: the storage and retrieval allow-lists, the routing map
(`docs/routing-and-tool-grant-map.md` and its `.json`), the seven role definitions under
`.claude/agents/`, the skill files under `.claude/skills/`, and the per-role mount table in
`scripts/run-agent.sh`.

Every check compares values, never file presence: a grant the policy states and an artifact
omits, an artifact grant the policy never made, an autonomy level that differs between two
artifacts, a `**Container permissions**` line that disagrees with the launcher, a near-miss
the policy cites and the calibration log does not define, or a tool both granted and denied
to one role. Every failure names the role, both artifacts and both values.

Run from the repository root:

    pytest eval/test_policy.py -v

Parsing rules this file relies on. They mirror the layout of the seven policy entries.

* A role entry is a `## Role N — \\`role\\`` heading. Its four dimensions are the `###`
  sections `MCP server and operation access`, `Skill activation scope`,
  `Data classification ceiling` and `Autonomy level`, closed by a `**Container
  permissions**` paragraph.
* A tool token is a backticked `mcp__<server>__<tool>` name or a harness name (`Task`,
  `Read`, `Write`, `Edit`, `Bash`). A token is granted when a grant/permit word is the
  nearest keyword before it, and denied when a deny word precedes it or follows it
  directly. `Deny every storage operation` denies every operation the server exposes.
* The launcher profile is the `case` block in `role_profile()`: one
  `role) ROLE_WS=.. ; ROLE_MEM=.. ; ROLE_TARGET=..` line per role, where `none` means
  "no separate mount". The workspace mode the policy states is the operative clause of its
  Container permissions paragraph (`writes no file there` denies a workspace write).
* A cited literal is compared after backticks, bold markers and whitespace runs are
  normalised away, and must sit within three lines of the cited line.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path
from typing import Any

# --- The portability seam: the artifact paths live in agentic.config.json ----------------------
# Every file this suite compares is named once, in the config's `artifacts` block, so a fork points
# the suite at its own tree by editing the config. The loader is stdlib only and falls back to its
# own embedded defaults, so the suite checks exactly what it checks today when the config file is
# absent. The flag below answers before pytest is imported, so it works on a host without pytest.
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

CONFIG_KEYS: tuple[str, ...] = (
    "artifacts.policy_document",
    "artifacts.grant_map_md",
    "artifacts.grant_map_json",
    "artifacts.calibration_log",
    "artifacts.storage_allow_list",
    "artifacts.retrieval_allow_list",
    "artifacts.storage_server",
    "artifacts.retrieval_server",
    "artifacts.gate_server",
    "artifacts.coursetools_server",
    "artifacts.launcher",
    "artifacts.definitions_dir",
    "artifacts.skills_dir",
)

if "--print-config" in sys.argv[1:]:
    print(
        json.dumps(
            {key: agentic_config.get(key) for key in CONFIG_KEYS}, indent=2, sort_keys=True
        )
    )
    raise SystemExit(0)

import pytest  # noqa: E402 - imported after the flag above, which must work on a host without it

REPO = Path(__file__).resolve().parent.parent


def _artifact(key: str) -> Path:
    """One artifact path from the config, resolved against the repository root."""
    return REPO / str(agentic_config.get(f"artifacts.{key}"))


POLICY_REL = str(agentic_config.get("artifacts.policy_document"))
SKILLS_REL = str(agentic_config.get("artifacts.skills_dir"))

POLICY = _artifact("policy_document")
ROUTING_MD = _artifact("grant_map_md")
ROUTING_JSON = _artifact("grant_map_json")
CALIBRATION = _artifact("calibration_log")
STORAGE_ALLOW = _artifact("storage_allow_list")
RETRIEVAL_ALLOW = _artifact("retrieval_allow_list")
STORAGE_SERVER = _artifact("storage_server")
RETRIEVAL_SERVER = _artifact("retrieval_server")
GATE_SERVER = _artifact("gate_server")
# The course-tools server is the one server outside the governed three, and it is named in the
# reports this suite emits. The config names it like the rest — `agentic.config.json:166`
# `"coursetools_server": "mcp/coursetools_server.py"` — so it resolves through `_artifact` here
# rather than through a literal path.
COURSETOOLS_SERVER = _artifact("coursetools_server")
LAUNCHER = _artifact("launcher")
AGENTS_DIR = _artifact("definitions_dir")
SKILLS_DIR = _artifact("skills_dir")

ROLES = (
    "orchestrator",
    "planner",
    "implementer",
    "tester",
    "reviewer",
    "project-manager",
    "researcher",
)

DIMENSIONS = (
    "MCP server and operation access",
    "Skill activation scope",
    "Data classification ceiling",
    "Autonomy level",
)

MODE_WORDS = {"rw": "read-write", "ro": "read-only", "none": "absent", "": "absent"}


# --------------------------------------------------------------------------- reading


def _read(path: Path) -> str:
    assert path.is_file(), (
        f"missing artifact {path.relative_to(REPO)}: the policy states a rule about this "
        "role, and the file that would settle it does not exist"
    )
    text = path.read_text(encoding="utf-8")
    assert text.strip(), f"artifact {path.relative_to(REPO)} is empty, so it settles nothing"
    return text


def _norm(text: str) -> str:
    """Drop markup and collapse whitespace, so a wrapped literal still matches."""
    text = text.replace("`", "").replace("**", "").replace("*", "")
    return re.sub(r"\s+", " ", text).strip()


def _norm_with_map(text: str) -> tuple[str, list[int]]:
    """Normalised text plus, per character, its offset in the original."""
    out: list[str] = []
    offsets: list[int] = []
    prev_space = False
    for index, char in enumerate(text):
        if char in "`*":
            continue
        if char.isspace():
            if prev_space:
                continue
            out.append(" ")
            offsets.append(index)
            prev_space = True
            continue
        out.append(char)
        offsets.append(index)
        prev_space = False
    return "".join(out), offsets


def _rel(path: Path) -> str:
    return str(path.relative_to(REPO))


def _mismatch(
    role: str,
    policy_artifact: str,
    policy_value: object,
    other_artifact: str,
    other_value: object,
    note: str = "",
) -> str:
    """One failure message naming the role, both artifacts and both values."""
    lines = [
        f"ROLE `{role}` disagrees with an artifact.",
        f"  policy claim : {policy_artifact}",
        f"                 -> {policy_value}",
        f"  artifact     : {other_artifact}",
        f"                 -> {other_value}",
    ]
    if note:
        lines.append(f"  fix one side : {note}")
    return "\n".join(lines)


def _setnote(left: set, right: set) -> str:
    """Which values are on one side only, so the reader knows what to change."""
    only_left = sorted(left - right)
    only_right = sorted(right - left)
    if not only_left and not only_right:
        return ""
    return f"policy-only={only_left} artifact-only={only_right}"


# --------------------------------------------------------------------------- policy


_ROLE_HEADING = re.compile(r"^## Role (\d+) — `([a-z][a-z0-9-]*)`\s*$", re.M)
_KEYWORD = re.compile(
    r"\b(Grant|grants|granted|Deny|denies|denied|Permit|permits|permitted)\b", re.I
)
_TOOL_TOKEN = re.compile(r"`(mcp__[a-z0-9]+__[a-z0-9_]+|Task|Read|Write|Edit|Bash|Grep|Glob)`")
_DENY_AFTER = re.compile(
    r"^\s*(?:keeps?\s+|stays?\s+|remains?\s+)?(denied|denies|deny|refused|not permitted)\b",
    re.I,
)


def _policy_text() -> str:
    return _read(POLICY)


def policy_sections() -> dict[str, str]:
    """The seven role entries, keyed by role name."""
    text = _policy_text()
    heads = list(_ROLE_HEADING.finditer(text))
    assert heads, (
        f"{_rel(POLICY)} carries no `## Role N — `role`` heading, so no role entry can be read"
    )
    sections: dict[str, str] = {}
    for index, head in enumerate(heads):
        end = heads[index + 1].start() if index + 1 < len(heads) else len(text)
        sections[head.group(2)] = text[head.end() : end]
    return sections


def _subsection(section: str, title: str) -> str:
    match = re.search(rf"^### {re.escape(title)}\s*$", section, re.M)
    assert match, (
        f"a policy role entry has no `### {title}` section: the four dimensions are how a "
        "reader answers a permissions question, so the entry is not observable"
    )
    rest = section[match.end() :]
    nxt = re.search(r"^### |^## ", rest, re.M)
    return rest if not nxt else rest[: nxt.start()]


def _bullets(block: str) -> list[str]:
    bullets: list[str] = []
    current: str | None = None
    for line in block.splitlines():
        if line.startswith("- "):
            if current:
                bullets.append(current)
            current = line[2:].strip()
        elif current is not None and line.startswith("  ") and line.strip():
            current = f"{current} {line.strip()}"
        elif not line.strip() and current:
            bullets.append(current)
            current = None
    if current:
        bullets.append(current)
    return bullets


def _classify(bullet: str) -> tuple[set[str], set[str]]:
    """Attribute every tool token in one bullet to the nearest grant or deny keyword."""
    keywords = [
        (m.start(), "grant" if m.group(0).lower()[:4] in ("gran", "hold", "perm") else "deny")
        for m in _KEYWORD.finditer(bullet)
    ]
    grants: set[str] = set()
    denials: set[str] = set()
    for token in _TOOL_TOKEN.finditer(bullet):
        verdict: str | None = None
        if _DENY_AFTER.match(bullet[token.end() : token.end() + 40]):
            verdict = "deny"
        if verdict is None:
            prior = [kind for pos, kind in keywords if pos < token.start()]
            verdict = prior[-1] if prior else "grant"
        (grants if verdict == "grant" else denials).add(token.group(1))
    return grants, denials


def policy_mcp(role: str) -> tuple[set[str], set[str]]:
    """The MCP tools the policy grants and denies this role."""
    section = policy_sections()[role]
    block = _subsection(section, "MCP server and operation access")
    grants: set[str] = set()
    denials: set[str] = set()
    for bullet in _bullets(block):
        if re.search(r"\bDeny every storage operation\b", bullet):
            denials |= set(server_operations()["storage"])
        if re.search(r"\bHold no MCP operation\b", bullet):
            continue
        gained, lost = _classify(bullet)
        grants |= gained
        denials |= lost
    return grants, denials


def policy_skills(role: str) -> tuple[set[str], bool]:
    """Skills the policy permits this role, plus its `Deny every other skill` claim."""
    block = _subsection(policy_sections()[role], "Skill activation scope")
    permitted = set(re.findall(r"\bPermit `([a-z0-9][a-z0-9-]*)`", block))
    denies_others = bool(re.search(r"Deny every other skill", block))
    return permitted, denies_others


def policy_ceiling(role: str) -> str:
    """The retrieval ceiling the policy states for this role (`none` when it holds none)."""
    block = _subsection(policy_sections()[role], "Data classification ceiling")
    if re.search(r"Hold no retrieval ceiling", block):
        return "none"
    match = re.search(r"Cap retrieval at `([a-z]+)`", block)
    assert match, (
        f"the policy entry for `{role}` states no retrieval ceiling, so the artifact that "
        "caps retrieval cannot be checked against it"
    )
    return match.group(1)


def policy_autonomy(role: str) -> str:
    """The autonomy level the policy states for this role."""
    block = _subsection(policy_sections()[role], "Autonomy level")
    match = re.search(r"Hold `([a-z]+)` autonomy", block)
    assert match, (
        f"the policy entry for `{role}` states no autonomy level, so a level that drifts "
        "between the policy and the definition cannot be detected"
    )
    return match.group(1)


def policy_container(role: str) -> dict[str, Any]:
    """What this role's Container permissions paragraph claims about its container.

    A paragraph may state the workspace and memory mode two ways: as an explicit mount
    sentence naming the mode and the launcher, or as the operative clause that says what
    the role may write. Both are read, so a paragraph that states two different modes for
    one path fails the check instead of passing on whichever clause is read first.
    """
    section = policy_sections()[role]
    match = re.search(
        r"^\*\*Container permissions\*\*(?P<text>.*?)(?=\n## |\Z)", section, re.M | re.S
    )
    assert match, (
        f"the policy entry for `{role}` carries no `**Container permissions**` line, so the "
        "launcher's mount for it has nothing to be checked against"
    )
    paragraph = re.sub(r"\s+", " ", match.group("text")).strip()

    workspace: set[str] = set()
    explicit_ws = re.search(
        r"mounts this repository (?P<mode>read-write|read-only) at `/workspace`", paragraph
    )
    if explicit_ws:
        workspace.add(explicit_ws.group("mode"))
    if re.search(
        r"writes no file there|writes neither|reads no file in `/workspace`", paragraph
    ):
        workspace.add("read-only")
    if re.search(
        r"(?:reads and writes|writes) (?:orchestration documents|the files[^.]*?) under "
        r"`/workspace`",
        paragraph,
    ):
        workspace.add("read-write")

    memory: set[str] = set()
    explicit_mem = re.search(
        r"mounts `/workspace/\.memory` (?P<mode>read-write|read-only)", paragraph
    )
    if explicit_mem:
        memory.add(explicit_mem.group("mode"))
    if re.search(r"mounts no `/workspace/\.memory`", paragraph):
        memory.add("absent")
    if not memory:
        if re.search(r"writes and revises its own entries in `/workspace/\.memory`", paragraph):
            memory.add("read-write")
        elif re.search(r"holds no `/workspace/\.memory` operation", paragraph):
            memory.add("read-only")
        elif re.search(r"writes (?:one [^.]*|and revises[^.]*) into `/workspace/\.memory`", paragraph):
            memory.add("read-write")

    return {"paragraph": paragraph, "workspace": workspace, "memory": memory}


def policy_near_misses() -> set[str]:
    return set(re.findall(r"\bNM-\d+\b", _policy_text()))


# --------------------------------------------------------------------------- artifacts


def _load_json(path: Path) -> dict:
    text = _read(path)
    try:
        return json.loads(text)
    except json.JSONDecodeError as error:  # pragma: no cover - a broken artifact is the finding
        pytest.fail(f"{_rel(path)} is not valid JSON: {error}")


def storage_allow() -> dict:
    return _load_json(STORAGE_ALLOW)


def retrieval_allow() -> dict:
    return _load_json(RETRIEVAL_ALLOW)


def routing_map() -> dict:
    return _load_json(ROUTING_JSON)


def map_grants(role: str) -> set[str]:
    return set(routing_map()["grants"].get(role, []))


def map_ceiling(role: str) -> str:
    return routing_map()["retrieval_ceiling"].get(role, "")


def server_operations() -> dict[str, set[str]]:
    """Every operation each MCP server exposes, read from the server source."""
    operations: dict[str, set[str]] = {}
    storage_source = _read(STORAGE_SERVER)
    match = re.search(r"^OPERATIONS = \((?P<ops>.*?)\)", storage_source, re.M | re.S)
    assert match, f"{_rel(STORAGE_SERVER)} no longer declares an OPERATIONS tuple"
    operations["storage"] = set(re.findall(r"\"([a-z_]+)\"", match.group("ops")))
    for server, path in (
        ("retrieval", RETRIEVAL_SERVER),
        ("gate", GATE_SERVER),
        ("coursetools", COURSETOOLS_SERVER),
    ):
        operations[server] = set(
            re.findall(r"^@mcp\.tool(?:\(\))?\s*\n(?:async )?def ([a-z_]+)\(", _read(path), re.M)
        )
    assert operations["retrieval"], f"{_rel(RETRIEVAL_SERVER)} exposes no @mcp.tool operation"
    return operations


def definition(role: str) -> dict:
    """One role definition's frontmatter grants, denials and autonomy, plus its tool table."""
    path = AGENTS_DIR / f"{role}.md"
    text = _read(path)
    match = re.match(r"^---\n(?P<front>.*?)\n---\n", text, re.S)
    assert match, (
        f"{_rel(path)} carries no YAML frontmatter, so its tools, disallowedTools and "
        "autonomy cannot be read"
    )
    front, body = match.group("front"), text[match.end() :]
    tools: list[str] = []
    disallowed: list[str] = []
    autonomy: str | None = None
    current: str | None = None
    for line in front.splitlines():
        if re.match(r"^\s+-\s", line) and current in ("tools", "disallowedTools"):
            target = tools if current == "tools" else disallowed
            target.append(line.strip()[2:].strip())
            continue
        field = re.match(r"^([A-Za-z][A-Za-z0-9_]*):\s*(.*)$", line)
        if not field:
            current = None
            continue
        key, value = field.group(1), field.group(2).strip()
        if key in ("tools", "disallowedTools"):
            current = key
            if value:
                (tools if key == "tools" else disallowed).extend(
                    item.strip() for item in value.split(",") if item.strip()
                )
        else:
            current = None
            if key == "autonomy":
                autonomy = value
    table_grants: set[str] = set()
    table_denials: set[str] = set()
    for line in body.splitlines():
        if not line.startswith("|"):
            continue
        cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
        if len(cells) < 2:
            continue
        names = re.findall(r"`([^`]+)`", cells[0])
        verdict = re.sub(r"[^A-Za-z]", "", cells[1]).lower()
        if not names or verdict not in ("yes", "no"):
            continue
        (table_grants if verdict == "yes" else table_denials).update(names)
    assert autonomy, (
        f"{_rel(path)} declares no `autonomy:` field, so an autonomy that drifts from the "
        "policy cannot be detected"
    )
    return {
        "path": path,
        "tools": tools,
        "disallowedTools": disallowed,
        "autonomy": autonomy,
        "table_grants": table_grants,
        "table_denials": table_denials,
    }


def launcher_profiles() -> dict[str, dict[str, str]]:
    """The per-role mount profile in the launcher's `role_profile()` case block."""
    text = _read(LAUNCHER)
    block = re.search(r"role_profile\(\)\s*\{(?P<body>.*?)\n\}", text, re.S)
    assert block, (
        f"{_rel(LAUNCHER)} carries no `role_profile()` case block, so the per-role mounts "
        "the policy's Container permissions lines describe cannot be read"
    )
    profiles: dict[str, dict[str, str]] = {}
    for match in re.finditer(
        r"^\s*(?P<role>[a-z][a-z0-9-]*)\)\s*ROLE_WS=(?P<ws>\w+);\s*ROLE_MEM=(?P<mem>\w+)\s*;"
        r"\s*ROLE_TARGET=(?P<target>\w+)",
        block.group("body"),
        re.M,
    ):
        profiles[match.group("role")] = {
            "ws": match.group("ws"),
            "mem": match.group("mem"),
            "target": match.group("target"),
        }
    assert sorted(profiles) == sorted(ROLES), (
        f"{_rel(LAUNCHER)} defines a profile for {sorted(profiles)} while the policy "
        f"governs {sorted(ROLES)}"
    )
    return profiles


def _cell_mode(cell: str) -> str:
    """The mount mode a matrix cell states, as `read-write`, `read-only` or `absent`."""
    text = _norm(cell)
    if text.startswith("not mounted") or text.startswith("no "):
        return "absent"
    for mode in ("read-write", "read-only"):
        if mode in text:
            return mode
    return ""


def launcher_matrix() -> dict[str, dict[str, str]]:
    """The rows of the launcher's own `--matrix` table, with the policy line each cites."""
    text = _read(LAUNCHER)
    rows: dict[str, dict[str, str]] = {}
    for line in text.splitlines():
        match = re.match(
            r"^\| `(?P<role>[a-z][a-z0-9-]*)` \| (?P<ws>[^|]+)\| (?P<mem>[^|]+)\|", line
        )
        if not match:
            continue
        cited = re.search(re.escape(POLICY_REL) + r":(?P<line>\d+)", line)
        rows[match.group("role")] = {
            "ws": _cell_mode(match.group("ws")),
            "mem": _cell_mode(match.group("mem")),
            "cited_line": cited.group("line") if cited else "",
        }
    return rows


def map_md_role_cell(role: str) -> str:
    """The routing map's role-table row for one role."""
    for line in _read(ROUTING_MD).splitlines():
        if re.match(rf"^\| `{re.escape(role)}`( \(stretch, optional\))? \|", line):
            return line
    pytest.fail(f"{_rel(ROUTING_MD)} carries no role-table row for `{role}`")


def map_md_autonomy(role: str) -> str:
    cells = [cell.strip() for cell in map_md_role_cell(role).strip().strip("|").split("|")]
    return cells[-1].split("—")[0].strip().lower()


def map_md_storage_grants(role: str) -> set[str]:
    """The `granted` cells of the routing map's storage-operation table."""
    rows = [
        line
        for line in _read(ROUTING_MD).splitlines()
        if re.match(rf"^\| `{re.escape(role)}` \|", line)
    ]
    header = re.search(
        r"^\| Role \| `read_entry` \| `list_entries` \| `write_entry` \| `update_entry` \| "
        r"`delete_entry` \|",
        _read(ROUTING_MD),
        re.M,
    )
    assert header, f"{_rel(ROUTING_MD)} carries no storage-operation table"
    for row in rows:
        cells = [cell.strip() for cell in row.strip().strip("|").split("|")]
        if len(cells) != 6:
            continue
        if all(cell in ("granted", "denied") for cell in cells[1:]):
            return {
                operation
                for operation, cell in zip(
                    ("read_entry", "list_entries", "write_entry", "update_entry", "delete_entry"),
                    cells[1:],
                )
                if cell == "granted"
            }
    pytest.fail(
        f"{_rel(ROUTING_MD)} storage table has no granted/denied row for `{role}`, so the "
        "policy's storage grant for it has nothing to be checked against"
    )


def map_md_tool_table(role: str) -> tuple[set[str], set[str]]:
    """The routing map's role-table grant and denial cells for one role."""
    cells = [cell.strip() for cell in map_md_role_cell(role).strip().strip("|").split("|")]
    granted = set(re.findall(r"`(mcp__[a-z0-9]+__[a-z0-9_]+)`", cells[3]))
    denied = set(re.findall(r"`(mcp__[a-z0-9]+__[a-z0-9_]+)`", cells[4]))
    return granted, denied


def map_md_ceiling(role: str) -> str:
    text = _read(ROUTING_MD)
    assert re.search(r"^## Retrieval operation grants\s*$", text, re.M), (
        f"{_rel(ROUTING_MD)} carries no `## Retrieval operation grants` section"
    )
    section = text.split("## Retrieval operation grants", 1)[1]
    for line in section.splitlines():
        if re.match(rf"^\| `{re.escape(role)}` \|", line):
            cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
            return cells[2].strip("`")
    pytest.fail(f"{re.escape(role)} has no retrieval row in {_rel(ROUTING_MD)}")


def calibration_near_misses() -> set[str]:
    return set(re.findall(r"^### (NM-\d+) —", _read(CALIBRATION), re.M))


def skill_files() -> dict[str, dict]:
    """Every skill the repository ships, with any Activation Scope it declares."""
    skills: dict[str, dict] = {}
    if not SKILLS_DIR.is_dir():
        return skills
    for path in sorted(SKILLS_DIR.glob("*/SKILL.md")):
        text = _read(path)
        name = re.search(r"^name:\s*([a-z0-9][a-z0-9-]*)\s*$", text, re.M)
        permitted: set[str] = set()
        denied: set[str] = set()
        scope = re.search(r"^#+ Activation Scope\s*$(?P<body>.*?)(?=\n#+ |\Z)", text, re.M | re.S)
        if scope:
            for line in scope.group("body").splitlines():
                if not line.strip():
                    continue
                roles = set(re.findall(r"\b((?:" + "|".join(ROLES) + r"))\b", line))
                if re.search(r"\b(permitted|allowed|granted)\b", line, re.I):
                    permitted |= roles
                elif re.search(r"\b(denied|forbidden|not permitted)\b", line, re.I):
                    denied |= roles
        skills[path.parent.name] = {
            "path": path,
            "name": name.group(1) if name else path.parent.name,
            "permitted": permitted,
            "denied": denied,
        }
    return skills


# --------------------------------------------------------------------------- the four checks


@pytest.mark.parametrize("role", ROLES)
def test_storage_allowlist_matches_policy(role: str) -> None:
    """The storage grants in the policy, the routing map and the allow-list are identical."""
    policy_ops, _ = policy_mcp(role)
    policy_storage = {tool.split("__")[2] for tool in policy_ops if tool.startswith("mcp__storage__")}
    allow_ops = set(storage_allow()["roles"].get(role, []))
    map_ops = {tool.split("__")[2] for tool in map_grants(role) if tool.startswith("mcp__storage__")}
    table_ops = map_md_storage_grants(role)

    assert allow_ops == policy_storage, _mismatch(
        role,
        f"{_rel(POLICY)} (MCP server and operation access)",
        sorted(policy_storage),
        f"{_rel(STORAGE_ALLOW)} roles.{role}",
        sorted(allow_ops),
        _setnote(policy_storage, allow_ops),
    )
    assert map_ops == policy_storage, _mismatch(
        role,
        f"{_rel(POLICY)} (MCP server and operation access)",
        sorted(policy_storage),
        f"{_rel(ROUTING_JSON)} grants.{role}",
        sorted(map_ops),
        _setnote(policy_storage, map_ops),
    )
    assert table_ops == policy_storage, _mismatch(
        role,
        f"{_rel(POLICY)} (MCP server and operation access)",
        sorted(policy_storage),
        f"{_rel(ROUTING_MD)} storage-operation table",
        sorted(table_ops),
        _setnote(policy_storage, table_ops),
    )


@pytest.mark.parametrize("role", ROLES)
def test_skill_scope_matches_policy(role: str) -> None:
    """Skill activation in the policy, the skill files and their Activation Scope agree."""
    skills = skill_files()
    permitted, denies_others = policy_skills(role)
    on_disk = set(skills)

    assert permitted <= on_disk, _mismatch(
        role,
        f"{_rel(POLICY)} (Skill activation scope)",
        sorted(permitted),
        f"{_rel(SKILLS_DIR)} (skills on disk)",
        sorted(on_disk),
        "the policy permits a skill the repository does not ship",
    )
    assert denies_others, _mismatch(
        role,
        f"{_rel(POLICY)} (Skill activation scope)",
        sorted(permitted),
        f"{_rel(SKILLS_DIR)} (skills on disk)",
        sorted(on_disk),
        "state `Deny every other skill`, so a newly added skill file cannot pass unnoticed",
    )

    claimed = set(
        re.findall(
            rf"find {re.escape(SKILLS_REL)} -name SKILL\.md`\s*->\s*`(\d+)`",
            policy_sections()[role],
        )
    )
    if claimed:
        assert {int(count) for count in claimed} == {len(skills)}, _mismatch(
            role,
            f"{_rel(POLICY)} (Skill activation scope)",
            f"`find {SKILLS_REL} -name SKILL.md` -> {sorted(int(c) for c in claimed)}",
            f"{_rel(SKILLS_DIR)}",
            f"ships {len(skills)} skill file(s): {sorted(on_disk)}",
            "the policy's stated reason for `Deny every other skill` no longer holds",
        )

    for name, skill in skills.items():
        if not skill["permitted"] and not skill["denied"]:
            continue
        if name in permitted:
            assert role not in skill["denied"], _mismatch(
                role,
                f"{_rel(POLICY)} (Skill activation scope)",
                f"permits `{name}`",
                f"{_rel(skill['path'])} (Activation Scope)",
                f"denies `{role}`",
            )
        else:
            assert role not in skill["permitted"], _mismatch(
                role,
                f"{_rel(POLICY)} (Skill activation scope)",
                f"does not permit `{name}`",
                f"{_rel(skill['path'])} (Activation Scope)",
                f"permits `{role}`",
            )


@pytest.mark.parametrize("role", ROLES)
def test_container_workspace_matches_policy(role: str) -> None:
    """The launcher's workspace and memory mounts match the role's Container permissions line."""
    container = policy_container(role)
    profile = launcher_profiles()[role]
    workspace = container["workspace"]
    memory = container["memory"]
    mounted_workspace = MODE_WORDS[profile["ws"]]
    mounted_memory = MODE_WORDS[profile["mem"]]

    assert workspace, _mismatch(
        role,
        f"{_rel(POLICY)} (Container permissions)",
        container["paragraph"],
        f"{_rel(LAUNCHER)} role_profile()",
        f"ROLE_WS={profile['ws']}",
        "the line states no workspace permission, so the mount cannot be checked against it",
    )
    assert workspace == {mounted_workspace}, _mismatch(
        role,
        f"{_rel(POLICY)} (Container permissions)",
        f"states {sorted(workspace)} for /workspace"
        + (" — two modes in one line" if len(workspace) > 1 else ""),
        f"{_rel(LAUNCHER)} role_profile() ({role})",
        f"ROLE_WS={profile['ws']} -> /workspace mounted {mounted_workspace}",
    )
    assert memory == {mounted_memory}, _mismatch(
        role,
        f"{_rel(POLICY)} (Container permissions)",
        f"states {sorted(memory) or ['nothing']} for /workspace/.memory"
        + (" — two modes in one line" if len(memory) > 1 else ""),
        f"{_rel(LAUNCHER)} role_profile() ({role})",
        f"ROLE_MEM={profile['mem']} -> /workspace/.memory {mounted_memory}",
    )


@pytest.mark.parametrize("role", ROLES)
def test_no_storage_overgrant(role: str) -> None:
    """No role holds a storage operation the policy never gave it."""
    policy_ops, policy_denials = policy_mcp(role)
    policy_storage = {
        tool.split("__")[2] for tool in policy_ops if tool.startswith("mcp__storage__")
    }
    policy_denied_storage = {
        tool.split("__")[2] for tool in policy_denials if tool.startswith("mcp__storage__")
    }
    allow_ops = set(storage_allow()["roles"].get(role, []))
    definition_tools = {
        tool.split("__")[2]
        for tool in definition(role)["tools"]
        if tool.startswith("mcp__storage__")
    }

    assert not allow_ops - policy_storage, _mismatch(
        role,
        f"{_rel(POLICY)} (MCP server and operation access)",
        f"grants {sorted(policy_storage)}, denies {sorted(policy_denied_storage)}",
        f"{_rel(STORAGE_ALLOW)} roles.{role}",
        f"{sorted(allow_ops)} (over-granted: {sorted(allow_ops - policy_storage)})",
        "remove the operation from the allow-list, or grant it in the policy with a reason",
    )
    assert not definition_tools - policy_storage, _mismatch(
        role,
        f"{_rel(POLICY)} (MCP server and operation access)",
        f"grants {sorted(policy_storage)}",
        f"{_rel(definition(role)['path'])} tools:",
        f"{sorted(definition_tools)} "
        f"(over-granted: {sorted(definition_tools - policy_storage)})",
        "a role definition must not hold an operation the policy never gave it",
    )
    assert not allow_ops & policy_denied_storage, _mismatch(
        role,
        f"{_rel(POLICY)} (MCP server and operation access)",
        f"denies {sorted(policy_denied_storage)}",
        f"{_rel(STORAGE_ALLOW)} roles.{role}",
        sorted(allow_ops),
        "the allow-list grants an operation the policy denies this role",
    )
    exposed = server_operations()["storage"]
    unknown = allow_ops - exposed
    assert not unknown, _mismatch(
        role,
        f"{_rel(STORAGE_SERVER)} OPERATIONS",
        sorted(exposed),
        f"{_rel(STORAGE_ALLOW)} roles.{role}",
        f"{sorted(allow_ops)} (unknown: {sorted(unknown)})",
    )
    if "delete_entry" in exposed:
        granting = sorted(
            other
            for other, ops in storage_allow()["roles"].items()
            if "delete_entry" in ops
        )
        assert not granting, _mismatch(
            role,
            f"{_rel(POLICY)} (Policy basis)",
            "`Grant mcp__storage__delete_entry to no role`",
            f"{_rel(STORAGE_ALLOW)} roles",
            f"delete_entry granted to {granting}",
        )


# --------------------------------------------------------------------------- the rest of the set


@pytest.mark.parametrize("role", ROLES)
def test_retrieval_allowlist_matches_policy(role: str) -> None:
    """The retrieval grant and its ceiling agree across the policy, the map and the allow-list."""
    policy_ops, _ = policy_mcp(role)
    holds_retrieve = "mcp__retrieval__retrieve" in policy_ops
    ceiling = policy_ceiling(role)
    allow_ops = set(retrieval_allow()["roles"].get(role, []))
    map_holds = "mcp__retrieval__retrieve" in map_grants(role)

    assert bool(allow_ops) == holds_retrieve, _mismatch(
        role,
        f"{_rel(POLICY)} (MCP server and operation access)",
        f"retrieve {'granted' if holds_retrieve else 'denied'}",
        f"{_rel(RETRIEVAL_ALLOW)} roles.{role}",
        sorted(allow_ops),
    )
    assert map_holds == holds_retrieve, _mismatch(
        role,
        f"{_rel(POLICY)} (MCP server and operation access)",
        f"retrieve {'granted' if holds_retrieve else 'denied'}",
        f"{_rel(ROUTING_JSON)} grants.{role}",
        f"retrieve {'granted' if map_holds else 'absent'}",
    )
    assert map_ceiling(role) == ceiling, _mismatch(
        role,
        f"{_rel(POLICY)} (Data classification ceiling)",
        ceiling,
        f"{_rel(ROUTING_JSON)} retrieval_ceiling.{role}",
        map_ceiling(role),
    )
    assert map_md_ceiling(role) == ceiling, _mismatch(
        role,
        f"{_rel(POLICY)} (Data classification ceiling)",
        ceiling,
        f"{_rel(ROUTING_MD)} retrieval table",
        map_md_ceiling(role),
    )
    if holds_retrieve:
        assert ceiling != "none", _mismatch(
            role,
            f"{_rel(RETRIEVAL_ALLOW)} derivation",
            "a role granted retrieve must carry a ceiling above none",
            f"{_rel(POLICY)} (Data classification ceiling)",
            f"ceiling {ceiling}",
        )


@pytest.mark.parametrize("role", ROLES)
def test_policy_grants_present_in_role_definitions(role: str) -> None:
    """Every tool the policy grants a role appears in that role's definition."""
    policy_grants, _ = policy_mcp(role)
    definition_tools = set(definition(role)["tools"])
    missing = sorted(tool for tool in policy_grants if tool not in definition_tools)
    assert not missing, _mismatch(
        role,
        f"{_rel(POLICY)} (MCP server and operation access)",
        f"grants {sorted(policy_grants)}",
        f"{_rel(definition(role)['path'])} tools:",
        f"{sorted(definition_tools)} (missing: {missing})",
        "add the grant to the definition, or withdraw it from the policy",
    )


@pytest.mark.parametrize("role", ROLES)
def test_policy_denials_absent_from_role_definitions(role: str) -> None:
    """No tool the policy denies a role is granted in that role's definition."""
    _, policy_denials = policy_mcp(role)
    definition_tools = set(definition(role)["tools"])
    leaked = sorted(tool for tool in policy_denials if tool in definition_tools)
    assert not leaked, _mismatch(
        role,
        f"{_rel(POLICY)} (MCP server and operation access)",
        f"denies {sorted(policy_denials)}",
        f"{_rel(definition(role)['path'])} tools:",
        f"{sorted(definition_tools)} (granted but denied: {leaked})",
    )
    map_grants_for_role = {tool.split("__")[2] for tool in map_grants(role)}
    map_denied = {
        tool.split("__")[2]
        for tool in map_md_tool_table(role)[1]
        if tool.startswith("mcp__")
    }
    conflicted = sorted(map_grants_for_role & map_denied)
    assert not conflicted, _mismatch(
        role,
        f"{_rel(ROUTING_MD)} role table (Tools denied)",
        sorted(map_denied),
        f"{_rel(ROUTING_JSON)} grants.{role}",
        f"{sorted(map_grants_for_role)} (both granted and denied: {conflicted})",
    )


@pytest.mark.parametrize("role", ROLES)
def test_no_tool_both_granted_and_denied(role: str) -> None:
    """No artifact grants and denies one role the same tool."""
    policy_grants, policy_denials = policy_mcp(role)
    both_policy = sorted(policy_grants & policy_denials)
    assert not both_policy, _mismatch(
        role,
        f"{_rel(POLICY)} (MCP server and operation access)",
        f"grants {sorted(policy_grants)}",
        f"{_rel(POLICY)} (MCP server and operation access)",
        f"denies {sorted(policy_denials)} (both: {both_policy})",
        "decide which side the role holds and delete the other bullet",
    )

    role_definition = definition(role)
    both_definition = sorted(set(role_definition["tools"]) & set(role_definition["disallowedTools"]))
    assert not both_definition, _mismatch(
        role,
        f"{_rel(role_definition['path'])} tools:",
        sorted(role_definition["tools"]),
        f"{_rel(role_definition['path'])} disallowedTools:",
        f"{sorted(role_definition['disallowedTools'])} (both: {both_definition})",
    )

    both_table = sorted(role_definition["table_grants"] & role_definition["table_denials"])
    assert not both_table, _mismatch(
        role,
        f"{_rel(role_definition['path'])} tool table (Granted)",
        sorted(role_definition["table_grants"]),
        f"{_rel(role_definition['path'])} tool table (Denied)",
        f"{sorted(role_definition['table_denials'])} (both: {both_table})",
    )


@pytest.mark.parametrize("role", ROLES)
def test_autonomy_matches_definitions(role: str) -> None:
    """The autonomy level is the same in the policy, the definition and the routing map."""
    policy_level = policy_autonomy(role)
    definition_level = definition(role)["autonomy"].strip().lower()
    map_level = map_md_autonomy(role)

    assert definition_level == policy_level, _mismatch(
        role,
        f"{_rel(POLICY)} (Autonomy level)",
        policy_level,
        f"{_rel(definition(role)['path'])} autonomy:",
        definition_level,
        "change the definition or the policy entry, never both",
    )
    assert map_level == policy_level, _mismatch(
        role,
        f"{_rel(POLICY)} (Autonomy level)",
        policy_level,
        f"{_rel(ROUTING_MD)} role table (Autonomy)",
        map_level,
    )


@pytest.mark.parametrize("role", ROLES)
def test_definition_tool_table_does_not_contradict_frontmatter(role: str) -> None:
    """A definition's tool table marks no tool against its own frontmatter."""
    item = definition(role)
    assert not item["table_grants"] - set(item["tools"]), _mismatch(
        role,
        f"{_rel(item['path'])} tools:",
        sorted(item["tools"]),
        f"{_rel(item['path'])} tool table",
        f"marks {sorted(item['table_grants'])} granted",
    )
    assert not item["table_denials"] & set(item["tools"]), _mismatch(
        role,
        f"{_rel(item['path'])} tools:",
        sorted(item["tools"]),
        f"{_rel(item['path'])} tool table",
        f"marks {sorted(item['table_denials'])} denied",
    )


def test_launcher_matrix_matches_its_case_block() -> None:
    """The launcher's own `--matrix` table agrees with its case block and cites the policy."""
    profiles = launcher_profiles()
    matrix = launcher_matrix()
    assert sorted(matrix) == sorted(ROLES), (
        f"{_rel(LAUNCHER)} --matrix prints rows for {sorted(matrix)} while the policy governs "
        f"{sorted(ROLES)}"
    )
    policy_lines = _policy_text().splitlines()
    for role, row in sorted(matrix.items()):
        profile = profiles[role]
        assert row["ws"] == MODE_WORDS[profile["ws"]], _mismatch(
            role,
            f"{_rel(LAUNCHER)} role_profile()",
            f"ROLE_WS={profile['ws']} -> {MODE_WORDS[profile['ws']]}",
            f"{_rel(LAUNCHER)} --matrix table",
            f"workspace {row['ws']}",
        )
        assert row["mem"] == MODE_WORDS[profile["mem"]], _mismatch(
            role,
            f"{_rel(LAUNCHER)} role_profile()",
            f"ROLE_MEM={profile['mem']} -> {MODE_WORDS[profile['mem']]}",
            f"{_rel(LAUNCHER)} --matrix table",
            f"memory {row['mem']}",
        )
        assert row["cited_line"], _mismatch(
            role,
            f"{_rel(LAUNCHER)} --matrix table",
            "no policy line cited",
            f"{_rel(POLICY)} (Container permissions)",
            "cited by every row so a reader can check the mount",
        )
        line = int(row["cited_line"])
        assert 1 <= line <= len(policy_lines), (
            f"ROLE `{role}`: {_rel(LAUNCHER)} --matrix cites {_rel(POLICY)}:{line}, but that "
            f"file has {len(policy_lines)} lines"
        )
        assert "Container permissions" in policy_lines[line - 1], _mismatch(
            role,
            f"{_rel(LAUNCHER)} --matrix table",
            f"cites {_rel(POLICY)}:{line}",
            f"{_rel(POLICY)} that line",
            policy_lines[line - 1].strip()[:120],
        )


def test_policy_citations_resolve() -> None:
    """Every `path:line` citation in the policy carries its quoted literal at that line."""
    citation = re.compile(
        r"`(?P<path>[A-Za-z0-9_./\-]+\.(?:md|json|py|sh|txt)):(?P<range>\d+(?:-\d+)?)`"
        r"\s*`(?P<literal>[^`]+)`"
    )
    policy_text = _policy_text()
    checked = 0
    failures: list[str] = []
    for parenthetical in re.finditer(r"\((?P<inner>[^()]*)\)", policy_text):
        for match in citation.finditer(parenthetical.group("inner")):
            path = REPO / match.group("path")
            literal = _norm(match.group("literal"))
            claimed = int(match.group("range").split("-")[0])
            if not path.is_file():
                failures.append(
                    f"  {match.group('path')}:{match.group('range')} — no such file, so the "
                    f"claim it supports cannot be checked"
                )
                continue
            raw = path.read_text(encoding="utf-8")
            normalised, offsets = _norm_with_map(raw)
            checked += 1
            found = None
            index = normalised.find(literal)
            while index >= 0:
                line = raw.count("\n", 0, offsets[index]) + 1
                if abs(line - claimed) <= 3:
                    found = line
                    break
                index = normalised.find(literal, index + 1)
            if found is None:
                failures.append(
                    f"  {_rel(path)}:{match.group('range')} — does not carry "
                    f"`{match.group('literal')}` within 3 lines of the cited line"
                )
    assert checked, (
        f"{_rel(POLICY)} carries no `path:line` citation with a quoted literal, so a reader "
        "cannot trace a rule to the artifact that settles it"
    )
    assert not failures, (
        f"{len(failures)} citation(s) in {_rel(POLICY)} do not resolve to the artifact they "
        "name:\n" + "\n".join(failures)
    )


def test_near_miss_citations_exist() -> None:
    """Every near-miss the policy cites is defined in the calibration log."""
    cited = policy_near_misses()
    defined = calibration_near_misses()
    assert cited, (
        f"{_rel(POLICY)} cites no near-miss identifier, so no denial is traced to an observed "
        f"event in {_rel(CALIBRATION)}"
    )
    missing = sorted(cited - defined)
    assert not missing, (
        f"{_rel(POLICY)} cites near-miss {missing} but {_rel(CALIBRATION)} defines only "
        f"{sorted(defined)}.\n"
        f"  policy   : {_rel(POLICY)} (Policy basis and role-entry denials)\n"
        f"  artifact : {_rel(CALIBRATION)} `## Near-miss patterns for Module 4 governance`\n"
        "  fix one side : add the pattern to the log, or cite a pattern that exists"
    )


def test_policy_artifacts_present() -> None:
    """Every artifact the policy cites exists, and every role entry is complete."""
    for path in (
        POLICY,
        ROUTING_MD,
        ROUTING_JSON,
        CALIBRATION,
        STORAGE_ALLOW,
        RETRIEVAL_ALLOW,
        LAUNCHER,
    ):
        _read(path)

    sections = policy_sections()
    missing_roles = sorted(set(ROLES) - set(sections))
    assert not missing_roles, (
        f"{_rel(POLICY)} carries no entry for {missing_roles}, so no artifact answers for "
        "those roles"
    )
    extra_roles = sorted(set(sections) - set(ROLES))
    assert not extra_roles, (
        f"ROLE `{extra_roles[0]}`: {_rel(POLICY)} carries an entry the launcher and the "
        f"allow-lists do not know ({sorted(launcher_profiles())})"
    )
    for role in ROLES:
        for dimension in DIMENSIONS:
            _subsection(sections[role], dimension)
        policy_container(role)
        definition(role)


def test_every_role_is_defined_in_a_definition() -> None:
    """The policed roles match the roles the launcher, the map and the definitions carry."""
    sections = set(policy_sections())
    grants = set(routing_map()["grants"])
    assert sections == grants, (
        f"{_rel(POLICY)} governs {sorted(sections)} while {_rel(ROUTING_JSON)} carries grants "
        f"for {sorted(grants)}"
    )
    ungoverned = []
    for path in sorted(AGENTS_DIR.glob("*.md")):
        text = path.read_text(encoding="utf-8")
        front = re.match(r"^---\n(.*?)\n---\n", text, re.S)
        if not front:
            continue
        governed = bool(
            re.search(r"^autonomy:", front.group(1), re.M)
            or re.search(r"`mcp__[a-z0-9]+__[a-z0-9_]+`", front.group(1))
            or re.search(r"^tools:.*mcp__", front.group(1), re.M)
        )
        if governed and path.stem not in sections:
            ungoverned.append(_rel(path))
    assert not ungoverned, (
        f"{ungoverned} carry gate tools or an autonomy level but have no entry in "
        f"{_rel(POLICY)}, so nothing scopes them"
    )
