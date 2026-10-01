# Phase 001 — what this fork has ported, and what it has not

**Type:** phase · **Classification:** internal · **Recorded:** 2026-09-30 · **Review:** 2026-10-28 · **Updated:** 2026-10-01

## The current phase

The repository is a fork of an agentic quality-gate kit. Its own fork check passes
(`bash scripts/port-self-test.sh --fork` → `port-self-test: PASS`), its suites pass, and its CI runs.
The work in progress is closing the gap between **the machinery, which was ported**, and **the
governance, which was not**.

## Ported and verified

- The gate vocabulary: eight named commands in `agentic.config.json` `toolchain.commands`, read by
  `mcp/gate/gate_vocabulary.py`, run by name through the gate server's `run_gate` / `run_fix`.
- The clippy cache-hit guard, re-pointed to this crate: the marker is `Checking agentic-console` and
  the touched file is `src/main.rs` (`agentic.config.json:42` `"marker": "Checking agentic-console",`).
- The container runtime under `sandbox/`: audited by hand, file by file, and re-pointed.
- The inference: seven documents under `.memory/reference/`, all carrying `project: proj-console`, and
  the retrieval harness that scores 8 of 8 against them above its 80 percent floor.
- The console: it reads `agentic.config.json`, points at any repository with `--repo`, and labels each
  seam as this repository's or still the reference project's.

## Not ported — open, and each one is a finding

1. **Six of the seven role definitions.** They are the reference project's rules under this
   repository's name. Their `AGENTS.md:NNN` citations point past the end of this repository's
   `AGENTS.md` (166 lines as committed), and they enforce rules for a schema, a frontend and a wasm
   package that do
   not exist here. Each one is recorded as a seam below.
2. **A project-id split.** Settled for the config, `CLAUDE.md` and the corpus by
   `decisions/decision-001-project-id.md`; the grant map and the role files are still behind.
3. **The console ports.** `console.ports` says gate 8101, storage 8102, retrieval 8103. The servers
   listen on 8003, 8001 and 8002, and no launcher publishes a port to the host. The console's own
   embedded fallback carries 8003/8001/8002 as well, so the configured ports are the outlier rather
   than one side of an ambiguity. `src/actions.rs`
   derives the gate selftest's argv from those config ports, so the disagreement decides behaviour.
4. **The container name.** `console.container` says `agent-console-m1`. `sandbox/run-agent.sh` derives
   `agent-$SLUG` from the workspace directory, giving `agent-agentic-console`. One fact, two sources.
5. **The role container prefix.** `console.role_container_prefix` says `agent-console-m4-`;
   `scripts/run-agent.sh:144` names a role box `agent-rev-m4-$ROLE`. The console's role boxes can never
   light, because the container it looks for is never the container the launcher makes.
6. **The registration file.** `.mcp.json` is not in this repository. A container started from a fresh
   clone therefore registers no MCP server, and a run against it would leave no journal row at all.
7. **`PORTING.md`.** Named four times — `agentic.config.json:10`, `scripts/agentic_config.py:59`,
   `scripts/run-agent.sh:12`, `docs/iteration-log.md:172` — and absent.

## Closed since this record — 2026-10-01

- **Item 1, the six role definitions.** Ported to this repository in `b01b3da`. Their `AGENTS.md:NNN`
  citations now land inside this repository's own `AGENTS.md` (highest cited line 162 of a 177-line file,
  against 166 as committed when this record was written), their `project_id` is `proj-console`, and the
  commands whose argv needs a frontend this repository does not carry are documented as such
  (`tester.md:7`, `tester.md:55`) rather than assumed.
- **Item 2, the project-id split.** Closed with them: the grant map now carries this repository's id
  (`docs/routing-and-tool-grant-map.json` `"project": "proj-console"`; `docs/routing-and-tool-grant-map.md`
  lines 3 and 5), so the config, `CLAUDE.md`, the corpus, the role definitions and the grant map all
  agree, as `decisions/decision-001-project-id.md` requires. `proj-komun` survives only in records —
  the fork-proof table, the ancestry entry in `agentic.config.json`, the red-team corpus, the schema
  examples and the decision record itself.
- **The blind spot below, its first half.** `scripts/port-self-test.sh` now sweeps the role definitions
  with the other wiring files (46 files), which is exactly what surfaced the 41 `proj-komun` seams in
  `.claude/agents/*.md`. The conformance gate still does not resolve citations inside them.

## What is deliberately not open

`.memory/reference/` is the corpus the retrieval server indexes, and its front matter is the schema the
server reads. It is committed on purpose. `.memory/storage.db` and the three audit logs are run output
and are gitignored.

## The blind spot that let the above survive

Two checks exist to catch exactly this class of drift, and neither covers the artifacts that carry it.
`scripts/port-self-test.sh` sweeps a named list of consumer files, and `.claude/agents/*.md` is not on
it. The conformance gate resolves citations for a configured file list, and `.claude/agents/*.md` is not
in that list either (`agentic.config.json` `gates.conformance.files`). The role definitions are the only
governed artifacts in this repository that no instrument reads.
