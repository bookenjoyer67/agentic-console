# Clippy Lint Report

Generated 2026-09-24 from the workspace root (`/workspace`), branch `feature/lab-task2-clippy-gate`.

## Command

```bash
cargo clippy --release -- -D warnings
```

This is the lint command the repository documents, taken verbatim from the Tests block in
`AGENTS.md`:

> ```bash
> cargo test --workspace     # komun-core + komun-server unit tests
> cargo clippy --release -- -D warnings   # must stay at zero warnings
> cd web && npm run check && npm run build && npx vitest run
> ```

No extra flags were added and no substitutions were made. Toolchain: `clippy 0.1.95
(59807616e1 2026-04-14)` / `rustc 1.95.0`.

## Verdict

**Lint-clean.** The command exited **0**.

The gate in this command is `-D warnings`, which promotes any clippy diagnostic to a hard error and
would have produced a non-zero exit. It exited 0, so clippy emitted no diagnostics at all — the
verdict is derived from that exit status together with the command's empty diagnostic output
(stdout was empty; stderr carried only cargo's `Checking`/`Finished` progress lines).

## Findings by lint rule

**None.** Zero warnings and zero errors, so no lint rule fired and there is nothing to group.

| Lint rule | Occurrences |
|:--|--:|
| _(none)_ | 0 |

### Scope that came back clean

| Crate | Path | Result |
|:--|:--|:--|
| `komun-core` | `crates/core` | 0 diagnostics |
| `komun-server` | `crates/server` | 0 diagnostics |
| `komun-wasm` | `crates/wasm` | 0 diagnostics |

All three workspace members were compiled fresh on the **release** profile — cached artifacts for
these crates were cleared first so the diagnostics were genuinely re-emitted rather than replayed
from a warm `target/`. Nothing was filtered out of the result.

One scope limit worth knowing, which is a property of the documented command rather than of this
run: `cargo clippy --release` lints the default targets (each crate's library and binary) and does
**not** compile test targets. Lints living in `#[cfg(test)]` code are therefore outside this gate.

## Most-affected file

**None** — no file carries any diagnostic, so there is no most-affected file to name.

## Recommendation

**Proceed.** The documented lint gate passes at zero warnings across `komun-core`, `komun-server`
and `komun-wasm` on the release profile, so nothing needs addressing before merge and no file
requires attention.

## Note on the one non-clippy warning in the output

The command's stderr ends with:

```
warning: the following packages contain code that will be rejected by a future version of Rust: sqlx-postgres v0.8.0
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
```

This is cargo's future-incompatibility notice about the third-party `sqlx-postgres v0.8.0`
dependency, not a clippy lint against this workspace's code. It is not subject to `-D warnings`,
did not affect the exit status, and is recorded here only so the raw output reconciles with the
zero-diagnostic count above.
