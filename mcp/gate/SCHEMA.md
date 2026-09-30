# Quality-Gate MCP Server Schema

Which server, endpoint, and runtime files does this document describe?

The server is named `gate` (`mcp/gate/server.py:86` `mcp = FastMCP("gate")`), and it serves streamable
HTTP at `http://localhost:8003/mcp` (a live `initialize` returned `HTTP/1.1 200 OK` with
`"serverInfo":{"name":"gate","version":"3.4.7"}`).

It exposes four named operations and no shell, no argv, and no working-directory control:
`list_gates`, `run_gate`, `run_fix`, `read_audit_log`
(`mcp/gate/server.py:466` `def list_gates() -> list[dict]:`,
`mcp/gate/server.py:494` `def run_gate(gate: str, calling_role: str = "unknown", timeout_seconds: int | None = None) -> dict:`,
`mcp/gate/server.py:521` `def run_fix(command: str, calling_role: str = "unknown", timeout_seconds: int | None = None) -> dict:`,
`mcp/gate/server.py:551` `def read_audit_log(limit: int = 20) -> list[dict]:`, one `@mcp.tool` before each).

`run_gate` runs the check-mode gates and `run_fix` runs the write-mode commands. Neither accepts a
command string, an extra argument or a shell.

Runtime file inside the sandbox container:

| File | Default path | Authority |
| --- | --- | --- |
| Audit journal | `/workspace/.memory/gate-audit.log` | `mcp/gate/gate_vocabulary.py:101` `AUDIT_PATH = os.getenv("GATE_AUDIT_PATH", str(Path(MEMORY_DIR) / "gate-audit.log"))` |

The path follows `MEMORY_DIR`, which defaults to `/workspace/.memory`, the config's `containers.memory_dir`
(`mcp/gate/gate_vocabulary.py:100` `MEMORY_DIR = os.getenv("MEMORY_DIR", str(agentic_config.get("containers.memory_dir")))`).
Override `GATE_AUDIT_PATH` and `GATE_WORKSPACE` for a local run
(`mcp/gate/gate_vocabulary.py:99` `WORKSPACE = os.getenv("GATE_WORKSPACE", str(agentic_config.get("containers.workspace")))`).

Which command starts the server, and how is it registered?

```bash
python3 mcp/gate/server.py --port 8003 --host 0.0.0.0
```

```bash
claude mcp add --transport http gate http://localhost:8003/mcp
```

`--port` defaults to `8003` (`mcp/gate/server.py:578` `parser.add_argument("--port", type=int, default=8003, help="HTTP port (default 8003)")`),
and `--host` defaults to `0.0.0.0` (`mcp/gate/server.py:579` `parser.add_argument("--host", default="0.0.0.0", help="bind address (default 0.0.0.0)")`). The help output
confirms both (`python3 mcp/gate/server.py --help` -> `--port PORT           HTTP port (default 8003)`).

## Which eight commands exist, and which command does each one run?

Where do the eight command names come from?

Eight commands exist: seven are check-mode and one is write-mode. The names are the
`toolchain.commands` keys in `agentic.config.json`, and `gate_vocabulary.py` builds one table from
them (`mcp/gate/gate_vocabulary.py:244` `COMMANDS: dict[str, dict[str, Any]] = {name: _gate(name) for name in COMMAND_NAMES}`).

That table is then split by each command's declared `writes` boolean into two disjoint vocabularies
(`mcp/gate/gate_vocabulary.py:245` `GATES: dict[str, dict[str, Any]] = {` and
`mcp/gate/gate_vocabulary.py:248` `FIX_COMMANDS: dict[str, dict[str, Any]] = {`). `run_gate` resolves a
name against `GATES` only, and `run_fix` resolves a name against `FIX_COMMANDS` only.

| Command | Mode | Exact argv | Authority |
| --- | --- | --- | --- |
| `test` | check | `cargo test --workspace` | `agentic.config.json:20-24` `"argv": [ "cargo", "test", "--workspace" ],` |
| `clippy` | check | `cargo clippy --release --all-targets -- -D warnings` | `agentic.config.json:31-39` `"argv": [ "cargo", "clippy", "--release", "--all-targets", "--", "-D", "warnings" ],` |
| `fmt` | check | `cargo fmt --check` | `agentic.config.json:51-55` `"argv": [ "cargo", "fmt", "--check" ],` |
| `policy` | check | `python3 -m pytest eval/test_policy.py eval/test_deterministic_step.py -q` | `agentic.config.json:72` `"argv": ["python3", "-m", "pytest", "eval/test_policy.py", "eval/test_deterministic_step.py", "-q"],` |
| `conformance` | check | `python3 scripts/run-conformance-gate.py` | `agentic.config.json:79` `"argv": ["python3", "scripts/run-conformance-gate.py"],` |
| `webcheck` | check | `npm --prefix web run check` | `agentic.config.json:93` `"argv": ["npm", "--prefix", "web", "run", "check"],` |
| `webtest` | check | `npm --prefix web run test` | `agentic.config.json:100` `"argv": ["npm", "--prefix", "web", "run", "test"],` |
| `fmt-fix` | write | `cargo fmt --all` | `agentic.config.json:86` `"argv": ["cargo", "fmt", "--all"],` |

