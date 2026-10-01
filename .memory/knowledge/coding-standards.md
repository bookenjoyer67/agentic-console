# coding-standards.md

The rules a change to this repository must respect. Human-maintained: the agent reads this file and
never writes to it. `.claude/hooks/guard-readonly-memory.sh` enforces that at the tool layer, because
the container's agent runs as root and root ignores file permissions.

Each rule names the artifact that states it, so a reader can check the rule instead of trusting this
summary. Where this file and its source disagree, the source wins and this file is the defect.

## 1. Never commit a credential

Real key material stays outside the repository. The broker stages it under the home directory
(`sandbox/stage-secrets.sh:14` `STATE="${HOME}/.config/komun-sandbox"`), the agent container holds a
dummy token only (`sandbox/run-agent.sh:91` `-e ANTHROPIC_AUTH_TOKEN=sandbox-dummy-token`), and an
environment variable is named in prose rather than pasted. `.env` is gitignored.

## 2. The seam table is the single source

A value that a consumer needs lives once in `agentic.config.json`, and every consumer reads it and
carries a fallback of its own. Edit the value in the config and the matching fallback in the consumer
together — a fallback left at the old default is the half-wired case `scripts/port-self-test.sh`
exists to catch. Add a row to that script's consumer list when a new file starts reading the config.

## 3. Touch a source before trusting a lint

`cargo clippy` over an unchanged tree prints nothing and exits 0, which is indistinguishable from a
clean lint. The clippy gate therefore touches a file under test before invoking cargo and then requires
its configured marker line in the output; `passed` for clippy is the exit code **and** a satisfied
guard (`agentic.config.json:41-46`).

## 4. Zero warnings is the bar, not a goal

`cargo clippy --release --all-targets -- -D warnings` must produce zero warnings, and the fmt,
clippy and test gates must all be run locally before a change is reported as done.

## 5. Never weaken a check or a test to reach a pass

A test edited to fit a change stops being a regression test. A check relaxed to make a tree green
removes the only evidence the tree was ever red. When a check is wrong, fix the check and say so; when
it is inconvenient, that is the signal it is working.

## 6. Every citation carries its literal

A reader must be able to verify a claim without trusting the writer. Quote the literal text the cited
line carries when writing `path:line`; a bare location is a v1 form. A claim that cannot be traced is
marked as untraceable rather than given invented authority, and a count carries the search that
produced it.

## 7. Fixed predicates require zero failures, not a passed count

A predicate over a test run requires `failed == 0` alongside any pass count or baseline. A passed count
alone prints PASS on a tree whose runner exited non-zero, which is the opposite of what the predicate is
for.

## 8. Read-only by construction

The console writes nothing inside a repository it watches; it offers no `rm`, no container restart, no
config write and no git command. Any new action it grows must put its exact argv, working directory,
environment and anything it would write on screen, and wait for a confirmation key. Nothing in this
repository reaches the network except the broker.
