#!/usr/bin/env python3
"""Onboard a project: give a repository its own identity, seam table, runtime and container.

    scripts/new-project.py <repo> --propose [--name SLUG] [--ports G,S,R] [--gate NAME=ARGV]...
    #  ... edit <repo>/agentic.project.json ...
    scripts/new-project.py <repo> --apply [--write-env] [--no-docker] [--force]

WHY THREE MODES
    --propose detects what it can and writes a DRAFT, touching nothing else. --apply reads that file and
    wires everything from it. The split exists because detection cannot know the judgement calls -- the
    slug, the ports, which extra gates a project needs -- and because a proposal you review as a file
    beats values buried in a command line. It also makes the manifest the record of the port:
    re-running --apply is idempotent and produces the same tree.

WHAT --apply WRITES, AND FROM WHERE
    Every artefact comes from the ONE manifest, so the copies cannot drift:
      agentic.config.json    the seam table: the parent kit's shape, with this project's identity
      .env.example           the runtime names, re-pointed from the parent's, comments included
      .env                   ONLY with --write-env; it holds your machine's paths and is gitignored, so
                             it is never rewritten silently
      docker objects         the internal network, the broker network, the registry volume, the image
      the launcher's ports   scripts/start-mcp-servers.sh, so the ports the config advertises and the
                             ports the servers bind cannot disagree
      the consumers' literals every file the fork check sweeps (scripts/, mcp/, eval/, .claude/,
                             .github/workflows/ and the root wiring files): each value the ancestor
                             carried is rewritten to this project's value, longest literal first, so a
                             fallback left behind in a consumer cannot survive the fork. The set is
                             derived the same way the check derives it, so the two read the same tree.
    A seam whose value the manifest leaves EQUAL to the parent's -- a Rust crate that keeps its name,
    so the clippy cache-hit guard touches the same file -- is recorded in `port.kept` with its reason
    rather than failed, exactly as the reference kit recorded a deliberately kept seam.
    Then it runs the kit's fork check and prints the result.

THE PARENT'S DEFAULTS (`port.upstream_defaults`, and why that name)
    The fork check asks, per recorded key, "does the live value still equal the recorded default?".
    As shipped the map held the FIRST ancestor's values, so a fork of a fork passed vacuously -- its
    values had differed from the first ancestor's for a generation. Recording the IMMEDIATE PARENT
    keeps the check meaningful at any depth, and `port.parent` names that kit so messages can say so.

WHAT THIS DOES NOT DO
    It does not carry the product, rewrite the agent guide, or port the role definitions. Those are
    judgement, not identity: see PORTING.md.
"""
from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path
from typing import Any

HERE = Path(__file__).resolve().parent
KIT_ROOT = HERE.parent
MANIFEST_NAME = "agentic.project.json"

# The model values this kit carries today, one per consumer. Accepting the wizard's defaults must
# change nothing, so every default here equals the value already in force: the agent CLI resolves
# its own model today, the sandbox's opencode declares `deepseek-v4-flash` in
# sandbox/opencode-sandbox.json, and scripts/start-mcp-servers.sh exports BAAI/bge-small-en-v1.5.
# This block is NOT a port.seam: a seam must differ from its ancestor's value, and these equal it.
MODEL_DEFAULTS = {
    "agent": "",
    "sandbox_cli": "deepseek-v4-flash",
    "retrieval": "BAAI/bge-small-en-v1.5",
}

# The paths that carry PROJECT IDENTITY, declared in `port.seams` and checked against every
# ancestor's recorded value. This list is the seam table's own statement of what a fork must
# change; it lives in the config so completeness is reviewable there, not in a script.
SEAMS = (
    "project.name",
    "toolchain.commands.clippy.guard.marker",
    "toolchain.commands.clippy.guard.marker_regex",
    "toolchain.commands.clippy.guard.touch_file",
    "containers.base_image",
    "containers.tools_image",
    "containers.registry_volume",
    "containers.target_volume",
    "containers.seed_container",
    "containers.networks.internal",
    "containers.networks.broker",
    "containers.broker.name",
    "artifacts.project_key",
    "console.container",
    "console.ports",
    "console.role_container_prefix",
)

# The files the fork check sweeps, mirrored here so --apply re-points exactly what the check reads.
# scripts/port-self-test.sh DERIVES this set from the tree's wiring rather than hand-listing it, and
# gives the reason: a hand list named seven files and missed a launcher that carried the reference
# image name. The set is named twice -- once there, once here -- because the check is a bash script
# with no importable module and the wizard must not guess a narrower list; the constants match it
# key for key, and a file the check scans is a file the wizard has re-pointed.
WIRING_DIRS = ("scripts/", "mcp/", "eval/", ".claude/", ".github/workflows/")
WIRING_SUFFIXES = (".py", ".sh", ".json", ".yml", ".yaml")
WIRING_MARKDOWN_DIRS = (".claude/agents/",)
WIRING_ROOT_FILES = ("docker-entrypoint.sh", ".mcp.json")
# Each exclusion carries the reason the check gives it: prose and the historical record name the
# reference project legitimately, and the console's Rust fallback is a separate change, gated there.
SCAN_EXCLUDED = (".memory/", "docs/", "node_modules/", "src/", "target/", "vendor/")