Both frontend gates name an npm script rather than a command line
(`web/package.json:9` `"test": "vitest run"`). The `check` script sits on that same line
(`web/package.json:9` `"check": "svelte-kit sync && svelte-check --tsconfig ./tsconfig.json",`).

Which mode does each command declare, and who declares it?

The config declares it, one boolean per command. The seven check-mode commands carry
`"writes": false` and the one write-mode command carries `"writes": true`
(`agentic.config.json:90` `"writes": true`). Nothing in the server infers a mode from a command's
argv, so a fork that adds a mutating command declares it rather than being guessed at.

The `policy` gate runs the eval suites. The `conformance` gate runs `scripts/run-conformance-gate.py`,
whose verdict is new drift only (`scripts/run-conformance-gate.py:2` `fail on NEW drift only.`), and
whose prose file set is the config key `gates.conformance.files` (`agentic.config.json:264` `"files": [`).
It checks each file against the same file at `HEAD` and fails only when a rule's finding count rises,
so the repository's pre-existing findings never make it red.

The `fmt-fix` command is the one command that rewrites the tree. It is `cargo fmt --all`, the write
counterpart of the `fmt` gate's `cargo fmt --check`, and no role that runs a check holds it.

The three cargo gates are the project's documented gates (`AGENTS.md:196` `cargo test --workspace`,
`AGENTS.md:197` `cargo clippy --release -- -D warnings`, `.memory/knowledge/coding-standards.md:10`
`` `cargo clippy --release -- -D warnings` must exit clean on every change. ``, and rule 1's cache-hit
note `.memory/knowledge/coding-standards.md:11` `a silent second run is a cache hit, not a clean lint`).

The clippy argv is deliberately stricter than the documented form: it adds `--all-targets`, so test
and example targets are linted too.

`list_gates` publishes every command with its argv and its mode
(`mcp/gate/server.py:476` `"argv": list(definition["argv"]),`,
`mcp/gate/server.py:478` `"writes": definition["writes"],`,
`mcp/gate/server.py:479` `"mode": "write" if definition["writes"] else "check",`). A caller can
therefore see which tool runs a command before naming it.

A live call on 2026-09-28, against the five-command surface of that day, returned the check-mode
tuples verbatim, so the allowlist a caller sees is the allowlist that runs:

```json
{"clippy": ["cargo", "clippy", "--release", "--all-targets", "--", "-D", "warnings"], "conformance": ["python3", "scripts/run-conformance-gate.py"], "fmt": ["cargo", "fmt", "--check"], "policy": ["python3", "-m", "pytest", "eval/test_policy.py", "eval/test_deterministic_step.py", "-q"], "test": ["cargo", "test", "--workspace"]}
```

The `fmt-fix` entry is published by that same code path and by no other, but no live capture of the
eight-entry list exists in this document yet.

## Why can a caller not run an arbitrary command?

What does the tool accept in place of a command string?

Because the only thing `run_gate` accepts is a key into `GATES`, and the argv it executes is that
table's tuple (`mcp/gate/server.py:120` `if gate not in GATES:` and
`mcp/gate/server.py:394` `argv = list(definition["argv"])`).

The refusal is raised before anything is executed
(`mcp/gate/server.py:122` `f"refused: '{gate}' is not an allowlisted gate. This server runs only "`). A caller asking for a
free-form command gets this text, captured from a live call:

```
Error calling tool 'run_gate': refused: 'cargo test --workspace' is not an allowlisted gate. This server runs only ['clippy', 'conformance', 'fmt', 'policy', 'test'] by name; it accepts no command string, no extra arguments and no shell.
```

An argument-passthrough attempt and a shell-injection attempt were refused with the same message:
`gate='test -- --nocapture'` and `gate='test; touch /tmp/gate-selftest-pwned'`, and the injection
target did not exist afterwards (`/tmp/gate-selftest-pwned exists=False`).

Does the second tool widen that surface?

No. `run_fix` is the same construction against the other table, so the surface grows by exactly one
configured command and by no new kind of input
(`mcp/gate/server.py:136` `def validate_fix(command: str) -> str:` with
`mcp/gate/server.py:148` `f"refused: '{command}' is not an allowlisted write-mode command. This server runs "`).

Which properties make that airtight?

- `shell` is never enabled and no caller string reaches the command line
  (`mcp/gate/server.py:400` `completed = subprocess.run(  # noqa: S603 - a fixed argv, never a caller-supplied string`
  with `shell` absent, and `mcp/gate/server.py:401` `argv,` as the whole argument list).
- The same tuple is what the result reports and what the journal records, so a reviewer can compare
  the executed argv against the allowlist rather than trusting the tool
  (`mcp/gate/server.py:436` `"argv": argv,`).
