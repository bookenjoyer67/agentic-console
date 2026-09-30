# Memory Architecture

## What this workflow needs to remember

This agent helps maintain **Komun** — a single-server mutual-aid web app (Rust/Axum server, SvelteKit
SPA, client-side crypto in WASM) — by running its pre-merge quality gate and keeping the prose docs in
sync with the code. Across sessions it needs to remember the current phase of that work, the decisions
that constrain future edits, the measured gate baselines that reveal a regression, known environment
quirks, and the questions that are still open because no line in the repository settles them. It does
not need to store information the repository already carries — file contents, test output, migration
text, or documentation that a person can read in place. It must not store the database URL or SMTP
credentials from `config.toml`, anything from `.env*`, session tokens, the `claude-auth` credential, and
never any key material or message plaintext (`AGENTS.md:134` — "**Never log** keys, bundles, passwords,
derived keys, or message plaintext"; the schema keeps message content server-unreadable,
`migrations/001_schema.sql:227`).

## Layer 1: Project memory directory

`.memory/project/` — what the agent has learned or decided across sessions. It reads the index on
startup and writes entries when state changes.

**Belongs here**

- The current phase of the work: Module 2 of the agentic-engineer course is building the memory system
  itself, alongside the capstone's pre-merge quality gate (2026-09-28).
- The measured gate baselines, with their date and the toolchain they came from: `cargo test
  --workspace` → 158 passed / 0 failed / 0 ignored, `cargo clippy --release -- -D warnings` → zero
  warnings, `npx vitest run` → 82 tests in 7 files, measured 2026-09-25 on `rustc 1.95.0` /
  `node v22.23.2` (`AGENTS.md:201-207`). A future session detects a regression by comparing against
  these numbers.
- Known environment quirks: the agent sandbox image carries no `python3`, so any Python-based harness
  needs an image rebuild (measured 2026-09-28); files the container writes into the mounted repo are
  root-owned on the host until they are chown'd (observed 2026-09-28).
- Standing constraints an edit must respect: `migrations/001_schema.sql` is frozen and schema changes
  are additive files (`AGENTS.md:111-117`); the build order is `wasm-pack` → `npm run build` →
  `cargo build` (`AGENTS.md:77-81`).
- Open questions, carried as state until someone rules on them: whether the intended term is
  "Svelte 5" or "SvelteKit 2" (`AGENTS.md:44-47`), and that the worked example at
  `docs/DOC-STYLE.md:38` and `:68` (`rg -c 'fetch\(' web/src -> 37`) does not reproduce — 47
  occurrences across 18 files, none above 18 (measured 2026-09-28).

**Does not belong here**

- The contents of the files those facts are about, or any test or command output — both are already in
  the repository, or reproducible from it, and storing them makes the entry rot the moment the code
  moves. Reason: memory is for what cannot be re-derived cheaply.
- Per-task notes once the task is finished ("edited line 42 of `post.rs`") — the lesson's own
  distinction holds: an audit log is not a memory system.
- Anything from `config.toml` or the environment files (`.env`, `.env.local`) (`AGENTS.md:59` `.env` / `.env.local`). Reason: secrets must never enter a layer
  that is designed to outlive the session and is designed to be committed.

- **Scope:** Project-scoped. This memory belongs to this repository only, and `.memory/SCOPE.md` names
  the project so a session can refuse to run against another project's memory.
- **Write permissions:** Agent-writable, but only through the index discipline in `CLAUDE.md`: check
  `MEMORY_INDEX.md` for an existing entry on the topic and update it rather than adding a duplicate. A
  human reviews entries before they are committed.
- **Pruning policy:** Entries tied to a feature branch are archived when that branch merges to `main`.
  Project-scoped entries (decisions, known quirks, current phase, gate baselines) are reviewed every
  90 days. Any entry a later entry supersedes is replaced by that entry in the same commit, never
  appended to.

## Layer 2: Knowledge files

`.memory/knowledge/` — stable rules a person maintains and the agent consults but never changes
(`chmod -R 444`, verified from inside the container in Step 6).

**Belongs here**

