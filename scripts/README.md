# Per-role agent launcher

What does `./scripts/run-agent.sh` do, and where do its permissions come from?

`./scripts/run-agent.sh <role> <command>` starts one container for one governed role, then runs the command
inside it. It is the Module 4.1 enforcement layer at the container boundary: the engine stays
`sandbox/run-agent.sh`, the Module 3 wrapper stays `sandbox/run-agent-m3.sh`, and this wrapper varies the
mount set per role.

## Invocation

How is the launcher invoked, and what does it refuse?

- Read `<role>` from the seven governed roles (`scripts/run-agent.sh:33` `VALID_ROLES="orchestrator planner implementer tester reviewer project-manager researcher"`).
- Read `<command>` from the four supported commands (`scripts/run-agent.sh:34` `VALID_CMDS="bash sh claude opencode"`).
- Refuse an unknown role on stderr, naming every valid role, with exit status 2 (`scripts/run-agent.sh:122` `printf 'valid roles: %s\n\n' "$VALID_ROLES" >&2`).
- Name the container `agent-rev-m4-<role>`, so two roles run side by side (`scripts/run-agent.sh:144` `NAME="${NAME:-agent-rev-m4-$ROLE}"`).
- Print the role, image, workspace mode and memory mode on every start, read back from the container (`scripts/run-agent.sh:272` `printf 'workspace : %s -> %s (%s)\n' "$REPO" "$WORKSPACE" "$eff_ws"`).
- Print the matrix with `./scripts/run-agent.sh --matrix`, which needs no Docker daemon (`scripts/run-agent.sh:115` `--matrix)  print_matrix; exit 0 ;;`).
- Reuse a running container whose mounts already match the role, instead of failing on a taken name (`scripts/run-agent.sh:230` `printf 'container %s is already running with the %s profile — reusing it\n' "$NAME" "$ROLE"`).

## Permission matrix

Which permissions does each role get?

Each role's container gets the modes below; the reason cell quotes the policy literal that settles the mode.
`./scripts/run-agent.sh --matrix` prints this table from the launcher itself (`scripts/run-agent.sh:82` `printf '%s\n' "$MATRIX_ROWS"`).

| Role | Workspace mount | Memory mount | Network | Reason |
|---|---|---|---|---|
| `orchestrator` | `/workspace` read-write | `/workspace/.memory` read-only | agent-internal + agent-net (broker only) | Policy grants workspace writes and no memory write (`docs/governance-policy.md:67` `holds no memory write grant`). |
| `planner` | `/workspace` read-only | `/workspace/.memory` read-write (nested bind over the read-only workspace) | agent-internal + agent-net (broker only) | Policy grants one plan entry and denies a workspace write (`docs/governance-policy.md:110` `writes one plan entry into`). |
| `implementer` | `/workspace` read-write | `/workspace/.memory` read-write | agent-internal + agent-net (broker only) | Policy grants workspace writes and memory entry writes (`docs/governance-policy.md:155` `writes and revises its own entries in`). |
| `tester` | `/workspace` read-only | `/workspace/.memory` read-write (nested bind over the read-only workspace) | agent-internal + agent-net (broker only) | Policy grants one result entry and denies a workspace write (`docs/governance-policy.md:202` `writes one test-result entry into`). |
| `reviewer` | `/workspace` read-only | `/workspace/.memory` read-write (nested bind over the read-only workspace) | agent-internal + agent-net (broker only) | Policy grants one review entry and denies a workspace write (`docs/governance-policy.md:248` `writes one review entry into`). |
| `project-manager` | `/workspace` read-only | not mounted (visible read-only through the workspace bind) | agent-internal + agent-net (broker only) | Policy grants memory reads and no write (`docs/governance-policy.md:292` `reads stored entries from`). |
| `researcher` | `/workspace` read-only | `/workspace/.memory` read-write (nested bind over the read-only workspace) | agent-internal + agent-net (broker only) | Policy grants one research entry and denies a repository read (`docs/governance-policy.md:337` `writes one `public` research entry into`). |