- The working directory is the server's own workspace, not a caller value
  (`mcp/gate/server.py:402` `cwd=WORKSPACE,`).
- The commands cannot be widened by a second element: there is no per-call argument list in either
  tool signature at all (`mcp/gate/server.py:494` `def run_gate(gate: str, calling_role: str = "unknown", timeout_seconds: int | None = None) -> dict:`,
  `mcp/gate/server.py:521` `def run_fix(command: str, calling_role: str = "unknown", timeout_seconds: int | None = None) -> dict:`).
- The executing function reads the argv from the table its caller passed, so neither tool can reach
  the other's command even by name
  (`mcp/gate/server.py:502` `result = execute_gate(gate, GATES, effective_timeout)` and
  `mcp/gate/server.py:532` `result = execute_gate(command, FIX_COMMANDS, effective_timeout)`).

## What does `run_gate` accept, and what does it return?

Which artifact settles the parameter list?

The tool signature is the parameter authority (`mcp/gate/server.py:494` `def run_gate(`).

- Pass `gate` (`str`, required) as one of `test`, `clippy`, `fmt`, `policy`, `conformance`, `webcheck`, `webtest` (`mcp/gate/server.py:494` `gate: str,`).
- Pass `calling_role` (`str`, optional, default `unknown`) so the journal names the caller (`mcp/gate/server.py:494` `calling_role: str = "unknown"`).
- Pass `timeout_seconds` (`int`, optional, default `null` -> 900) as the per-run cap, clamped to 60..3600 (`mcp/gate/server.py:494` `timeout_seconds: int | None = None`).

The returned object carries one key per required fact (`mcp/gate/server.py:434` `return {`):

| Field | Type | Meaning |
| --- | --- | --- |
| `gate` | `str` | The allowlisted name that ran (`mcp/gate/server.py:435` `"gate": command,`) |
| `argv` | `list[str]` | The exact argv executed (`mcp/gate/server.py:436` `"argv": argv,`) |
| `exit_code` | `int` | Process exit status, `-1` when the run was killed (`mcp/gate/server.py:437` `"exit_code": exit_code,`) |
| `passed` | `bool` | Verdict: exit 0 **and** guard satisfied (`mcp/gate/server.py:422` `passed = exit_code == 0 and bool(guard["satisfied"])`) |
| `verdict` | `str` | `pass`, `fail: exit code N`, or the guard/timeout cause (`mcp/gate/server.py:424` `verdict = f"fail: killed after {timeout_seconds}s without finishing"`) |
| `timed_out` | `bool` | Whether the timeout killed the process (`mcp/gate/server.py:412` `timed_out = True`) |
| `timeout_seconds` | `int` | The effective (clamped) timeout (`mcp/gate/server.py:441` `"timeout_seconds": timeout_seconds,`) |
| `duration_seconds` | `float` | Wall-clock seconds, rounded to milliseconds (`mcp/gate/server.py:416` `duration_seconds = round(time.monotonic() - started, 3)`) |
| `writes` | `bool` | The command's declared mode as a boolean (`mcp/gate/server.py:443` `"writes": bool(definition["writes"]),`) |
| `mode` | `str` | `check` or `write`, the same fact in words (`mcp/gate/server.py:444` `"mode": "write" if definition["writes"] else "check",`) |
| `guard` | `dict` | The cache-hit guard outcome, see below (`mcp/gate/server.py:445` `"guard": guard,`) |
| `summary` | `dict` | The configured output summary, see below (`mcp/gate/server.py:446` `"summary": summary,`) |
| `stdout` / `stderr` | `str` | Captured output, ANSI escapes stripped (`mcp/gate/server.py:432` `stdout, stdout_truncated = clamp_output(strip_ansi(raw_stdout))`) |
| `stdout_truncated` / `stderr_truncated` | `bool` | Whether the 200 000-character cap cut the stream (`mcp/gate/server.py:84` `MAX_OUTPUT_CHARS = 200_000`) |
| `output_ansi_stripped` | `bool` | Always `true`, so a caller knows colour codes were removed (`mcp/gate/server.py:451` `"output_ansi_stripped": True,`) |

A `run_gate` result always reports `"mode": "check"`, because the name was resolved against the
check-mode table before anything ran.

## What does `run_fix` accept, and which calls does it refuse?

Which artifact settles this tool's parameter list?

The tool signature settles it, and it is the mirror of `run_gate`'s
(`mcp/gate/server.py:521` `def run_fix(`). Three parameters, none of which carries an argv element, a
path, a flag, a working directory or a shell:

- Pass `command` (`str`, required) as an allowlisted write-mode name; the vocabulary holds one, `fmt-fix` (`mcp/gate/server.py:521` `command: str,`).
- Pass `calling_role` (`str`, optional, default `unknown`) so the journal names the caller (`mcp/gate/server.py:521` `calling_role: str = "unknown"`).
- Pass `timeout_seconds` (`int`, optional, default `null` -> 900) as the per-run cap, clamped the same way (`mcp/gate/server.py:521` `timeout_seconds: int | None = None`).