- The quality-gate standard: `cargo clippy --release -- -D warnings` must stay at zero warnings and
  `cargo test --workspace` must stay green (`AGENTS.md:197`).
- The documentation standard's authority rule: every claim carries a `path:line` containing the
  literal text it quotes, and a count carries the search that produced it (`docs/DOC-STYLE.md`).
- The frontend dialect rule: Svelte 5 runes only — `$state`, `$derived`, `$effect`, `$props`, with no
  `export let`, no `on:click` and no `$:` (`AGENTS.md:104-106`).
- The crypto boundary rule: no key material, key bundle, password, derived key or message plaintext is
  ever logged or written to any memory layer (`AGENTS.md:134`, `docs/CONVENTIONS.md:80-81`).
- The change-control rule: never edit `migrations/001_schema.sql`; add `002+` (`AGENTS.md:148`).

**Does not belong here**

- Project state that is supposed to change — the current phase, the current ticket, the latest
  measurement. Reason: this layer is the read-only, human-controlled half of the system; putting
  moving state in it forces the agent to treat a stale value as a standard.
- Anything the agent is expected to update during a session. Reason: a rule the agent can rewrite is
  not a guardrail, and the `444` permissions make that machine-enforced rather than advisory.
- Secrets, credentials and personal data, for the same reason as Layer 1.

- **Scope:** Project-scoped and human-authored. It encodes the rules that come from `AGENTS.md`,
  `docs/DOC-STYLE.md` and `docs/CONVENTIONS.md`, for this repository only.
- **Write permissions:** Human only. The agent is read-only, enforced by filesystem permissions rather
  than by instruction, and the enforcement is verified inside the sandbox container because the mount
  can change how host permissions arrive.
- **Pruning policy:** Reviewed every 90 days and whenever `AGENTS.md` or `docs/DOC-STYLE.md` changes. A
  rule that has moved into code, a test or a config default is deleted from this layer rather than
  duplicated, because the code is then the authority.

## Layer 3: Indexed reference documents

`.memory/reference/` — larger or more numerous documents that would be too expensive to load every
session. The agent queries this layer by keyword and reads one or two hits.

**Belongs here**

- The contract-audit reports under `docs/contract-audit/`, which record what an earlier audit found and
  why each finding was accepted or rejected (`AGENTS.md:149`).
- The design rationale in `docs/ARCHITECTURE.md`, `docs/CRYPTO.md` and `docs/DATABASE.md` — for
  example why `matches.message` was dropped in migration `003` when the column it replaced had been
  plaintext (`migrations/003_drop_matches_message.sql:22`).
- The iteration-log entries for earlier runs, which are the evidence behind the baselines in Layer 1
  (`docs/iteration-log.md`).
- Past pull-request descriptions, where they record a decision the diff alone does not explain.

**Does not belong here**

- Anything the agent needs on every session — the gate commands, the build order, the current phase.
  Reason: an indexed layer costs a search to use, so hot rules belong in Layer 2 or in `CLAUDE.md`.
- Generated output and build artifacts (`web/build/`, `crates/wasm/pkg/`), which are gitignored and
  reproducible. Reason: an index over reproducible artifacts is a second copy that drifts.

- **Scope:** Project-scoped, human-written, agent read-only.
- **Write permissions:** Human only. The agent consults it by keyword and must not read the whole
  directory (stated in `CLAUDE.md`, and pinned by the same `444` permissions as Layer 2).
- **Pruning policy:** An entry is removed when the artifact it indexes is deleted or superseded, and
  the layer is reviewed every 90 days together with Layer 1, because an index that points at a deleted
  document is the failure mode this layer introduces.

## Allocation decision table

