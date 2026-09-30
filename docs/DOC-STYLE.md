# Documentation standard

Frozen 2026-09-26 for the 2.2 managed context run. This file is the rule set the `komun-docs-stylist`
agent applies to the repository's prose. It is versioned: a revision made inside a session is a new
version number plus a dated entry in the history at the bottom — never an in-place edit of an older
version's text.

Scope of a run: the files named in the run instruction, section by section. This run covers `AGENTS.md`
and `docs/DEVELOPMENT.md`.

## What counts as a claim

A claim is a sentence that asserts something about the repository: what the code, schema, config or
scripts do, what a file contains, what a command prints, or what must never be committed. Every claim
needs authority (R2). A sentence that only describes the document itself is not a claim.

## Rules (v2)

Which rules govern prose from 2026-09-26 onward, and how is each one detected?

v2 is the current rule set. The v1 table below is kept exactly as written, because a revision here is
a new version and never a rewrite of an older one.

| Rule | Requirement | How a violation is detected |
|---|---|---|
| R1 | A section opens with the question it answers. An existing "This section …" purpose sentence is deleted. | the first line after the heading is not a question, or opens with "This section" |
| R2 | Every claim carries its authority in parentheses, naming the artifact **and** the literal text, value or count that settles it. | a parenthetical that names a file, command or search but quotes nothing from it |
| R3 | A sentence is at most 35 words. | word count of the text between two sentence terminators |
| R4 | A rule is an imperative, and never hedges (`should`, `might`, `probably`, `may want to`). | a bullet whose first word is not a verb |
| R5 | Withdrawn in v2: nesting is allowed where it shows real hierarchy. | — |

### Authority forms (v2)

What has to be inside a v2 parenthetical?

- a location and the text at it — (`crates/server/src/api/mod.rs:44` `.merge(search::router(state.clone()))`)
- a command and its output — (`cargo test --workspace` -> `158 passed, 0 failed`)
- a search and its count — (`rg -c 'fetch\(' web/src` -> `37`)

A bare location is a v1 form and fails R2 in v2: `(crates/server/src/api/mod.rs:44)` names an artifact
but quotes nothing from it. The section "A claim you cannot trace" below is unchanged and still
applies in full.

### Rulings carried into v2

Which earlier rulings survive the version bump?

- Open every section with its question, `###` headings included.
- Write rule bullets and rules in prose as verb-initial imperatives.
- Leave a bullet that states a fact in its existing shape, and add R2 authority to it.

## Rules (v1)

| Rule | Requirement | How a violation is detected |
|---|---|---|
| R1 | Every section opens with one purpose sentence. | the first line after the heading starts with "This section" |
| R2 | Every claim carries its authority in parentheses: a `path:line`, or a backticked command with its output. | a claim sentence with no parenthesised authority |
| R3 | A sentence is at most 25 words. | word count of the text between two sentence terminators |
| R4 | A rule is an imperative, and never hedges (`should`, `might`, `probably`, `may want to`). | a bullet whose first word is not a verb |
| R5 | Lists are flat: no nested bullets. | any bullet indented under another bullet |

## Authority forms

Use the shortest form that settles the claim:

- a location — `(crates/server/src/api/mod.rs:41)`
- a command and its result — `(cargo test --workspace -> 158 passed, 0 failed)`
- a count with the search that produced it — `(rg -c 'fetch\(' web/src -> 37)`

## A claim you cannot trace

Do not delete the claim, and do not invent authority for it. Mark the sentence with `[UNVERIFIED]`,
and add it to a `Claims needing verification` list at the end of that section, naming what would settle
it. That list is part of the run's evidence and the session summary must carry it forward verbatim.

## Version history

| Version | Date | Change |
|---|---|---|
| v1 | 2026-09-26 | Frozen before run 001 of the managed context run: R1–R5 above. |
| v2 | 2026-09-26 | R1 replaced: a section opens with the question it answers, and an existing "This section …" purpose sentence is deleted. R2 strengthened: the parenthetical names the artifact and the literal text, value or count that settles the claim. R3 limit raised from 25 words to 35. R4 unchanged. R5 withdrawn: nesting is allowed where it shows real hierarchy. Q1 and Q2 rulings from run 001 carried forward, with the question opener replacing R1's purpose sentence. |