The returned object is the table above, with `"writes": true` and `"mode": "write"`.

Which two refusal directions keep the modes apart?

The two tables are disjoint, so each tool refuses the other's names, and each says so by mode as well
as by name:

- Refuse a write-mode command named to `run_gate`, because `GATES` has no such key
  (`mcp/gate/server.py:126` `f" '{gate}' is a write-mode command, refused here by mode as well as by name: "`).
- Refuse a check-mode gate named to `run_fix`, because `FIX_COMMANDS` has no such key
  (`mcp/gate/server.py:152` `f" '{command}' is a check-mode gate, refused here by mode as well as by name: "`).

Every other string a caller might send `run_fix` — a command line, an argument passthrough, a
shell-injection attempt — is refused for the reason all of them are refused: it is not a key of the
table, and the table is the only source of an argv
(`mcp/gate/server.py:146` `if command not in FIX_COMMANDS:`).

Has `fmt-fix` been executed through this server?

Not in any run recorded in this document. It rewrites the tree, so the self-test deliberately never
executes it, and the evidence here is refusal evidence only. Treat every claim about what `fmt-fix`
would produce as unmeasured until a run appears in the journal.

## How does the clippy cache-hit guard work?

Which two steps make up the guard?

The gate touches a file under test, then requires cargo's status line for that crate in the output.

1. Touch `crates/server/src/main.rs` before running cargo
   (`agentic.config.json:44` `"touch_file": "crates/server/src/main.rs",`,
   `mcp/gate/server.py:263` `os.utime(touch_file, None)`). A missing file is reported as an
   unsatisfied guard rather than ignored (`mcp/gate/server.py:270` `"detail": f"cache-hit guard could not touch {touch_file}: {error}",`).
2. Require the `Checking komun-server` marker in the combined output
   (`agentic.config.json:42` `"marker": "Checking komun-server",`,
   `agentic.config.json:43` `"marker_regex": "\\bChecking\\b\\s+(?P<marker>komun-server)\\b",`).
3. Fail the gate when the marker is absent, even at exit 0 (`mcp/gate/server.py:427` `elif not guard["satisfied"]:` with `mcp/gate/server.py:288` `f"cache-hit guard not satisfied: no '{guard['marker']}' line in the output, so a clean "`).

Why does the guard check the stripped output rather than the raw output?

Because this image runs cargo with colour forced on (`CARGO_TERM_COLOR=always` in the container
environment), so a status line is written as `\033[1m\033[92m    Checking\033[0m komun-server`. Byte
evidence from the captured stderr: `Checking` appears 3 times while the literal
`Checking komun-server` appears 0 times, because the escape sequence sits between the two words. The
guard strips SGR sequences first (`mcp/gate/server.py:286` `match = GUARD_MARKER_PATTERN.search(strip_ansi(combined_output))`,
`mcp/gate/server.py:79` `ANSI_ESCAPE = re.compile(r"\x1b\[[0-9;?]*[ -/]*[@-~]")`), and the returned
stdout and stderr are stripped for the same reason.

A live clippy call through the server reported the guard satisfied and pasted its own evidence:

```json
{"gate": "clippy", "argv": ["cargo", "clippy", "--release", "--all-targets", "--", "-D", "warnings"], "exit_code": 0, "passed": true, "verdict": "pass", "duration_seconds": 2.533, "timed_out": false, "guard": {"applied": true, "satisfied": true, "marker": "Checking komun-server", "touched": "/workspace/crates/server/src/main.rs", "detail": "found 'Checking komun-server' in the cargo output"}}
```

and the returned stderr begins with that line:

```
    Checking komun-server v0.1.0 (/workspace/crates/server)
    Finished `release` profile [optimized] target(s) in 2.67s
```

What does the guard protect against, measured rather than assumed? Two clippy runs with no touch in
between, both over the warm target directory, exited `0` with no `Checking komun-server` line at all
(`run1: exit=0 checking_komun_server=0`, `run2: exit=0 checking_komun_server=0`). A gate that read
exit codes alone would call that a clean lint; the guard makes it unrepresentable.

## What does the configured output summary report?

Which artifact declares a summary rule, and which command carries one?

The config declares it, one `summary` block per command, beside the `guard` block it mirrors. Seven
commands declare `"summary": null` and one declares a rule: `fmt`
(`agentic.config.json:58` `"summary": {`).

The vocabulary normalises that block once, at import time
(`mcp/gate/gate_vocabulary.py:126` `def _summary(command: dict[str, Any], embedded: dict[str, Any]) -> dict[str, Any] | None:`),
and compiles each declared pattern beside the guard's
(`mcp/gate/gate_vocabulary.py:269` `SUMMARY_PATTERNS: dict[str, dict[str, re.Pattern[str]]] = {`).

Compiled patterns live in `SUMMARY_PATTERNS` and not in `COMMANDS`, so `COMMANDS` stays JSON-safe for
the tool that publishes it (`mcp/gate/server.py:476` `"argv": list(definition["argv"]),`).