| Information type | Where it goes | Reason |
|---|---|---|
| How to run the gate (`cargo test --workspace`, `cargo clippy --release -- -D warnings`, `npm run check`) | Skill / `AGENTS.md` "Tests" — not memory | Procedural steps are a procedure, not memory; they belong where the agent already looks for commands |
| The database URL and SMTP credentials (`config.toml`, `.env`) | Environment only, gitignored — no memory layer | Secrets must never be written into a layer designed to outlive the session and be committed |
| The file being edited right now | Context window only | No value once the session ends |
| The measured gate baselines, date-stamped (158 tests / zero warnings / 82 vitest) | Layer 1, project memory | A future session needs them to detect a regression; they are current state, not a procedure |
| Past contract-audit reports and the migration rationale for `003` | Layer 3, indexed reference | Too many and too large to load every session; retrieved by keyword on demand |
| A superseded design, e.g. the plaintext `matches.message` column | Layer 1 as current state ("dropped in 003"), Layer 3 for the old rationale | The agent must not act on the superseded design, but the reason it was dropped stays auditable |
| Key material, key bundles, passwords, recovery codes, message plaintext | Nowhere | Standing rule: never logged, never stored, in any layer |
| The memory scope declaration (`SCOPE.md`) | Layer 1, project memory — read-only to the agent | Identifies which project owns the mounted memory directory. Added after failure-mode testing: without it, memory mounted from another project looks identical to the right memory |
| Where the credential lives (`KOMUN_DATA_API_KEY`, the infra team's secret store) | Layer 1, project memory — the pointer only, never the value | Records how the credential is obtained without storing it. Added after failure-mode testing: the first version of this entry held the key itself |

## Data Classification

Before writing anything to a memory file, classify it:

- **Public** — Safe to commit to the repo and share broadly. Most project decisions and coding standards fall here.

- **Internal** — Safe within the team but not for public repos. Store in a non-committed volume or .gitignore the containing folder.

- **Confidential** — Sensitive business data. Do not store in agent memory. Retrieve from secure systems on demand.

- **Secret** — Credentials, tokens, API keys, PII. Must never appear in any memory file. If the agent encounters a secret during a run, use it for the immediate task only and explicitly do not write it to any memory layer. Reference the environment variable name instead.

### Guardrails

A pre-commit hook at `scripts/hooks/pre-commit` scans `.memory/` for common credential patterns (`sk-`, `password=`, `secret=`, `token=`, `api_key=`, `apikey=`) before each commit. If a pattern is found, the commit is blocked and nothing is written to history. The hook is versioned with the code, and `core.hooksPath` points at `scripts/hooks/` so Git runs it from there: a fresh clone arms it with one command, `git config core.hooksPath scripts/hooks`. That indirection is deliberate — a hook inside `.git/` cannot be reviewed in a diff and dies with the clone — but it is also not self-arming, which is exactly why the classification policy in `CLAUDE.md` does not depend on this hook being present.

## Enforcement

- Scope: each `.memory/` root carries a `SCOPE.md` declaring its owning repo. The agent reads it on startup and halts on a mismatch. **Soft guard** — a policy in `CLAUDE.md`, executed only if the agent obeys.
- Write permissions: `knowledge/` and `reference/` are read-only to the agent; `project/` is read-write; credentials are never written, and a pointer to the environment variable name is stored instead. **Soft guard** in `CLAUDE.md`, backed by a **hard stop**: a `PreToolUse` hook (`.claude/hooks/guard-readonly-memory.sh`) denies any write to the read-only layers, which matters because the container's agent runs as root and root ignores file permissions outright.
- Stale entries: review dates are checked before use; an entry whose review date has passed is flagged and acted on only after human confirmation. **Soft guard** — nothing executes it but the agent's compliance, so it depends on the entry being in view, which the startup hook guarantees.
- Secrets at commit: the pre-commit hook blocks commits matching common credential patterns. **Hard stop** — it runs at a defined point, after staging and before the commit object exists, independently of anything the agent decided or believed.

Soft guards depend on the model obeying the instruction; hard stops are executed by something other than the model. The distinction was not theoretical here: file permissions were the intended hard stop for the read-only layers, and they did not bind the agent at all.

## Why these safeguards exist

### Scope verification

**Problem:** memory mounted from another project is indistinguishable from the correct memory — same directory layout, same file names, same entry numbering. Nothing about the content announces that it belongs to a different codebase.

**Observed during testing:** with a second project's memory mounted over `.memory/`, the agent summarised that project's decisions as if they applied to this repository, and the mounted decision contradicted the migration rule this repo actually follows. With `SCOPE.md` present and the check in `CLAUDE.md`, a fresh session halted and reported the mismatch instead of answering from the wrong memory.

**Change made:** `SCOPE.md` at each memory root, a halt-on-mismatch rule in `CLAUDE.md`, and the memory-loading hook printing the scope declaration into every fresh session — so the check cannot be skipped by simply not reading the file.

### Stale memory

**Problem:** an entry whose review date has passed reads exactly like a current one. The index can also disagree with the entry about the date, which hides the staleness further.

**Observed during testing:** a decision entry backdated 58 days, containing a claim about a vector index that was never built, was flagged on the next session. The agent cited the entry's own review date, proved the claim false (no such file existed anywhere in the image), and asked for confirmation before acting — it did not silently apply the stale content.

**Change made:** the stale-memory policy in `CLAUDE.md` requires flagging any entry past its review date and human confirmation before acting on it. The startup hook injects the entries themselves, so the review date is always in front of the agent rather than sitting in a file it might not open.

### Write policy and data classification

**Problem:** anything that writes memory — agent or human — can put a credential into a file whose whole purpose is to be committed and to outlive the session.

**Observed during testing:** asked to record a decision containing a fake API key, the agent refused and wrote a redaction note in its place, citing the write policy and the coding standard on secret material. When the value was planted by hand instead, the pre-commit hook blocked the commit.

**Change made:** an explicit four-level classification check placed first in the write policy (above the existing write rules), plus the Git hook as the enforcement point for content that reaches staging anyway.

### Pre-commit hard stop

**Problem:** a soft guard is exactly as reliable as the model's willingness to comply, and permissions are not a hard stop when the agent runs as root.

**Observed during testing:** the read-only permission bits on the knowledge layer were set as prescribed and did not stop a root write; the `PreToolUse` hook did, on a run where the rule had been withdrawn and the edit pre-authorised, leaving only the hook to stop it.

**Change made:** both the Git hook and the `PreToolUse` hook act at defined points outside the model's decision, which is what makes them hard stops rather than stronger wording.

## Alternatives considered

**1. Keep everything in `CLAUDE.md`, with no `.memory/` tree.** Rejected. `CLAUDE.md` is read at the
start of every session whether or not its content is relevant, which is the "too much memory" failure
named in the lesson, and this repository already has evidence of the cost of one large context file: in
the Module 2.2 run on 2026-09-26 the session compacted from 166,700 tokens to 12,837, and the generated
summary dropped the unresolved-questions field entirely — the one field a future session most needs. A
small index plus opt-in reference layers keeps the startup read small and keeps the unresolved questions
in a file that is read deliberately.

**2. Put `.memory/` outside the repository, in a mounted volume.** Rejected for this system, and the
repository's own history argues against it: the 2.2 run's evidence is citable only because the
artifacts sit in the working tree with commit SHAs behind them (`docs/iteration-log.md:694`). Memory
kept outside version control cannot be diffed against the code whose behaviour it explains, and a mount
set up wrongly is exactly the scope-leak failure mode the next lesson drills. The cost of this choice
is real and accepted: entries must be reviewed before every commit, which is why the write policy in
`CLAUDE.md` requires it.

**3. A vector store or embedded retrieval database for the reference layer.** Deferred, not rejected.
Module 3 requires an MCP-backed retrieval tool, so the retrieval layer will be built there with its own
evaluation; building it now would mean maintaining two retrieval paths. There is also a concrete
blocker today: the agent sandbox image carries no `python3`, so a Python MCP server needs an image
rebuild before it can run at all (measured 2026-09-28). Until then the reference layer is greppable
markdown, which needs no embedding model and no extra dependency.

**4. Store every decision as an append-only log.** Rejected on the lesson's own distinction — an audit
log is not a memory system. Layer 1 holds current state, and a superseding entry replaces the entry it
supersedes in the same commit; the history of why a decision changed lives in Layer 3 and in the
repository's own commit log, where it is already complete.
