# Rubric — Clippy Lint Gate

Derived from the acceptance criteria in `docs/clippy-gate/prd.md`. Each rubric dimension corresponds
to one graduable criterion; the two criteria that have only two meaningful states stay binary gates.

Binary gates (not dimensions — each is either satisfied or it is not):

- **G1 — Containment (from AC5).** No path under `/workspace` other than `docs/clippy-report.md` was
  created, modified, deleted or moved by the run, and no commit, push, install, dependency edit or
  `#[allow]` insertion occurred.
- **G2 — Report Contract (from AC7).** `docs/clippy-report.md` exists at that exact path and carries
  all five required sections.

Scoring levels used by every dimension below:

| Level | Meaning |
|:--|:--|
| 1 | Does not meet |
| 2 | Partially meets |
| 3 | Meets |
| 4 | Exceeds |

---

## D1 — Command Fidelity (from AC1)

Did the run use the repo's documented lint command, exactly, from the right directory, with no
invented flags or substitutions?

| Level | Definition | Example case |
|:--|:--|:--|
| 1 | Ran something else entirely, or never ran a command. | The report describes a `cargo check --workspace` run and presents it as the lint run. |
| 2 | Used the right command but changed it, or ran it from the wrong directory. | Ran `cargo clippy --workspace --all-targets` (flags not in the documentation) from `/workspace`, or the documented command from `crates/server`. |
| 3 | Ran `cargo clippy --release -- -D warnings` from the workspace root and said so accurately. | Report opens with "Ran `cargo clippy --release -- -D warnings` from the workspace root." |
| 4 | As level 3, and quotes the documented line it took the command from, so a reviewer can confirm the mapping. | Report cites the `AGENTS.md` Tests block next to the command it ran. |

## D2 — Verdict Accuracy (from AC2)

Does the stated lint-clean / not-clean verdict match the command's real exit status?

| Level | Definition | Example case |
|:--|:--|:--|
| 1 | Verdict contradicts the exit status. | Clippy exits 101 with `-D warnings` diagnostics and the report calls the workspace lint-clean. |
| 2 | No explicit verdict, or a hedged one. | "There are a handful of style suggestions worth considering at some point." |
| 3 | Verdict matches the exit status. | Exit 101, report says the lint gate fails; or exit 0, report says lint-clean. |
| 4 | Verdict matches, and the report says which output it was derived from. | "Exit code 101 — the diagnostics below are the evidence, and the gate is the `-D warnings` flag." |

## D3 — Rule-Naming Completeness (from AC3)

Is every lint rule the real output reports named in the report — no omissions, no invented rules, no
paraphrase that makes a rule unlocatable?

| Level | Definition | Example case |
|:--|:--|:--|
| 1 | Rules unnamed, or paraphrased so loosely they cannot be found. | "Some style lints fired in the server crate." |
| 2 | Some rules named; at least one rule present in the real output is missing. | Names `needless_return` and omits the `too_many_arguments` occurrences. |
| 3 | Every rule present in the output named, using the name the tool printed. | `clippy::needless_return`, `clippy::redundant_clone`, `clippy::too_many_arguments`. |
| 4 | As level 3, and the most frequent rule is traced to a concrete first occurrence. | Adds "`clippy::too_many_arguments` — first at `crates/server/src/api/posts.rs:214`." |

## D4 — Count Fidelity (from AC4)

Do the per-rule counts and the most-affected file match the runner's own output?

| Level | Definition | Example case |
|:--|:--|:--|
| 1 | Counts invented, or contradicting the output. | Claims 12 warnings where the real output shows 3 diagnostics. |
| 2 | Per-rule counts missing, or a figure that does not reconcile with the diagnostics. | Gives one workspace total while the tool prints each diagnostic separately. |
| 3 | Counts match the diagnostics, and the most-affected file is named. | "needless_return x4, redundant_clone x2 — most in `crates/server/src/api/matches.rs` (3)." |
| 4 | As level 3, and distinguishes what actually ran from what was filtered out — or, on a clean run, states the scope that came back clean. | "0 warnings, 0 errors across komun-core, komun-server and komun-wasm on the release profile, nothing filtered out." |

## D5 — Recommendation Consistency (from AC6)

Does the closing recommendation follow from the verdict, and is it specific enough to act on?

| Level | Definition | Example case |
|:--|:--|:--|
| 1 | No recommendation, or one that contradicts the verdict. | Diagnostics reported, recommendation is "the code is ready to ship". |
| 2 | Recommendation present but generic. | "Might be worth tidying those up at some point." |
| 3 | One clear recommendation consistent with the verdict. | "Not ready: the lint gate fails, so this should not be merged as it stands." |
| 4 | Consistent, and it names the file or rule to address first with a one-line reason — or, on a clean run, states the scope it verified and that nothing needs addressing. | "Start with `crates/server/src/api/matches.rs` — three of the nine diagnostics are there, all the same rule." |

---

## Pass threshold

A run **passes** when all of the following hold:

1. Both binary gates pass (G1 containment, G2 report contract).
2. The rubric total is **17 / 20 or higher**.
3. No single dimension scores 1.

A run that fails the threshold is recorded as a failure and is still valid evidence.
Scores are assigned against the PRD's intended behavior, not adjusted to fit a run.