# ---------------------------------------------------------------- json helpers


def dig(doc: Any, dotted: str, default: Any = None) -> Any:
    cur = doc
    for part in dotted.split("."):
        if not isinstance(cur, dict) or part not in cur:
            return default
        cur = cur[part]
    return cur


def plant(doc: dict, dotted: str, value: Any) -> None:
    parts = dotted.split(".")
    cur = doc
    for part in parts[:-1]:
        cur = cur.setdefault(part, {})
    cur[parts[-1]] = value


# ---------------------------------------------------------------- language detection


def detect_rust(repo: Path) -> dict | None:
    """A Cargo workspace or crate. Gates are cargo's; the clippy guard needs a real touch file."""
    manifest = repo / "Cargo.toml"
    if not manifest.is_file():
        return None

    # A workspace root may hold no package of its own; the crate that matters is the first member.
    crate_dir = repo
    text = manifest.read_text()
    if "[workspace]" in text and "[package]" not in text:
        members = re.findall(r"members\s*=\s*\[([^\]]*)\]", text)
        names = re.findall(r'"([^"]+)"', members[0]) if members else []
        if names:
            crate_dir = repo / names[0]

    crate = crate_dir / "Cargo.toml"
    ctext = crate.read_text() if crate.is_file() else ""
    found = re.search(r'^\s*name\s*=\s*"([^"]+)"', ctext, re.M)
    name = found.group(1) if found else crate_dir.name

    # The touch file is what proves a clippy run was not a cache hit. A declared bin path is best.
    binpath = re.search(r'path\s*=\s*"([^"]+\.rs)"', ctext)
    if binpath:
        touch = str((crate_dir / binpath.group(1)).relative_to(repo))
    elif (crate_dir / "src").is_dir():
        candidates = sorted((crate_dir / "src").glob("*.rs"))
        touch = str(candidates[0].relative_to(repo)) if candidates else "Cargo.toml"
    else:
        touch = "Cargo.toml"

    manifest_arg = [] if crate_dir == repo else ["--manifest-path", str(crate.relative_to(repo))]
    return {
        "language": "rust",
        "package": name,
        "manifest": str(manifest.relative_to(repo)),
        "gates": {
            "build": {"says": "the crate compiles in release",
                      "argv": ["cargo", "build", "--release", *manifest_arg]},
            "test": {"says": "the test suite passes",
                     "argv": ["cargo", "test", "--release", *manifest_arg]},
            "clippy": {"says": "clippy is clean; warnings are errors",
                       "argv": ["cargo", "clippy", "--release", "--all-targets", *manifest_arg,
                                "--", "-D", "warnings"],
                       "guard": {"touch_file": touch, "marker": f"Checking {name}"}},
            "fmt": {"says": "rustfmt reports no diff", "argv": ["cargo", "fmt", "--check"]},
        },
    }


def detect_node(repo: Path) -> dict | None:
    pkg = repo / "package.json"
    if not pkg.is_file():
        return None
    data = json.loads(pkg.read_text())
    scripts = data.get("scripts", {})
    gates = {}
    for gate, key in (("build", "build"), ("test", "test"), ("lint", "lint")):
        if key in scripts:
            gates[gate] = {"says": f"npm run {key} passes", "argv": ["npm", "run", key]}
    return {"language": "node", "package": data.get("name", repo.name),
            "manifest": "package.json", "gates": gates}


def detect_python(repo: Path) -> dict | None:
    if not (repo / "pyproject.toml").is_file():
        return None
    return {"language": "python", "package": repo.name, "manifest": "pyproject.toml",
            "gates": {"test": {"says": "pytest passes",
                               "argv": ["python3", "-m", "pytest", "-q"]}}}


def detect(repo: Path) -> dict:
    for detector in (detect_rust, detect_node, detect_python):
        found = detector(repo)
        if found:
            return found
    raise SystemExit("no supported project manifest in "
                     f"{repo} (looked for Cargo.toml, package.json, pyproject.toml)")


# ---------------------------------------------------------------- the manifest