A malformed rule degrades to no summary rather than to a crash. `_summary` returns `None` for an
unrecognised mode, an uncompilable pattern, or a named group absent from that pattern
(`mcp/gate/gate_vocabulary.py:126` `def _summary(command: dict[str, Any], embedded: dict[str, Any]) -> dict[str, Any] | None:`).

Which four numbers does the `fmt` rule count?

| Count | Mode | Declared at |
| --- | --- | --- |
| `hunks` | `count_matching_lines` | `agentic.config.json:63` `"hunks": {"mode": "count_matching_lines", "pattern": "^Diff in "},` |
| `files` | `count_unique_groups` | `agentic.config.json:64` `"files": {"mode": "count_unique_groups", "pattern": "^Diff in (?P<value>.+?)(?:(?::|\\s+at\\s+line\\s+)\\d+)?:?\\s*$", "group": "value"},` |
| `added` | `count_matching_lines` | `agentic.config.json:65` `"added": {"mode": "count_matching_lines", "pattern": "^\\+"},` |
| `removed` | `count_matching_lines` | `agentic.config.json:66` `"removed": {"mode": "count_matching_lines", "pattern": "^-"}` |

`count_matching_lines` counts the lines a pattern matches. `count_unique_groups` counts the distinct
values of one named group across those matches, which is what turns hunk lines into a file count.

Which text does the count run over, and at which point in the run?

Count over the captured `stdout` and `stderr` together
(`agentic.config.json:60` `"streams": ["stdout", "stderr"],`), with ANSI escapes stripped first
(`agentic.config.json:61` `"strip_ansi": true,`).

Strip before matching, because this image forces cargo colour on, so `^\+` against the raw bytes
matches nothing (`mcp/gate/server.py:79` `ANSI_ESCAPE = re.compile(r"\x1b\[[0-9;?]*[ -/]*[@-~]")`).

Count above the output clamp, so the four numbers describe the whole run
(`mcp/gate/server.py:421` `summary = compute_output_summary(command, raw_stdout, raw_stderr)`). That
line takes the raw capture, and the clamp runs later
(`mcp/gate/server.py:432` `stdout, stdout_truncated = clamp_output(strip_ansi(raw_stdout))`).

The summary reports the character count it consumed and whether the clamp would have cut it, so a
reader can tell a whole-run count from a partial one
(`mcp/gate/server.py:311` `def compute_output_summary(command: str, stdout: str, stderr: str) -> dict[str, Any]:`).

Does the summary change any verdict?

No. `passed` stays exit 0 **and** guard satisfied
(`mcp/gate/server.py:422` `passed = exit_code == 0 and bool(guard["satisfied"])`), and the summary is
computed on a line above it without entering it.

`verdict` is unchanged for the same reason. The summary is a reporting field in the response and one
key in the journal row, and no verdict reads it back.

What does a command with no rule record?

Record an unapplied marker with no counts, so the absence is explicit rather than a missing key
(`mcp/gate/server.py:311` `def compute_output_summary(command: str, stdout: str, stderr: str) -> dict[str, Any]:`).
The seven commands that declare `"summary": null` take that path, and the self-test asserts it on
`clippy` and on `test` (`mcp/gate/selftest.py:505` `"no_summary_gate_records_none",`).

What does this run not settle?

Whether any other gate needs a summary rule is not settled by this run. Only `fmt` declares one
(`agentic.config.json:58` `"summary": {`), and nothing measured here bears on `test`, `clippy`,
`policy`, `conformance`, `webcheck`, `webtest` or `fmt-fix`.

No run through the summary code path is recorded in this document yet, so treat the four numbers the
gate now reports as unmeasured here until a journal row carries them.

## What does the per-run timeout do?

What is the default cap, and which bounds clamp it?

Default 900 s (`mcp/gate/server.py:81` `DEFAULT_TIMEOUT_SECONDS = int(os.getenv("GATE_TIMEOUT_SECONDS", "900"))`),
clamped into 60..3600 s (`mcp/gate/server.py:168` `return max(MIN_TIMEOUT_SECONDS, min(MAX_TIMEOUT_SECONDS, timeout_seconds))`).

The expiry kills the process and is reported rather than raised: `exit_code` becomes `-1`,
`timed_out` becomes `true`, and `passed` becomes `false`
(`mcp/gate/server.py:411` `except subprocess.TimeoutExpired as expired:`).

A live call asked for `timeout_seconds=1` and got the floor reported back
(`requested=1s reported=60s timed_out=False exit=1`), which is the clamp working. The kill path
itself is not exercised by the self-test: no gate in this repository runs longer than the 60 s floor,
so there is no honest way to reach it without changing the gates.

Both tools clamp through the same function, so `run_fix` cannot buy a longer or shorter window than
`run_gate` (`mcp/gate/server.py:531` `effective_timeout = validate_timeout(timeout_seconds)`).

## What shape does one audit-journal record have?

How is each record written to the journal?

