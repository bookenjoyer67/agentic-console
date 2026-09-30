# Project memory index

Read this file at the start of every session, after `.memory/SCOPE.md`. It is the load manifest: the
SessionStart hook (`.claude/hooks/load-memory.sh`) inlines every entry listed under **Active entries**,
so an entry becomes part of the startup context exactly when it is registered here.

Before writing a new entry, check this index for one on the same topic and update that entry instead of
adding a duplicate. An entry whose review date has passed is flagged to the human before it is acted on.

## Active entries

- `decisions/decision-001-project-id.md` — this repository's project id is `proj-console`, in the config, the corpus and the storage calls; review 2026-12-29
- `phase-001-fork-port-state.md` — what the fork has ported and what it has not, as of 2026-09-30; review 2026-10-28

## Archived entries

(none)