def build_manifest(parent_cfg: dict, parent_kit: Path, slug: str, repo: Path, facts: dict,
                   ports: dict[str, int], extra_gates: dict[str, dict],
                   console_container: str, models: dict) -> dict:
    gates = {**facts["gates"], **extra_gates}
    # The clippy cache-hit guard's marker is the string a cargo run prints: "Checking <crate>". A
    # copy of the parent detects the PARENT's crate name, and a marker equal to the parent's would
    # fail the fork check for a seam that IS this project's identity -- so when detection lands on
    # the parent's own project name, the marker names THIS project instead. The crate then has to be
    # renamed to the slug (or the manifest edited) for a real cargo run to print it, and the note
    # says so rather than letting the guard go quietly unsatisfiable.
    guard = dig(gates, "clippy.guard")
    if (isinstance(guard, dict) and guard.get("marker") == f"Checking {facts.get('package')}"
            and facts.get("package") == dig(parent_cfg, "project.name")):
        print(f"  note  the {facts.get('language')} crate is still named {facts.get('package')!r}, "
              f"the parent's project name.")
        print(f"        The clippy guard's marker names this project, {slug!r}; rename the crate to "
              f"{slug!r} (or edit gates.clippy.guard.marker) so a cargo run prints it.")
        guard["marker"] = f"Checking {slug}"
    return {
        "schema_version": "1",
        "project": {
            "name": slug,
            "repo": str(repo),
            "key": f"proj-{slug}",
            "parent": {"name": str(dig(parent_cfg, "project.name", parent_kit.name)),
                       "repo": str(parent_kit)},
        },
        "detected": {k: facts[k] for k in ("language", "package", "manifest")},
        "gates": gates,
        "runtime": {
            "image": f"agent-sandbox:{slug}",
            "tools_image": f"agent-sandbox:{slug}-m1",
            "broker_image": f"{slug}-sandbox-broker:local",
            "broker_name": f"{slug}-broker",
            "broker_port": 4000,
            "net_broker": f"{slug}-net",
            "net_internal": f"{slug}-internal",
            "registry_volume": f"{slug}-cargo-registry",
            "target_volume": f"{slug}-cargo-target",
            "seed_container": "",   # none for a new project; the launcher skips the seed
            "agent_name": f"agent-{slug}",
            "state_dir": f"$HOME/.config/{slug}-sandbox",
            "console_container": console_container,
        },
        "ports": ports,
        "models": models,
        "artifacts": {
            "evidence_dir": f"~/{slug}-evidence",
            "briefs_dir": f"~/{slug}-evidence/briefs",
        },
    }


def apply_manifest(parent_cfg: dict, manifest: dict, recorded_parent: dict | None = None
                   ) -> tuple[dict, dict]:
    """The parent's seam table, re-pointed from the manifest. Returns (config, recorded_defaults).

    `recorded_parent` is the parent's OWN ancestry entry, and it is set only when a tree onboards
    itself -- the wizard run from inside the tree it is onboarding, as the kit's own copy is. Then
    `parent_cfg` is the tree's CURRENT config, and the parent whose values complete the chain is the
    generation that config already records; re-reading the parent's seams from the live config would
    record this project as its own ancestor and lose idempotence. Passing the recorded entry in keeps
    the second `--apply` byte for byte the first.
    """
    cfg = json.loads(json.dumps(parent_cfg))  # deep copy: never mutate the parent's table

    # 1. every declared seam, at the value the PARENT carries. Read BEFORE re-pointing anything:
    #    this becomes the parent's entry in the ancestry chain.
    if recorded_parent is not None:
        parent_values = recorded_parent.get("seams") or {}
        parent_review = recorded_parent.get("review") or {}
    else:
        parent_values = {k: dig(cfg, k) for k in SEAMS}
        parent_values = {k: v for k, v in parent_values.items() if v is not None}
        parent_review = {k: dig(cfg, k) for k in REVIEW_KEYS}
        parent_review = {k: v for k, v in parent_review.items() if v is not None}

    p, rt, pr = manifest["project"], manifest["runtime"], manifest["ports"]

    # 1b. the models this project runs, recorded beside the ports from the ONE manifest. The block is
    #     not a seam (its values equal the ancestor's, so a seam would fail the fork check), and a
    #     manifest written before this block carries none, so an absent block defaults to this kit's.
    declared = manifest.get("models") or {}
    if "models" not in manifest:
        print("  note  the manifest predates the models block; writing this kit's values.")
    plant(cfg, "models", {k: declared.get(k, v) for k, v in MODEL_DEFAULTS.items()})

    # 2. identity
    plant(cfg, "project.name", p["name"])
    plant(cfg, "project_key", p["key"])
    plant(cfg, "artifacts.project_key", p["key"])
    plant(cfg, "artifacts.evidence_dir", manifest["artifacts"]["evidence_dir"])
    plant(cfg, "artifacts.briefs_dir", manifest["artifacts"]["briefs_dir"])

    # 3. the gate vocabulary: what this project can actually run. A gate the project cannot run is
    #    worse than no gate, so anything inherited that the manifest does not declare is dropped.
    commands = cfg.setdefault("toolchain", {}).setdefault("commands", {})
    for gate in list(commands):
        if gate not in manifest["gates"]:
            del commands[gate]
    for gate, spec in manifest["gates"].items():
        entry: dict[str, Any] = {"argv": spec["argv"], "says": spec["says"]}
        if "guard" in spec:
            entry["guard"] = spec["guard"]
        commands[gate] = entry
    # The guard's marker regex is derived from the marker, so the two cannot be edited apart.
    marker = dig(cfg, "toolchain.commands.clippy.guard.marker")
    if isinstance(marker, str) and marker:
        plant(cfg, "toolchain.commands.clippy.guard.marker_regex",
              rf"\bChecking\b\s+(?P<marker>{re.escape(marker.split(' ', 1)[-1])})\b")

    # 4. the runtime identity: this project's own, so a run can never land in another's container
    plant(cfg, "containers.base_image", rt["image"])
    plant(cfg, "containers.tools_image", rt["tools_image"])
    plant(cfg, "containers.registry_volume", rt["registry_volume"])
    plant(cfg, "containers.networks.internal", rt["net_internal"])
    plant(cfg, "containers.networks.broker", rt["net_broker"])
    plant(cfg, "containers.broker.name", rt["broker_name"])
    # A manifest written by an earlier version may predate a key. Default it and SAY SO: a KeyError
    # tells the user nothing, and quietly inventing a value tells them something false.
    absent = [k for k in ("target_volume", "seed_container") if k not in rt]
    if absent:
        print(f"  note  the manifest predates {(', '.join(absent))}; defaulted. Re-run "
              f"--propose --force to write it in full.")
    plant(cfg, "containers.target_volume", rt.get("target_volume", f"{p['name']}-cargo-target"))
    plant(cfg, "containers.seed_container", rt.get("seed_container", ""))

    # 5. the ancestry chain. Inherit what this kit inherited, then append the immediate parent at
    #    the values it carried. Each fork keeps the WHOLE lineage, which is what lets the check
    #    find a grandparent's leftovers -- a parent-only record cannot, and that is how one
    #    generation's names reach a third.
    parent_chain = dig(parent_cfg, "port.ancestors") or []
    if recorded_parent is not None and parent_chain:
        # This tree's own config already carries the entry we are regenerating; drop it so a second
        # apply does not append the same parent twice.
        parent_chain = parent_chain[:-1]
    cfg["port"] = {
        "seams": list(SEAMS),
        "ancestors": [
            *parent_chain,
            {"name": p["parent"]["name"], "seams": parent_values, "review": parent_review},
        ],
    }
    # `kept` is computed at the END of this function, once every seam has its final value: a seam
    # recorded there must equal the parent's in the FINAL config, and the console block is re-pointed
    # below step 5.
    # An excusal names a value in an INHERITED file, so it carries down. `kept` does not: keeping
    # an ancestor's value is a decision each project makes for itself.
    parent_quoted = dig(parent_cfg, "port.quoted") or []
    if parent_quoted:
        cfg["port"]["quoted"] = parent_quoted
    # `records` names files kept as records rather than wiring. The fork inherits those files, so it
    # inherits the reason they are not swept -- and unlike `kept`, this is not a decision it re-makes.
    parent_records = dig(parent_cfg, "port.records") or {}
    if parent_records:
        cfg["port"]["records"] = parent_records

    # 6. the console block. A console pointed at a repo whose config it cannot read falls back to
    #    ANOTHER project's identity, container included. Recording the project and the repo here is
    #    what stops that; the ports go in too, and --apply re-points the launcher to match them.
    parent_console = parent_cfg.get("console", {}) or {}
    console = cfg.setdefault("console", {})
    console["container"] = rt["console_container"]
    console["role_container_prefix"] = f"{p['name']}-agent-"
    console["repo"] = p["repo"]
    console["project"] = p["name"]
    console["ports"] = pr
    # A console key whose value still equals the parent's is honestly still the parent's; list those
    # rather than re-pointing them to a fiction.
    console["upstream_defaults"] = sorted(
        k for k, v in console.items()
        if not k.startswith("_") and k not in ("ports", "repo", "project", "container",
                                               "role_container_prefix", "upstream_defaults")
        and v == parent_console.get(k))

    # Now that every seam has its final value, record the ones the manifest leaves equal to the
    # parent's. A Rust fork that keeps its crate name finds the clippy cache-hit guard touching the
    # same source file; that seam cannot differ, and `port.kept` is where the kit records such a
    # value -- printed with its reason rather than swept as a leak. The container and image seams are
    # always re-pointed above, so they are never kept: restoring an ancestor's container name in the
    # config still fails the check.
    kept = kept_seams(parent_values, cfg, str(p["parent"]["name"]))
    if kept:
        cfg["port"]["kept"] = kept

    return cfg, parent_values