Write one JSON object per line, with keys in sorted order
(`mcp/gate/server.py:175` `line = json.dumps(record, sort_keys=True) + "\n"`).

Flush and `fsync` each record so a following `tail -n 1` sees it immediately
(`mcp/gate/server.py:179` `os.fsync(handle.fileno())`).

| Key | Meaning |
| --- | --- |
| `timestamp` | ISO-8601 UTC instant (`mcp/gate/server.py:210` `"timestamp": utc_now(),`) |
| `tool` | The tool that ran it, `run_gate` or `run_fix` (`mcp/gate/server.py:211` `"tool": tool,`) |
| `gate` | The allowlisted command name (`mcp/gate/server.py:212` `"gate": gate,`) |
| `argv` | The exact argv executed (`mcp/gate/server.py:213` `"argv": argv,`) |
| `exit_code` | The process exit status (`mcp/gate/server.py:214` `"exit_code": exit_code,`) |
| `duration_seconds` | Wall-clock seconds (`mcp/gate/server.py:215` `"duration_seconds": duration_seconds,`) |
| `passed` | The verdict (`mcp/gate/server.py:216` `"passed": passed,`) |
| `timed_out` | Whether the timeout killed it (`mcp/gate/server.py:217` `"timed_out": timed_out,`) |
| `guard_applied` / `guard_satisfied` | The cache-hit guard outcome (`mcp/gate/server.py:218` `"guard_applied": guard_applied,`) |
| `summary` | The configured output summary, or an unapplied marker (`mcp/gate/server.py:220` `"summary": summary,`) |
| `writes` | Whether the command rewrote files: `false` for a check, `true` for a fix (`mcp/gate/server.py:221` `"writes": writes,`) |
| `calling_role` | The caller's role, defaulting to `unknown` (`mcp/gate/server.py:222` `"calling_role": calling_role or "unknown",`) |

Why does a journal row carry the summary as well as the response?

Because a journal reader reaches the counts without the payload that produced them, and the payload
is the part the clamp cuts. The server passes the same object to both
(`mcp/gate/server.py:446` `"summary": summary,` in the result, `mcp/gate/server.py:220` `"summary": summary,`
in the record), and the self-test compares them
(`mcp/gate/selftest.py:544` `"journal_records_the_summary",`).

Why do `tool` and `writes` both appear?

Because a reader of the journal should be able to tell a check from a mutation from the record alone,
without consulting the config. `tool` names the operation and `writes` names the mode, and the server
sets them together at the call site (`mcp/gate/server.py:513` `writes=False,` in `run_gate`,
`mcp/gate/server.py:543` `writes=True,` in `run_fix`).

A real line written by this server (`run_gate` on `clippy` as `tester`), captured before the `writes`
key existed:

```json
{"argv": ["cargo", "clippy", "--release", "--all-targets", "--", "-D", "warnings"], "calling_role": "tester", "duration_seconds": 2.533, "exit_code": 0, "gate": "clippy", "guard_applied": true, "guard_satisfied": true, "passed": true, "timed_out": false, "timestamp": "2026-09-28T17:54:14.719689+00:00", "tool": "run_gate"}
```

Why does a refused call add no line?

Because the refusal is raised in validation, before `execute_gate` and before `audit_invocation`
(`mcp/gate/server.py:500` `validate_gate(gate)` on a line above `mcp/gate/server.py:503` `audit_invocation(`),
so the journal stays a record of executed commands only — the same reason the storage server never
journals a refused write (`mcp/storage/server.py:121` `if classification not in WRITE_CLASSIFICATIONS:`).

`run_fix` is ordered the same way, so a refused fix is as absent from the journal as a refused gate
(`mcp/gate/server.py:530` `validate_fix(command)` on a line above `mcp/gate/server.py:533` `audit_invocation(`).

Measured: three refusals in one self-test run left the journal at 10 lines, unchanged
(`refusals_journal_nothing :: journal lines before=10 after=10`).

No tool edits or erases the journal; it is opened append-only
(`mcp/gate/server.py:176` `with open(AUDIT_PATH, "a", encoding="utf-8") as handle:`), and the only
read is bounded (`mcp/gate/server.py:553` `if not isinstance(limit, int) or isinstance(limit, bool) or limit < 1 or limit > 200:`
with `mcp/gate/server.py:554` `raise ValueError("limit must be an integer between 1 and 200")`).

## Which exit codes did five of the seven check-mode gates produce?

When and where were these exit codes captured?

Captured through the server in the agent sandbox container
(`cargo 1.95.0 (f2d3ce0bd 2026-03-21)`, `rustfmt 1.9.0-stable (59807616e1 2026-04-14)`). The three
cargo rows are the 2026-09-28 capture, and the two Python rows were captured on 2026-09-29 after the
vocabulary grew to five names:

