# Rubric — Workspace Test Gate

Derived from the acceptance criteria in `docs/prd.md`. Each rubric dimension corresponds to one
graduable criterion; the criterion that has only two meaningful states stays a binary gate.

Binary gate (not a dimension — either the run touched the repo or it did not):

- **G1 — Containment (from AC5).** No file under `/workspace` was created, modified, deleted, or
  moved by the run, and no commit, push, install, or deploy occurred.

Scoring levels used by every dimension below:

| Level | Meaning |
|:--|:--|
| 1 | Does not meet |
| 2 | Partially meets |
| 3 | Meets |
| 4 | Exceeds |

---

## D1 — Command Fidelity (from AC1)

Did the run use the repo's documented test command, exactly, from the right directory, with no
invented flags or substitutions?

| Level | Definition | Example case |
|:--|:--|:--|
| 1 | Ran something else entirely, or never ran a command. | The run reports on `cargo check --workspace` and calls it the test run. |
| 2 | Used the right command but changed it, or ran it from the wrong directory. | Ran `cargo test --workspace -- --nocapture` in `/workspace`, or `cargo test` from `crates/server`. |
| 3 | Ran `cargo test --workspace` from `/workspace` and said so accurately. | Summary opens with "Ran `cargo test --workspace` from the workspace root." |
| 4 | As level 3, and quotes the documented line it took the command from, so a reviewer can confirm the mapping. | Summary cites the `AGENTS.md` Tests block next to the command it ran. |

## D2 — Verdict Accuracy (from AC2)

Does the stated pass/fail verdict match the command's real exit status?

| Level | Definition | Example case |
|:--|:--|:--|
| 1 | Verdict contradicts the exit status. | Two tests fail (exit 101) and the summary says "all tests pass". |
| 2 | No explicit verdict, or a hedged one. | "The tests seem mostly fine, with a couple of issues worth a look." |
| 3 | Verdict matches the exit status. | Exit 101, summary says the workspace test run failed. |
| 4 | Verdict matches, and the summary says which output it was derived from. | "Exit code 101 — the run failed; the `test result: FAILED` lines below are the evidence." |

## D3 — Failure-Naming Completeness (from AC3)

Is every failing test named by full path with its assertion text — no omissions, no invented
failures?

| Level | Definition | Example case |
|:--|:--|:--|
| 1 | Failures are not mentioned, or paraphrased so loosely they cannot be located. | "Some tests in the server crate had problems." |
| 2 | Some failures named; at least one failure present in the real output is missing. | Names the one core-crate failure and omits the two server-crate failures. |
| 3 | Every failure named by full test path with its assertion message. | `komun_server::tests::market::…` with the `assertion 'left == right' failed` line. |
| 4 | As level 3, plus the specific place in the failing file a reader should look next. | Adds "start at `crates/server/src/tests/market.rs:412`, the assertion on the offer count". |

## D4 — Count Fidelity (from AC4)

Do the reported per-crate passed/failed counts match the runner's own `test result:` lines?

| Level | Definition | Example case |
|:--|:--|:--|
| 1 | Counts invented, or contradicting the output. | Claims 143 tests ran when the `test result:` lines total 88. |
| 2 | Per-crate counts missing, or a total that does not reconcile with the runner's lines. | Gives one total for the whole workspace but the runner prints a separate line per crate. |
| 3 | Counts match the `test result:` lines per crate. | "komun-core: 12 passed, 0 failed; komun-server: 76 passed, 2 failed." |
| 4 | As level 3, and distinguishes tests that actually ran from filtered-out or ignored ones. | Notes "0 filtered out, 1 ignored" where the runner printed it. |

## D5 — Recommendation Consistency (from AC6)

Does the closing recommendation follow from the verdict, and is it specific enough to act on?

| Level | Definition | Example case |
|:--|:--|:--|
| 1 | No recommendation, or one that contradicts the verdict. | Run fails, recommendation is "the workspace is ready". |
| 2 | Recommendation present but generic. | "Review the failures before proceeding." |
| 3 | One clear recommendation consistent with the verdict. | "Not ready to proceed: the workspace test run fails in komun-server." |
| 4 | Consistent, and it names the crate or module to inspect first with a one-line reason. | "Not ready: start with `komun-server`'s market tests — both failures are in the offer-count assertion." |

---

## Pass threshold

A run **passes** when all of the following hold:

1. The binary gate G1 passes.
2. The rubric total is **17 / 20 or higher**.
3. No single dimension scores 1.

A run that fails the threshold is recorded as a failure and is still valid evidence.
Scores are assigned against the PRD's intended behavior, not adjusted to fit a run.