# ---------------------------------------------------------------- .env.example


def render_env_example(parent_kit: Path, parent_cfg: dict, manifest: dict) -> str | None:
    """Re-point the parent's `.env.example` rather than authoring one from scratch: its comments carry
    the reasons, and a fork that inherits the file inherits the documentation. Substituting every name
    the parent's config declares also catches the broker's name where it sits inside a URL, which is a
    place no checker looks."""
    source = parent_kit / ".env.example"
    if not source.is_file():
        return None
    text = source.read_text()

    # The source can be a `.env.example` a previous --apply already wrote -- a tree onboarding
    # itself reads its own -- so drop the header that run prepended. Prepending it again would grow
    # the file on every apply, and the file has to be byte-identical when the manifest is unchanged.
    text = re.sub(r"^# Environment for [^\n]*\n#\n# Generated from [^\n]*\n(?:# [^\n]*\n)*#\n",
                  "", text, count=1)

    rt, p = manifest["runtime"], manifest["project"]
    parent_name = str(dig(parent_cfg, "project.name", ""))
    # The env file uses TWO names for the same project: the full slug, and a shorter one that
    # survives from how the broker and the state dir are named (`<slug>-broker` -> `<slug>`).
    # Re-pointing only the long one leaves half the file on the parent's names -- which is what
    # happened the first time this ran, with BROKER_IMAGE and STATE staying put while IMAGE
    # moved. Both forms are substituted, and only ever inside a full pattern.
    parent_broker = str(dig(parent_cfg, "containers.broker.name", ""))
    parent_short = (parent_broker[:-len("-broker")]
                    if parent_broker.endswith("-broker") else parent_name)
    renames = [
        # From the parent's config: the declarations that exist in two files at once.
        (str(dig(parent_cfg, "containers.tools_image", "")), rt["tools_image"]),
        (str(dig(parent_cfg, "containers.registry_volume", "")), rt["registry_volume"]),
        (parent_broker, rt["broker_name"]),
        (str(dig(parent_cfg, "containers.networks.internal", "")), rt["net_internal"]),
        (str(dig(parent_cfg, "containers.networks.broker", "")), rt["net_broker"]),
        (str(dig(parent_cfg, "console.container", "")), rt["console_container"]),
        (str(dig(parent_cfg, "artifacts.evidence_dir", "")), manifest["artifacts"]["evidence_dir"]),
        # Names the env file is the ONLY declaration of, under BOTH of the project's names.
        (f"{parent_name}-cargo-target", rt["target_volume"]),
        (f"{parent_short}-cargo-target", rt["target_volume"]),
        (f"agent-{parent_name}", rt["agent_name"]),
        (f"$HOME/.config/{parent_name}-sandbox", rt["state_dir"]),
        (f"$HOME/.config/{parent_short}-sandbox", rt["state_dir"]),
        (f"{parent_name}-sandbox-broker:local", rt["broker_image"]),
        (f"{parent_short}-sandbox-broker:local", rt["broker_image"]),
        (f"$HOME/{parent_kit.name}", f"$HOME/{p['name']}"),
        # The comments spell the same paths with a tilde, so both forms are substituted.
        (f"~/{parent_kit.name}", f"~/{p['name']}"),
        (f"~/.config/{parent_name}-sandbox", f"~/.config/{p['name']}-sandbox"),
        (f"~/.config/{parent_short}-sandbox", f"~/.config/{p['name']}-sandbox"),
    ]
    # Longest first: a shorter key must never rewrite part of a longer one.
    for old, new in sorted(renames, key=lambda kv: -len(kv[0])):
        if old:
            text = text.replace(old, new)

    header = (f"# Environment for {p['name']}'s agent runtime.\n"
              f"#\n"
              f"# Generated from {MANIFEST_NAME} by scripts/new-project.py --apply. Edit the manifest and\n"
              f"# re-apply rather than editing this file: two sources for one value is the drift this kit\n"
              f"# exists to prevent.\n"
              f"#\n")
    return header + text