| Gate | Exit code | `passed` | Guard | Duration | Evidence |
| --- | --- | --- | --- | --- | --- |
| `test` | `0` | `true` | not applicable | 5.7 s | `test result: ok. 20 passed; 0 failed` and `ok. 138 passed; 0 failed` — 158 passed, 0 failed, the documented baseline |
| `clippy` | `0` | `true` | applied, satisfied | 2.5 s | `Checking komun-server v0.1.0 (/workspace/crates/server)`, then `Finished` with no lint |
| `fmt` | `1` | `false` | not applicable | 0.1 s | 213 diff hunks across 35 files |
| `policy` | `0` | `true` | not applicable | 1.1 s | `90 passed in 0.90s`, the two eval suites |
| `conformance` | `0` | `true` | not applicable | 2.2 s | `verdict pass`, `"reason": "no rule's finding count rose against HEAD"`, 135 findings at base against 134 current |

Every row above predates the summary rule, so the `fmt` row's hunk and file counts are the capture of
that day rather than a number this gate reported. The four counts the gate now computes enter this
table after a recorded run produces them.

Three commands have no row. `fmt-fix` rewrites the tree, so it has never been run here
(`mcp/gate/server.py:521` `def run_fix(`). `webcheck` and `webtest` entered the vocabulary after this
capture (`agentic.config.json:93` `"argv": ["npm", "--prefix", "web", "run", "check"],` and
`agentic.config.json:100` `"argv": ["npm", "--prefix", "web", "run", "test"],`). A row for any of the
three would be a prediction rather than a capture.

The `fmt` gate genuinely fails, and the failure is not this server's doing and not new: rustfmt over
the committed `HEAD` revision of an untouched file also fails (`git show HEAD:crates/server/src/api/admin.rs`
piped into `rustfmt --check --edition 2021` -> exit `1`, `9` hunks). The drift spans 35 files, far
wider than the two test files the working-tree fix touches, so it is repo-wide formatting drift
against this rustfmt version rather than a change under test. Neither `cargo fmt --check` nor the
gate rewrites any file; check mode only reports.

## How is this server's quality validated?

Which script validates this server, and what does it assert?

`mcp/gate/selftest.py` calls the running server over streamable HTTP and asserts every behaviour this
document claims: the tool surface, the eight-command allowlist and the mode published for each, the
three refusal shapes aimed at the check-mode names, refusal in both directions across the mode
boundary, the clippy guard, the configured output summary and its absence on the seven commands
without a rule, the test baseline predicate, the timeout clamp, real exit codes from the three cargo
gates, the ANSI strip, and one journal line per executed invocation.

The write-mode command is never executed by the self-test. Its mode-boundary checks are refusal and
absence checks, so a run of this script formats nothing
(`mcp/gate/selftest.py:13` `through the refusals that prove it is unreachable from the check surface.`).

Run it against a listening server:

```bash
python3 mcp/gate/server.py --port 8003 &
python3 mcp/gate/selftest.py --url http://localhost:8003/mcp
```

Which line does it print, and which exit code does it use?

Print one `SELFTEST_RESULT` line and exit `0` only when every check passed
(`mcp/gate/selftest.py:571` `print(f"SELFTEST_RESULT passed={passed} total={total}", flush=True)`).

The last recorded run predates the write-mode checks: against the five-command surface it printed
`SELFTEST_RESULT passed=27 total=27` and exited `0`. The rows naming `run_fix` and the rows naming
the output summary have not yet appeared in a recorded run, so read them as what the script asserts
rather than as a measurement.

