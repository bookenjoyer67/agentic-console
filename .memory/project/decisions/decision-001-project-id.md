# Decision 001 — this repository's project id is `proj-console`

**Type:** decision · **Classification:** internal · **Recorded:** 2026-09-30 · **Review:** 2026-12-29

## The decision

Every project-scoped call in this repository — storage reads and writes, retrieval queries, the grant
map, the corpus front matter — uses the project id **`proj-console`**.

`proj-komun` is not this repository's id. It is the reference project's, and it survives here only in
artifacts that were not re-pointed when the fork was made.

## Why this is recorded rather than assumed

At the time of writing the tree carried a three-way disagreement:

| artifact | says |
|---|---|
| `agentic.config.json:206` `"project_key": "proj-console"` | `proj-console` |
| `CLAUDE.md:101` — the plan is written "under project `proj-console`" | `proj-console` |
| `.memory/reference/*.md` front matter — `project: proj-console`, all seven documents | `proj-console` |
| `docs/routing-and-tool-grant-map.json:2` `"project": "proj-komun"` | `proj-komun` |
| six of the seven `.claude/agents/*.md` — `project_id: "proj-komun"` | `proj-komun` |

`agentic.config.json` decides it, because it is the single seam table this repository is built around
and every other consumer is required to follow it. The corpus already follows it. The grant map and the
role definitions are the artifacts that are behind.

## Consequence, so the next reader does not have to rediscover it

A storage entry written under `proj-komun` is invisible to a run that reads under `proj-console`, and
the retrieval server filters every query by project id, so a query scoped to the wrong id returns
nothing rather than returning the wrong document. Both failures are silent.

## What is not decided here

Which of the two port systems other than the config should follow — the container name and the console
ports. Those are open, and they are recorded as open in `phase-001-fork-port-state.md`.