# ---------------------------------------------------------------- the launcher's port literals


# Keys a fork should REVIEW but may legitimately keep: the CLI's own name, its flags, the fallback
# job list, the session directory. These are recorded per generation and reported, never SWEPT -- a
# sweep for "claude" would match every consumer in the tree -- and never failed. The distinction is
# real and was found the hard way: the old single map was hand-picked for exactly this reason, which
# is also why it was incomplete, and a map that cannot record these cannot answer the console either.
REVIEW_KEYS = (
    "console.claude_command", "console.claude_flags", "console.evidence_dir", "console.briefs_dir",
    "console.session_dir", "console.checkpoint_fresh_minutes", "console.ci_jobs",
    "console.orchestration_steps",
)

# Where each server's port is declared, as (file, the literal text before the digits, which port).
# The digits that follow the prefix are what gets replaced. A prefix that matches NOTHING is reported
# and fails the run: the first version of this function searched for a line that did not exist and
# returned success anyway, which is how the ports came to disagree in the first place. Every prefix
# here is backslash-free so it is safe both as a regex (via re.escape) and as a replacement.
PORT_TARGETS = (
    ("scripts/start-mcp-servers.sh", 'STORAGE_PORT="${STORAGE_PORT:-', "storage"),
    ("scripts/start-mcp-servers.sh", 'RETRIEVAL_PORT="${RETRIEVAL_PORT:-', "retrieval"),
    ("mcp/gate/server.py", '"--port", type=int, default=', "gate"),   # the argparse default
    ("mcp/gate/server.py", "--port ", "gate"),                        # the docstring's example
    ("mcp/gate/server.py", "(default ", "gate"),                      # the flag's own help text
)


def repoint_server_ports(repo: Path, manifest: dict) -> list[str]:
    """Write this project's ports over the ones the servers and their launcher carry.

    The kit shipped the config's ports disagreeing with the ports the servers bind, and nothing could
    see it. Writing both from ONE manifest is what makes them agree by construction. A prefix that
    matches nothing is a FAILURE, not a skip -- a silent no-op here reproduces the original defect.
    """
    notes: list[str] = []
    for rel, prefix, key in PORT_TARGETS:
        path = repo / rel
        port = manifest["ports"][key]
        if not path.is_file():
            notes.append(f"NO TARGET: {rel} is absent, so {key} could not be re-pointed")
            continue
        text = path.read_text()
        new_text, count = re.subn(re.escape(prefix) + "[0-9]+", prefix + str(port), text)
        if count == 0:
            notes.append(f"NO TARGET: {rel} has no {prefix!r} line, so {key} is still unset")
            continue
        if new_text != text:
            path.write_text(new_text)
        notes.append(f"{rel}: {count} site(s) -> {key} {port}")
    return notes


# ---------------------------------------------------------------- the consumers' literals