| Check | What it proves |
| --- | --- |
| `tool_surface`, `run_gate_parameters` | Exactly four tools, and `run_gate` takes `gate`, `calling_role`, `timeout_seconds` (`mcp/gate/selftest.py:63` `EXPECTED_TOOLS = {"list_gates", "run_gate", "run_fix", "read_audit_log"}`) |
| `run_fix_parameters` | `run_fix` takes `command`, `calling_role`, `timeout_seconds`, and no fourth property (`mcp/gate/selftest.py:68` `EXPECTED_RUN_FIX_PARAMS = {"command", "calling_role", "timeout_seconds"}`) |
| `allowlist_is_the_eight_documented_commands` | The published argv and mode equal the expected argv and mode, held independently in the test (`mcp/gate/selftest.py:39` `EXPECTED_GATES = {`) |
| `refuses_free_form_command`, `refuses_argument_passthrough`, `refuses_shell_injection` | A command string, an extra argument, and an injected `touch` are all refused on the three cargo names |
| `policy_refuses_free_form_command`, `policy_refuses_argument_passthrough`, `policy_refuses_shell_injection` | The same three refusals, aimed at `policy` |
| `conformance_refuses_free_form_command`, `conformance_refuses_argument_passthrough`, `conformance_refuses_shell_injection` | The same three refusals, aimed at `conformance` |
| `run_gate_refuses_the_write_mode_command` | `run_gate` refuses `fmt-fix` by name and by mode (`mcp/gate/selftest.py:326` `"run_gate_refuses_the_write_mode_command",`) |
| `run_fix_refuses_a_check_mode_gate`, `run_fix_refuses_shell_injection` | `run_fix` refuses a check-mode name and an injection string (`mcp/gate/selftest.py:332` `"run_fix_refuses_a_check_mode_gate",`) |
| `refused_run_fix_journals_nothing`, `run_fix_shell_injection_ran_nothing` | The mode refusals added no journal line and created no file |
| `shell_injection_ran_nothing`, `refusals_journal_nothing` | Nothing ran and nothing was journalled |
| `timeout_is_clamped_and_reported` | `timeout_seconds=1` comes back as `60` |
| `fmt_gate_executes_and_reports`, `fmt_gate_writes_nothing` | The real `fmt` exit code and its diff report |
| `fmt_summary_reported` | The `fmt` response carries an applied summary with all four declared counts (`mcp/gate/selftest.py:412` `"fmt_summary_reported",`) |
| `fmt_summary_matches_an_independent_recount` | A recount of the returned output, written without the config's patterns, agrees with the reported counts (`mcp/gate/selftest.py:425` `"fmt_summary_matches_an_independent_recount",`) |
| `no_summary_gate_records_none` | `clippy` and `test` report an unapplied summary with no counts, and `fmt` is the only summarised command (`mcp/gate/selftest.py:505` `"no_summary_gate_records_none",`) |
| `journal_records_the_summary` | The journal row's `summary` equals the response's `summary`, key for key (`mcp/gate/selftest.py:544` `"journal_records_the_summary",`) |
| `clippy_guard_applied`, `clippy_guard_satisfied`, `clippy_passed_requires_guard`, `clippy_marker_in_captured_output` | The guard touched the file, found the marker, and gates `passed` |
| `baseline_predicate_rejects_a_failed_test`, `baseline_predicate_accepts_the_recorded_baseline` | A synthetic record of `{"passed": 158, "failed": 1}` reads `FAIL` and one of `{"passed": 159, "failed": 0}` reads `PASS` (`mcp/gate/selftest.py:126` `"baseline_predicate_rejects_a_failed_test",`) |
| `test_gate_executes_and_reports`, `test_gate_totals` | The real `test` exit code, zero failures, and at least the recorded baseline of passes (`mcp/gate/selftest.py:78` `TEST_BASELINE_PASSED = 159`) |
| `captured_output_has_no_ansi_escapes` | No ESC byte survives into the returned output |
| `journal_grows_one_line_per_invocation`, `journal_records_argv_exit_code_and_timestamp`, `read_audit_log_matches_the_journal_file` | One line per executed call, with the exact argv, exit code and timestamp |
| `journal_marks_every_check_as_a_check` | Every line the run produced carries `"tool": "run_gate"` and `"writes": false` |

Why does the baseline predicate get its own two checks?

Because a totals check that reads the pass count alone calls a suite green while a test fails. The
predicate requires zero failures as well as the baseline count
(`mcp/gate/selftest.py:99` `def baseline_ok(record: dict[str, Any]) -> bool:`), and the two synthetic
records exercise both directions without depending on the day's real totals.

The test holds its own expectation of the eight argv tuples rather than asking the server twice, so a
server that published a wrong allowlist would fail the check instead of validating itself. It holds
its own recount of the `fmt` counts for the same reason.

## Design notes

Why is there no `run_command`, no `--` passthrough, and no cwd argument?

- Keep the execution surface a fixed vocabulary, so a call cannot widen its own access: the
  `COMMANDS` table is the only argv source (`mcp/gate/gate_vocabulary.py:244` `COMMANDS: dict[str, dict[str, Any]] = {name: _gate(name) for name in COMMAND_NAMES}`).
- Split that vocabulary by declared mode and give each half its own tool, so a check surface cannot
  reach a mutation (`mcp/gate/gate_vocabulary.py:248` `FIX_COMMANDS: dict[str, dict[str, Any]] = {`).
- Keep `shell` off and the argv a list, so a metacharacter in a caller value can never become a
  command (`mcp/gate/server.py:400` `completed = subprocess.run(  # noqa: S603 - a fixed argv, never a caller-supplied string`).
- Journal only executed commands, so the journal's line count is itself an audit fact
  (`mcp/gate/server.py:500` `validate_gate(gate)` runs before the journal call).
- Record the tool and the mode on every line, so a mutation is legible in the journal without the
  config (`mcp/gate/server.py:211` `"tool": tool,`).
- Touch one declared file and require its status line, so a cached clippy run cannot be mistaken for
  a clean lint (`mcp/gate/server.py:422` `passed = exit_code == 0 and bool(guard["satisfied"])`).
- Strip ANSI before matching and before returning, so cargo's forced colour cannot break a guard or a
  caller's grep (`mcp/gate/server.py:79` `ANSI_ESCAPE = re.compile(r"\x1b\[[0-9;?]*[ -/]*[@-~]")`).
- Declare the counting rule in the config rather than in the server, so a fork changes one table and
  no code path (`agentic.config.json:58` `"summary": {`).
- Count above the output clamp, so a role reading the response reaches numbers a clamped payload
  would hide (`mcp/gate/server.py:421` `summary = compute_output_summary(command, raw_stdout, raw_stderr)`).