- Derive the memory mode from the grant map rather than from the workspace mode (`docs/routing-and-tool-grant-map.json:17` `"mcp__storage__write_entry"`): planner, implementer, tester, reviewer and researcher hold that grant, the orchestrator and the project-manager hold none.
- Read the workspace mode off the deny in each policy entry (`scripts/run-agent.sh:95` `planner)         ROLE_WS=ro; ROLE_MEM=rw;   ROLE_TARGET=ro ;;`), so the five roles that write no repository file get `:ro`.
- Mount the memory layer as a nested bind inside the read-only workspace (`scripts/run-agent.sh:168` `MOUNTS+=(-v "$REPO/.memory:/workspace/.memory")`), so a granted entry write has a path while the repository stays read-only.
- Read the write-cache line as the third variation: `/workspace/target` is read-write for the tester alone (`scripts/run-agent.sh:97` `tester)          ROLE_WS=ro; ROLE_MEM=rw;   ROLE_TARGET=rw ;;`).
- Overlay the grant authority and the audit journals read-only over the workspace (`scripts/run-agent.sh:187` `declare -a OVERLAY_FILES=(`), so a writable memory layer cannot rewrite the files a grant is read from.
- Treat the mount as the enforcement, not a file mode, because the container runs as root and root ignores a read-only bit (`docs/memory-architecture.md:196` `did not stop a root write`).
- Keep both Module 3 networks for every role (`sandbox/run-agent-m3.sh:31` `docker network inspect agent-internal >/dev/null 2>&1 || { docker network create --internal agent-internal; }`).
- Keep the credential broker pattern, so no container holds a key (`sandbox/run-agent.sh:91` `-e ANTHROPIC_AUTH_TOKEN=sandbox-dummy-token \`).
- Adapt the lesson's `/memory` mount to this repository's layout (`sandbox/README-m3.md:54` `the memory directory stays inside the repo at /workspace/.memory`; `docs/governance-policy.md:16` `as absent in this container`).

State the layout deviation plainly: this launcher mounts `/workspace/.memory`, and the image has no
`/memory`. The lesson's check still passes, because it asks whether `/memory` is mounted, and it is not
(`docker exec agent-rev-m4-reviewer sh -c "grep -q ' /memory ' /proc/mounts || echo 'OK: /memory is not mounted'"` -> `OK: /memory is not mounted`).

## Proof commands

Which commands prove the permission difference, and what does each print?

```bash
./scripts/run-agent.sh --matrix                                             # the table above
./scripts/run-agent.sh janitor bash                                          # (f) unknown role
./scripts/run-agent.sh reviewer bash -c 'touch /workspace/should-fail.txt'    # (a) read-only workspace
docker inspect agent-rev-m4-reviewer -f '{{range .Mounts}}{{if or (eq .Destination "/workspace") (eq .Destination "/workspace/.memory")}}{{.Destination}} RW={{.RW}} source={{.Source}}
{{end}}{{end}}'                                                              # (b) the two mounts, one per line
./scripts/run-agent.sh reviewer bash -c 'touch /workspace/.memory/reviewer-entry-probe.txt && ls -l /workspace/.memory/reviewer-entry-probe.txt && rm /workspace/.memory/reviewer-entry-probe.txt && echo "reviewer memory entry write+remove OK"'   # (b)
./scripts/run-agent.sh planner bash -c 'touch /workspace/.memory/planner-entry-probe.txt && rm /workspace/.memory/planner-entry-probe.txt && echo "planner memory entry write+remove OK"'       # (c)
./scripts/run-agent.sh tester bash -c 'touch /workspace/.memory/tester-entry-probe.txt && rm /workspace/.memory/tester-entry-probe.txt && echo "tester memory entry write+remove OK"'         # (c)
./scripts/run-agent.sh researcher bash -c 'touch /workspace/.memory/researcher-entry-probe.txt && rm /workspace/.memory/researcher-entry-probe.txt && echo "researcher memory entry write+remove OK"'   # (c)
./scripts/run-agent.sh implementer bash -c 'touch /workspace/ok-to-write.txt && ls -l /workspace/ok-to-write.txt && rm /workspace/ok-to-write.txt && echo "workspace write+remove OK"'   # (d)
./scripts/run-agent.sh implementer bash -c 'touch /workspace/.memory/implementer-entry-probe.txt && rm /workspace/.memory/implementer-entry-probe.txt && echo "implementer memory entry write+remove OK"'   # (d)
./scripts/run-agent.sh orchestrator bash -c 'touch /workspace/.memory/orchestrator-should-fail.txt'   # (e) read-only memory
./scripts/run-agent.sh project-manager bash -c 'touch /workspace/.memory/pm-should-fail.txt'          # (e) no memory mount
```

### Proof (a) — the reviewer's workspace

Does the reviewer's workspace refuse a write?

Yes, even for the root user inside the container.

```
$ ./scripts/run-agent.sh reviewer bash -c 'touch /workspace/should-fail.txt'
role      : reviewer
image     : agent-sandbox:komun-m3
container : agent-rev-m4-reviewer
networks  : agent-internal (no egress) + agent-net (broker rev-broker:4000 only)
workspace : /home/computing/rev -> /workspace (read-only)
memory    : /workspace/.memory (mounted read-write)
cache     : rev-cargo-target -> /workspace/target (ro)
command   : docker exec -w /workspace agent-rev-m4-reviewer bash
touch: cannot touch '/workspace/should-fail.txt': Read-only file system
```

### Proof (b) — the reviewer's memory entry path

Which path does the reviewer's granted review entry write land on?

`/workspace/.memory`, mounted read-write as a nested bind inside the read-only `/workspace`.

```
$ docker inspect agent-rev-m4-reviewer -f '{{range .Mounts}}{{if or (eq .Destination "/workspace") (eq .Destination "/workspace/.memory")}}{{.Destination}} RW={{.RW}} source={{.Source}}
{{end}}{{end}}'
/workspace RW=false source=/home/computing/rev
/workspace/.memory RW=true source=/home/computing/rev/.memory

$ docker exec agent-rev-m4-reviewer sh -c "grep ' /workspace/.memory ' /proc/mounts"
/dev/nvme0n1p2 /workspace/.memory btrfs rw,noatime,compress=zstd:1,ssd,discard=async,space_cache=v2,subvolid=257,subvol=/@home 0 0

$ ./scripts/run-agent.sh reviewer bash -c 'touch /workspace/.memory/reviewer-entry-probe.txt && ls -l /workspace/.memory/reviewer-entry-probe.txt && rm /workspace/.memory/reviewer-entry-probe.txt && echo "reviewer memory entry write+remove OK"'
container agent-rev-m4-reviewer is already running with the reviewer profile — reusing it
role      : reviewer
...
-rw-r--r-- 1 root root 0 Sep 28 19:10 /workspace/.memory/reviewer-entry-probe.txt
reviewer memory entry write+remove OK
```

The probe removes what it wrote, so the layer carries no test file afterwards
(`find .memory -maxdepth 1 -name '*probe*'` -> no output).

### Proof (c) — the other three read-only roles' granted entry writes

Do the planner, tester and researcher keep a read-only workspace and a writable memory path?

Yes to both, and each removes what it wrote.

```
$ ./scripts/run-agent.sh planner bash -c 'touch /workspace/should-fail.txt'
touch: cannot touch '/workspace/should-fail.txt': Read-only file system
$ ./scripts/run-agent.sh planner bash -c 'touch /workspace/.memory/planner-entry-probe.txt && ls -l /workspace/.memory/planner-entry-probe.txt && rm /workspace/.memory/planner-entry-probe.txt && echo "planner memory entry write+remove OK"'
-rw-r--r-- 1 root root 0 Sep 28 19:10 /workspace/.memory/planner-entry-probe.txt
planner memory entry write+remove OK

$ ./scripts/run-agent.sh tester bash -c 'touch /workspace/should-fail.txt'
touch: cannot touch '/workspace/should-fail.txt': Read-only file system
$ ./scripts/run-agent.sh tester bash -c 'touch /workspace/.memory/tester-entry-probe.txt && ls -l /workspace/.memory/tester-entry-probe.txt && rm /workspace/.memory/tester-entry-probe.txt && echo "tester memory entry write+remove OK"'
-rw-r--r-- 1 root root 0 Sep 28 19:10 /workspace/.memory/tester-entry-probe.txt
tester memory entry write+remove OK

$ ./scripts/run-agent.sh researcher bash -c 'touch /workspace/should-fail.txt'
touch: cannot touch '/workspace/should-fail.txt': Read-only file system
$ ./scripts/run-agent.sh researcher bash -c 'touch /workspace/.memory/researcher-entry-probe.txt && ls -l /workspace/.memory/researcher-entry-probe.txt && rm /workspace/.memory/researcher-entry-probe.txt && echo "researcher memory entry write+remove OK"'
-rw-r--r-- 1 root root 0 Sep 28 19:10 /workspace/.memory/researcher-entry-probe.txt
researcher memory entry write+remove OK
```

The writable memory bind narrows nothing outside its own path: each of the three prints
`workspace : /home/computing/rev -> /workspace (read-only)` on the same launch.

### Proof (d) — the implementer's writes

Does the implementer write the workspace and reach a writable memory layer?

Yes to both, and it removes what it wrote.

```
$ ./scripts/run-agent.sh implementer bash -c 'touch /workspace/ok-to-write.txt && ls -l /workspace/ok-to-write.txt && rm /workspace/ok-to-write.txt && echo "workspace write+remove OK"'
role      : implementer
image     : agent-sandbox:komun-m3
container : agent-rev-m4-implementer
networks  : agent-internal (no egress) + agent-net (broker rev-broker:4000 only)
workspace : /home/computing/rev -> /workspace (read-write)
memory    : /workspace/.memory (mounted read-write)
cache     : rev-cargo-target -> /workspace/target (ro)
command   : docker exec -w /workspace agent-rev-m4-implementer bash
-rw-r--r-- 1 root root 0 Sep 28 19:10 /workspace/ok-to-write.txt
workspace write+remove OK

$ ./scripts/run-agent.sh implementer bash -c 'touch /workspace/.memory/implementer-entry-probe.txt && ls -l /workspace/.memory/implementer-entry-probe.txt && rm /workspace/.memory/implementer-entry-probe.txt && echo "implementer memory entry write+remove OK"'
-rw-r--r-- 1 root root 0 Sep 28 19:10 /workspace/.memory/implementer-entry-probe.txt
implementer memory entry write+remove OK
```

### Proof (e) — the two roles with no memory write grant

Do the orchestrator and the project-manager stay out of the memory layer?

Yes: the orchestrator writes the workspace and finds `/workspace/.memory` read-only, and the
project-manager finds no memory mount at all.

```
$ ./scripts/run-agent.sh orchestrator bash -c 'touch /workspace/orchestrator-ok.txt && rm /workspace/orchestrator-ok.txt && echo "workspace write+remove OK"'
workspace : /home/computing/rev -> /workspace (read-write)
memory    : /workspace/.memory (mounted read-only)
workspace write+remove OK
$ ./scripts/run-agent.sh orchestrator bash -c 'touch /workspace/.memory/orchestrator-should-fail.txt'
touch: cannot touch '/workspace/.memory/orchestrator-should-fail.txt': Read-only file system

$ ./scripts/run-agent.sh project-manager bash -c 'touch /workspace/should-fail.txt'
workspace : /home/computing/rev -> /workspace (read-only)
memory    : /workspace/.memory (not mounted (visible read-only through the /workspace bind))
touch: cannot touch '/workspace/should-fail.txt': Read-only file system
$ ./scripts/run-agent.sh project-manager bash -c 'touch /workspace/.memory/pm-should-fail.txt'
touch: cannot touch '/workspace/.memory/pm-should-fail.txt': Read-only file system
```

### Proof (f) — an unknown role

Does an unknown role get a clear refusal?

Yes, with the valid roles named on stderr and exit status 2.

```
$ ./scripts/run-agent.sh janitor bash
error: unknown role "janitor"

valid roles: orchestrator planner implementer tester reviewer project-manager researcher

usage: ./scripts/run-agent.sh <role> <command>

  <role>     orchestrator planner implementer tester reviewer project-manager researcher
  <command>  bash sh claude opencode

examples:
  ./scripts/run-agent.sh reviewer bash
  ./scripts/run-agent.sh implementer bash -c "touch /workspace/ok-to-write.txt"
  ./scripts/run-agent.sh --matrix
$ echo $?
2
```

## Re-running the launcher

Does a second launch of the same role fail on the taken container name?

No. The launcher reuses the running container whose mounts match the role profile.

```
$ ./scripts/run-agent.sh reviewer bash -c 'touch /workspace/should-fail.txt'
container agent-rev-m4-reviewer is already running with the reviewer profile — reusing it
role      : reviewer
...
touch: cannot touch '/workspace/should-fail.txt': Read-only file system
```

## Policy disagreements

Where does the container layer disagree with the policy document?

The one conflict this launcher carried is closed, and `docs/policy-reconciliation.md` records it as `C5`
with the side fixed for each of its two halves.

- Replace the shared opening sentence with a per-role mount statement, so each entry names its own workspace mode (`docs/governance-policy.md:248` ``the launcher mounts this repository read-only at `/workspace` for this role``).
- Cite the launcher's own mount argument as that statement's authority (`docs/governance-policy.md:110` `MOUNTS+=(-v "$REPO:/workspace:ro")`), so the policy and the artifact read from one source.
- Mount the memory layer read-write for the four roles whose granted entry write had no path (`scripts/run-agent.sh:168` `MOUNTS+=(-v "$REPO/.memory:/workspace/.memory")`), and keep the rest of their workspace read-only.
- Keep the granted write in the policy, because the grant map grants it (`docs/routing-and-tool-grant-map.json:17` `"mcp__storage__write_entry"`) and the map is the decision of record.
- Record the fifth conflict with its fixed side (`docs/policy-reconciliation.md` `C5`), because the Module 4.1 checklist resolves a mismatch by moving one side and naming it.

Two facts about the resolution are worth stating plainly. First, the writable memory bind sits inside a
read-only workspace, which docker honours because it sorts the two binds by destination depth
(`docker exec agent-rev-m4-reviewer sh -c "grep ' /workspace/.memory ' /proc/mounts"` -> `/dev/nvme0n1p2 /workspace/.memory btrfs rw,...`). Second, the
read-only `.memory/knowledge` and `.memory/reference` layers stay enforced by the `PreToolUse` hook rather
than by the mount, because the container user is root (`docs/memory-architecture.md:160` `denies any write to the read-only layers`).

## Claims needing verification

Which claims in this file does no run here settle?

- Verify the tester's gate run under a read-only workspace, because `cargo` writes `Cargo.lock` in some states.
- Verify the git worktree case, which this launcher omits (`sandbox/run-agent.sh:76` `GIT_MOUNT=()`).
- Verify the in-container egress and broker probe for these containers, because the run that would have measured it was denied.
- Verify that a real `mcp__storage__write_entry` call lands, because the probes here write a file into `/workspace/.memory` and call no MCP tool.
- Verify that the storage server can still append to `.memory/storage-audit.log`, which the launcher overlays read-only (`scripts/run-agent.sh:192` `.memory/storage-audit.log`).