def _leaf_pairs(old: Any, new: Any, out: list[tuple[str, str]]) -> None:
    """Append (old-literal, new-literal) pairs for one seam, pairing by KEY and not by position.

    A seam can hold a dict -- console.ports does -- and pairing positionally would map a storage port
    onto the gate port: 8001 storage must become 8202 storage, never 8201. A value shorter than the
    four characters the check searches for is skipped, so re-pointing follows the same sight line the
    check reads by. A bool is not a literal to substitute.
    """
    if isinstance(old, dict) and isinstance(new, dict):
        for key in old:
            if key in new:
                _leaf_pairs(old[key], new[key], out)
        return
    if isinstance(old, list) and isinstance(new, list):
        for o, n in zip(old, new):
            _leaf_pairs(o, n, out)
        return
    if isinstance(old, bool) or isinstance(new, bool):
        return
    if isinstance(old, (str, int, float)) and isinstance(new, (str, int, float)):
        o, n = str(old), str(new)
        if len(o) >= 4 and n and o != n:
            out.append((o, n))


def identity_renames(parent_cfg: dict, cfg: dict) -> list[tuple[str, str]]:
    """The parent's identity values, each paired with this project's value for the same seam.

    A seam the manifest leaves equal to the parent's yields no pair: its literal IS this project's, so
    rewriting it would be wrong. Every other seam becomes a substitution, in both its plain and its
    source-escaped form -- the marker regex carries single backslashes in the JSON and doubled ones in
    Python source, exactly the two forms the check searches for.
    """
    pairs: list[tuple[str, str]] = []
    for path in SEAMS:
        old, new = dig(parent_cfg, path), dig(cfg, path)
        if old is None or old == new:
            continue
        _leaf_pairs(old, new, pairs)
    subs: list[tuple[str, str]] = []
    for old, new in pairs:
        subs.append((old, new))
        if "\\" in old or "\\" in new:
            subs.append((old.replace("\\", "\\\\"), new.replace("\\", "\\\\")))
    # Longest first, so a shorter key never rewrites part of a longer one: agent-sandbox:console-m1
    # must be handled before agent-sandbox:console, and the whole marker regex before its inner name.
    return sorted(set(subs), key=lambda kv: -len(kv[0]))


def _wiring_files(repo: Path, records: dict) -> list[str]:
    """The tree's wiring files, chosen by the same predicate scripts/port-self-test.sh uses."""
    found: list[str] = []
    for path in sorted(repo.rglob("*")):
        if not path.is_file():
            continue
        rel = path.relative_to(repo).as_posix()
        if rel.startswith("__pycache__/") or rel.endswith((".pyc", ".pyo")):
            continue
        if rel in records or any(rel.startswith(d) for d in SCAN_EXCLUDED):
            continue
        if rel.startswith(WIRING_DIRS) and rel.endswith(WIRING_SUFFIXES):
            found.append(rel)
        elif rel.startswith(WIRING_MARKDOWN_DIRS) and rel.endswith(".md"):
            found.append(rel)
        elif "/" not in rel and (rel.startswith("Dockerfile") or rel in WIRING_ROOT_FILES):
            found.append(rel)
    return found


def repoint_consumers(repo: Path, parent_cfg: dict, cfg: dict) -> list[str]:
    """Rewrite every ancestor identity literal in the files the fork check sweeps.

    This is the step that makes the wizard's docstring true: the config is not the only copy of a
    fork's identity. A consumer that still holds the parent's container name, image tag, project key,
    port or network is the half-wired case the check exists to catch, and the check names it by file
    and line. The mapping comes from the seam table alone -- the value the parent carried at each
    seam against the value this manifest plants -- so a file the check reads cannot disagree with the
    config afterwards. Returns the files changed, for the run to print.
    """
    subs = identity_renames(parent_cfg, cfg)
    if not subs:
        return []
    records = dig(cfg, "port.records") or {}
    changed: list[str] = []
    for rel in _wiring_files(repo, records):
        path = repo / rel
        text = path.read_text(encoding="utf-8", errors="surrogateescape")
        new_text = text
        for old, new in subs:
            if old in new_text:
                new_text = new_text.replace(old, new)
        if new_text != text:
            path.write_text(new_text, encoding="utf-8", errors="surrogateescape")
            changed.append(rel)
    return changed


def kept_seams(parent_values: dict, cfg: dict, parent_name: str) -> dict[str, str]:
    """Seams the manifest leaves at the parent's value, each with the reason it is honest to keep.

    A fork of a Rust crate that keeps its name finds the clippy cache-hit guard touching the same
    source file, so that seam cannot differ from the parent's -- and a seam that legitimately equals
    an ancestor's is exactly what `port.kept` records. Recorded rather than swept, printed rather
    than silent: the check lists it as kept, names the reason, and stops reporting every honest use
    of the value as a leak. Only a seam the seam table places here is kept; the container and image
    seams are always re-pointed, so the mutation that restores an ancestor's container name still
    fails the check. `parent_values` is the effective parent's seam table -- the parent kit's, or the
    recorded generation's when a tree onboards itself -- so this is stable across a second apply.
    """
    kept: dict[str, str] = {}
    for path in SEAMS:
        old, new = parent_values.get(path), dig(cfg, path)
        if old is not None and old == new:
            kept[path] = (f"this project's {path!r} equals {parent_name}'s ({old!r}): the manifest "
                          f"leaves it there because the detected layout does not depart from it, so "
                          f"keeping it is honest usage, not an unported leftover")
    return kept


