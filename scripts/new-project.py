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
                   console_container: str) -> dict:
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
        "gates": {**facts["gates"], **extra_gates},
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
        "artifacts": {
            "evidence_dir": f"~/{slug}-evidence",
            "briefs_dir": f"~/{slug}-evidence/briefs",
        },
    }


def apply_manifest(parent_cfg: dict, manifest: dict) -> tuple[dict, dict]:
    """The parent's seam table, re-pointed from the manifest. Returns (config, recorded_defaults)."""
    cfg = json.loads(json.dumps(parent_cfg))  # deep copy: never mutate the parent's table

    # 1. every declared seam, at the value the PARENT carries. Read BEFORE re-pointing anything:
    #    this becomes the parent's entry in the ancestry chain.
    parent_values = {k: dig(cfg, k) for k in SEAMS}
    parent_values = {k: v for k, v in parent_values.items() if v is not None}
    parent_review = {k: dig(cfg, k) for k in REVIEW_KEYS}
    parent_review = {k: v for k, v in parent_review.items() if v is not None}

    p, rt, pr = manifest["project"], manifest["runtime"], manifest["ports"]

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
    cfg["port"] = {
        "seams": list(SEAMS),
        "ancestors": [
            *parent_chain,
            {"name": p["parent"]["name"], "seams": parent_values, "review": parent_review},
        ],
    }
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
                                               "role_container_prefix")
        and v == parent_console.get(k))

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

    rt, p = manifest["runtime"], manifest["project"]
    parent_name = str(dig(parent_cfg, "project.name", ""))
    # The env file uses TWO names for the same project: the full slug, and a shorter one that
    # survives from how the broker and the state dir are named (`console-broker` -> `console`).
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
                              args.console_container or f"{slug}-console")
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

    cfg, recorded = apply_manifest(parent_cfg, manifest)
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