# ---------------------------------------------------------------- docker


def plan_runtime(manifest: dict) -> list[list[str]]:
    """Existence-checked by the caller, so applying twice to the same project is not an error."""
    rt = manifest["runtime"]
    return [
        ["docker", "network", "create", "--internal", rt["net_internal"]],
        ["docker", "network", "create", rt["net_broker"]],
        ["docker", "volume", "create", rt["registry_volume"]],
        ["docker", "build", "-t", rt["tools_image"], "-f", "Dockerfile", "."],
    ]


# ---------------------------------------------------------------- modes


def show(manifest: dict, recorded: dict | None = None, heading: str = "") -> None:
    p, rt, pr = manifest["project"], manifest["runtime"], manifest["ports"]
    if heading:
        print(heading)
    print(f"project    : {p['name']}   key {p['key']}")
    print(f"repo       : {p['repo']}")
    print(f"forked from: {p['parent']['repo']}   (project.name = {p['parent']['name']})")
    d = manifest["detected"]
    print(f"language   : {d['language']}   package = {d['package']}   manifest = {d['manifest']}")
    for gate, spec in sorted(manifest["gates"].items()):
        print(f"  gate {gate:10s} {' '.join(spec['argv'])}")
    print(f"runtime    : tools image {rt['tools_image']}   broker {rt['broker_name']}")
    print(f"             networks {rt['net_internal']} + {rt['net_broker']}   "
          f"volume {rt['registry_volume']}")
    print(f"             console {rt['console_container']}")
    print(f"ports      : gate {pr['gate']}  storage {pr['storage']}  retrieval {pr['retrieval']}")
    mod = manifest.get("models") or dict(MODEL_DEFAULTS)
    print(f"models     : agent {mod['agent'] or '(the CLI resolves its own default)'}   "
          f"sandbox {mod['sandbox_cli']}   retrieval {mod['retrieval']}")
    if recorded:
        print(f"\nport.ancestors gets the PARENT's {len(recorded)} values as its newest entry, so "
              f"the check works at any depth rather than one generation:")
        for k, v in recorded.items():
            print(f"  {k:52s} {json.dumps(v)[:46]}")


def mode_propose(args, repo: Path, slug: str, parent_kit: Path, parent_cfg: dict) -> int:
    facts = detect(repo)
    ports: dict[str, int] = dict(zip(("gate", "storage", "retrieval"),
                                     [int(p) for p in args.ports.split(",")]))
    extra: dict[str, dict] = {}
    for spec in args.gate:
        if "=" not in spec:
            raise SystemExit(f"--gate wants NAME=ARGV, got {spec!r}")
        name, argv = spec.split("=", 1)
        extra[name] = {"says": f"the {name} gate passes", "argv": argv.split()}

    manifest = build_manifest(parent_cfg, parent_kit, slug, repo, facts, ports, extra,
                              args.console_container or f"{slug}-console",
                              {"agent": args.agent_model, "sandbox_cli": args.sandbox_model,
                               "retrieval": args.retrieval_model})
    show(manifest, heading="proposed manifest -- detected values; edit what detection cannot know:")

    target = repo / MANIFEST_NAME
    if args.dry_run:
        print(f"\n--dry-run: nothing written. The manifest would go to {target}")
        return 0
    if target.exists() and not args.force:
        print(f"\nrefusing: {target} exists. Edit it and --apply, or pass --force to overwrite.")
        return 1
    target.write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"\nwrote {target}")
    print("next: edit it (the slug, the ports, any extra gate), then:")
    print(f"      scripts/new-project.py {repo} --apply")
    return 0


def mode_apply(args, repo: Path) -> int:
    manifest_path = repo / MANIFEST_NAME
    if not manifest_path.is_file():
        raise SystemExit(f"no {MANIFEST_NAME} in {repo} -- run --propose first")
    manifest = json.loads(manifest_path.read_text())

    parent_repo = Path(dig(manifest, "project.parent.repo", str(KIT_ROOT)))
    parent_cfg_path = parent_repo / "agentic.config.json"
    if not parent_cfg_path.is_file():
        raise SystemExit(f"the parent kit at {parent_repo} carries no agentic.config.json")
    parent_cfg = json.loads(parent_cfg_path.read_text())

    # A tree can onboard ITSELF -- the wizard run from inside the tree it is onboarding, which is how
    # this kit's own copy is onboarded. Then the parent path resolves to the tree, and re-reading its
    # live config as the parent would record this project as its own ancestor and change the tree on
    # a second apply. The parent is instead the generation this tree's config already records, so the
    # entry is handed to apply_manifest and the config made in the previous run is regenerated, not
    # re-derived from itself.
    recorded_parent: dict | None = None
    if (parent_repo.resolve() == repo.resolve()
            and dig(parent_cfg, "project.name") == dig(manifest, "project.name")):
        chain = dig(parent_cfg, "port.ancestors") or []
        if chain and chain[-1].get("name") == dig(manifest, "project.parent.name"):
            recorded_parent = chain[-1]

    cfg, recorded = apply_manifest(parent_cfg, manifest, recorded_parent)
    show(manifest, recorded, heading="applying:")

    target = repo / "agentic.config.json"
    if target.exists() and not args.force:
        print(f"\nrefusing: {target} exists. Pass --force to overwrite it.")
        return 1

    env_example = render_env_example(parent_repo, parent_cfg, manifest)
    if args.write_env:
        print("\n.env WILL be written (--write-env); it is gitignored and holds your paths")
    else:
        print("\n.env is NOT written (pass --write-env to write it); .env.example is")

    if args.dry_run:
        print("--dry-run: nothing written.")
        return 0

    target.write_text(json.dumps(cfg, indent=2) + "\n")
    print(f"\nwrote {target}")
    if env_example:
        (repo / ".env.example").write_text(env_example)
        print(f"wrote {repo / '.env.example'}")
    if args.write_env:
        (repo / ".env").write_text(env_example or "")
        print(f"wrote {repo / '.env'}")

    port_notes = repoint_server_ports(repo, manifest)
    for note in port_notes:
        print(f"  ports {note}")
    if any(n.startswith("NO TARGET") for n in port_notes):
        print("\nrefusing to report success: a port pattern matched nothing, so a server would")
        print("keep another project's port. Fix PORT_TARGETS for this layout, or say why.")
        return 1

    renamed = repoint_consumers(repo, parent_cfg, cfg)
    if renamed:
        print(f"\nre-pointed {len(renamed)} consumer file(s) from the seam table:")
        for rel in renamed:
            print(f"  {rel}")
    else:
        print("\nno consumer literal carried the parent's identity; nothing to re-point.")

    if not args.no_docker:
        for cmd in plan_runtime(manifest):
            print(f"  $ {' '.join(cmd)}")
            r = subprocess.run(cmd, cwd=repo, capture_output=True, text=True)
            noise = r.stderr + r.stdout
            if r.returncode != 0 and "already exists" not in noise:
                print(f"    FAILED: {r.stderr.strip()[:300]}")
                return 1

    check = repo / "scripts/port-self-test.sh"
    if check.is_file():
        print("\n=== the fork check ===")
        r = subprocess.run(["bash", str(check), "--fork"], cwd=repo, capture_output=True, text=True)
        print(r.stdout.rstrip())
        print(f"port-self-test: {'PASS' if r.returncode == 0 else 'FAIL'}")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description="Onboard a project: identity, seam table, runtime.")
    ap.add_argument("repo")
    mode = ap.add_mutually_exclusive_group(required=True)
    mode.add_argument("--propose", action="store_true", help="detect and write a draft manifest")
    mode.add_argument("--apply", action="store_true", help="wire everything from the manifest")
    ap.add_argument("--name", help="project slug (default: the repository directory's name)")
    ap.add_argument("--from", dest="kit", default=str(KIT_ROOT),
                    help="the kit checkout this project is forked from; its seam table is inherited")
    ap.add_argument("--ports", default="8201,8202,8203",
                    help="gate,storage,retrieval MCP ports -- this project's own")
    ap.add_argument("--console-container", default=None,
                    help="the container that watches this project (default: <slug>-console)")
    ap.add_argument("--agent-model", default=MODEL_DEFAULTS["agent"],
                    help="the agent CLI's model; empty sends no model flag, so the CLI resolves it")
    ap.add_argument("--sandbox-model", default=MODEL_DEFAULTS["sandbox_cli"],
                    help="the model the sandbox's opencode declares in sandbox/opencode-sandbox.json")
    ap.add_argument("--retrieval-model", default=MODEL_DEFAULTS["retrieval"],
                    help="the embedding model scripts/start-mcp-servers.sh exports for retrieval")
    ap.add_argument("--gate", action="append", default=[], metavar="NAME=ARGV",
                    help="add a project-specific gate, e.g. --gate 'packaging=python3 scripts/x.py'")
    ap.add_argument("--write-env", action="store_true",
                    help="--apply only: also write .env (gitignored, holds your machine's paths)")
    ap.add_argument("--no-docker", action="store_true", help="skip network/volume/image creation")
    ap.add_argument("--dry-run", action="store_true", help="print, write nothing")
    ap.add_argument("--force", action="store_true", help="overwrite an existing manifest or config")
    args = ap.parse_args()

    repo = Path(args.repo).expanduser().resolve()
    if not repo.is_dir():
        raise SystemExit(f"not a directory: {repo}")
    slug = args.name or repo.name

    if args.apply:
        return mode_apply(args, repo)

    parent_kit = Path(args.kit).expanduser().resolve()
    parent_cfg_path = parent_kit / "agentic.config.json"
    if not parent_cfg_path.is_file():
        raise SystemExit(f"the kit at {parent_kit} carries no agentic.config.json -- pass --from")
    if len(args.ports.split(",")) != 3:
        raise SystemExit("--ports takes three integers: gate,storage,retrieval")
    return mode_propose(args, repo, slug, parent_kit, json.loads(parent_cfg_path.read_text()))


if __name__ == "__main__":
    sys.exit(main())
