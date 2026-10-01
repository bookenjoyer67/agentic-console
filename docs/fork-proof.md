# Fork proof

This repository is a fork of an agentic quality-gate kit. The kit claims it is portable, and this file is the
evidence rather than the claim: the kit's own instrument runs here, over this repository, and decides whether
the fork is real.

    bash scripts/port-self-test.sh --fork

## The seam table changed after this document was measured

The check this file is about was rewritten after the run recorded below, and the change is larger than a
refactor. It no longer reads one `*_defaults` map holding the FIRST ancestor's values; it reads a declared
`port.seams` list plus the whole `port.ancestors` chain, and it derives the files it reads from the tree's
wiring rather than from a hand list of seven. Three defects went with the old shape, and each one made this
instrument quieter than it looked: it could not see a value two generations back, because its baseline was
the first ancestor and not the parent; it could not see a wiring file nobody had listed, which is how
`scripts/start-mcp-servers.sh` kept the reference project's image name while the check stayed green; and it
had nowhere to record a value an ancestor legitimately shares, so such a value would have been reported as
drift and the map was hand-trimmed to avoid that, which is what made it incomplete.

Every section below that quotes a measurement -- the instrument's own output, the two columns of the seam
table, the console's dump, the pass counts -- was taken against the earlier check. They are marked here
rather than deleted, and not restated from memory: they need re-measuring against the check that is in the
tree now. `docs/DOC-STYLE.md` states the rule this follows: do not delete the claim, and do not invent
authority for it.

## What the instrument measures

`scripts/port-self-test.sh` does three things, and the third only exists in `--fork` mode:

1. `the loader` — the seam table resolves from this repository and parses.
2. `consumers read the config (mutation proof)` — five consumers are shown to follow the seam table rather than
   a hardcoded value.
3. `fork mode` — every seam recorded in `port.komun_defaults` must differ from the value the reference project
   shipped, and the old literal must be gone from seven consumer files.

That third section is why this repository cannot be a copy with a renamed directory: a value that still equals
the recorded default fails, and so does a fallback left behind in a consumer. The message the check prints for
that case names it exactly: *a fallback equal to the old default is the half-wired case this check exists to
catch*.

## The run

    == 1. the loader ==
      ok    loader resolves its source: /home/computing/agentic-console/agentic.config.json
      ok    loader emits valid JSON carrying schema_version
    == 2. consumers read the config (mutation proof) ==
      ok    mcp/gate/server.py follows the config
      ok    scripts/classify-change.py follows the config
      ok    scripts/validate_doc_conformance_deterministic.py follows the config
      ok    eval/test_policy.py follows the config
      ok    scripts/run-agent.sh follows the config
    == 3. fork mode: seams still carrying a Komun default ==
      ok    no Komun default survives in the config or in a consumer

    port-self-test: PASS

The plain run passes too, so the fork proof does not come at the cost of the ordinary reading.

## Every seam, before and after

| seam | the reference project | this repository |
| --- | --- | --- |
| `project.name` | `komun` | `agentic-console` |
| `toolchain.commands.test.argv` | `cargo test --workspace` | `cargo test --release` |
| `toolchain.commands.clippy.guard.marker` | `Checking komun-server` | `Checking agentic-console` |
| `toolchain.commands.clippy.guard.marker_regex` | `\bChecking\b\s+(?P<marker>komun-server)\b` | `\bChecking\b\s+(?P<marker>agentic-console)\b` |
| `toolchain.commands.clippy.guard.touch_file` | `crates/server/src/main.rs` | `src/main.rs` |
| `containers.base_image` | `agent-sandbox:komun` | `agent-sandbox:console` |
| `containers.tools_image` | `agent-sandbox:komun-m3` | `agent-sandbox:console-m1` |
| `containers.registry_volume` | `komun-cargo-registry` | `console-cargo-registry` |
| `containers.networks.internal` | `agent-internal` | `console-internal` |
| `containers.networks.broker` | `agent-net` | `console-net` |
| `containers.broker.name` | `rev-broker` | `console-broker` |
| `artifacts.project_key` | `proj-komun` | `proj-console` |
| `console.container` | `agent-rev-m3` | `agent-console-m1` |
| `console.role_container_prefix` | `agent-rev-m4-` | `agent-console-m4-` |
| `console.ports` | 8001, 8002, 8003 | 8101, 8102, 8103 |
| `console.evidence_dir` | `~/komun-agent-exercise-4-3` | `~/console-run-evidence` |
| `console.briefs_dir` | `~/komun-agent-exercise-4-3/briefs` | `~/console-run-evidence/briefs` |
| `console.ci_jobs` | five jobs | one job, `gate` |

The clippy guard is the interesting one. It exists because a cached linter prints nothing and exits 0, which is
indistinguishable from a clean lint. In the reference project the guard proved itself by touching a Rust source
file in a cargo workspace. Here it touches `src/main.rs`, because that is what this repository gates.

## The one key that left the seam list, and why

`artifacts.style_rules` is gone from `port.komun_defaults`. It is the only removal, and the reason is not
convenience: the value is the path `docs/DOC-STYLE.md`, this repository carries its own file at that same path,
and the value is therefore no longer inherited from the reference project. Leaving the key would have flagged
eleven legitimate mentions of a path that belongs to this repository. Removing any other key would have been
the half-wiring the check exists to catch, which is why it is the only one.

## A documented deviation

`scripts/agentic_config.py` embeds a fallback copy of the seam table. It no longer embeds the two
reference-defaults maps (`port.komun_defaults` and the console block's list), because the key name itself
carries the string the fork check searches for. The file says so at the top of the table, and the omission is
inert: the only reader of those maps is `scripts/port-self-test.sh`, and it reads them from
`agentic.config.json`. Nothing else in the repository reads them.

## What still names the reference project

`docs/iteration-log.md`, `docs/calibration-log.md`, `docs/agent-rubric.md`, `docs/context-management/**`,
`docs/clippy-gate/iteration-log.md`, `docs/adr/ADR-001-*.md` and `docs/DOC-STYLE.md` name the reference project
because they record runs that happened against it. They are this pipeline's provenance, and a fork does not
rewrite history. The fork check does not scan them, and neither does any consumer.

## The visible proof

The console reads the seam table and labels every value as this repository's or still the reference project's.
Pointed at this repository, it draws:

    config seams (value, and whether it is still a Komun default):
        artifacts.project_key              proj-console             changed for this repo / generic
        containers.base_image              agent-sandbox:console    changed for this repo / generic
        containers.broker.name             console-broker           changed for this repo / generic
        containers.networks.broker         console-net              changed for this repo / generic
        containers.networks.internal       console-internal         changed for this repo / generic
        containers.registry_volume         console-cargo-registry   changed for this repo / generic
        containers.tools_image             agent-sandbox:console-m1 changed for this repo / generic
        project.name                       agentic-console          changed for this repo / generic
        toolchain.commands.clippy.guard.*  Checking agentic-console changed for this repo / generic
        console.container                  agent-console-m1         changed for this repo / generic
        console.ports                      gate 8101, storage 8102, retrieval 8103
        console.evidence_dir               ~/console-run-evidence   changed for this repo / generic
        console.briefs_dir                 ~/console-run-evidence/briefs
        console.session_dir                /root/.claude/projects/-workspace
        console.ci_jobs                    1 jobs                   changed for this repo / generic
        console.claude_command             claude                   still a Komun default
        console.orchestration_steps        8 steps                  still a Komun default

Two seams still read as the reference project's, and both are honest. `claude_command` is the agent binary, and
it is named `claude` in both repositories. `orchestration_steps` is the eight-step sequence with its two human
checkpoints, which this repository keeps deliberately: the pipeline is the same pipeline.

    agentic-console --repo /path/to/this/repo --dump

## The fork's own runtime

The fork names its own resources, so it runs beside the reference project instead of colliding with it:

    container   agent-console-m1
    image       agent-sandbox:console-m1     (tagged from the proven agent-sandbox:komun-m3)
    volume      console-cargo-registry       (seeded, because the container cannot fetch crates)
    networks    console-internal, console-net
    broker      console-broker
    ports       8101 gate, 8102 storage, 8103 retrieval

## Outstanding

- the adapted pipeline workflow. `.github/workflows/ci.yml` here is the console's own gate (format, lint, test).
  The reference project ran five pipeline jobs from its workflow: a change classifier, a policy suite, an
  evaluation harness, an advisory review and an audit trail. Those jobs belong in this repository too, pointed at
  this repository's commands and documents.
- the broker container, which needs the same credential *names* the reference project's broker holds. Values are
  never read, printed or copied by hand.

## The instruments, after the port

All four run in this repository, and these are their verdicts. `pytest` lives in the sandbox image rather than
on the host, so the suites run inside the container against a copy of the tree.

| instrument | command | verdict |
|---|---|---|
| the fork check | `bash scripts/port-self-test.sh --fork` | PASS |
| the pipeline's suites | `python3 -m pytest eval/test_policy.py eval/test_deterministic_step.py -q` | 90 passed |
| the console's tests | `cargo test --release --offline` | 52 passed |
| the conformance gate | `python3 scripts/run-conformance-gate.py` | verdict pass, 12 files checked |

Two governed documents changed with the fork, and both are cleaner than the copies they replace.

| document | the reference repository | this repository |
|---|---|---|
| `AGENTS.md` | 225 lines, 11 conformance findings, 34 of 39 citations resolved | 166 lines, 0 findings, 32 of 32 resolved |
| `CLAUDE.md` | 135 lines, 18 findings, 0 of 1 citations resolved | 135 lines, 17 findings, 1 of 1 resolved |

The seventeen findings that remain in `CLAUDE.md` are the style debt the reference copy already carried.

## The runtime the fork ships

The `sandbox/` directory came across as the pipeline's container runtime. Fifty-six spots in it named the
reference project. The ones that decide behaviour were re-pointed by hand, because the kit's check reads
`scripts/run-agent.sh` and not `sandbox/run-agent.sh`.

| file | what changed |
|---|---|
| `sandbox/run-agent.sh` | the engine: workspace default, state directory, network, agent image, broker image, broker name, registry volume |
| `sandbox/stage-secrets.sh` | the state directory it stages key material into |
| `sandbox/opencode-sandbox.json` | the broker URL the container's tooling calls |
| `sandbox/Dockerfile.m3` | the base image it builds from |
| `sandbox/broker/broker.py` | the broker's own name, in its docstring, its user agent and its health reply |

Two files keep their names: `sandbox/README-m3.md` and `sandbox/run-agent-m3.sh`. They record the Module 3
sandbox exercise, and their checkpoint answers quote measurements taken under those names. Rewriting them
would falsify a record, so they stay as measured.

**A blind spot worth knowing.** The fork check scans seven consumer files. The launcher among them is
`scripts/run-agent.sh`, the per-role wrapper. The engine that wrapper calls, `sandbox/run-agent.sh`, carries
its own defaults and is not scanned. The wrapper pins the values, so this repository's own path is correct,
but a reader who runs the engine directly would meet the reference project's defaults. Those defaults were
re-pointed by hand rather than by the check. Adding the engine to the check's file list would catch this class
of drift; that is the kit owner's call, not a change to make quietly.

**One name to settle before the first run.** The console reads its container name from `console.container` in
`agentic.config.json`, which says `agent-console-m1`. The launcher derives a container name from the workspace
directory, giving `agent-agentic-console`. Either the runtime is created as `agent-console-m1`, or the config
follows the launcher. Settle it once, or the console watches a container that does not exist.

## The retrieval harness, in this repository

The harness is `mcp/retrieval/run_ground_truth.py`. It queries the retrieval server and judges each answer
against a ground-truth document, with an 80 percent floor as its exit status. It carried two things from the
reference project that cannot work here: a default project id of `proj-komun`, and a corpus that did not exist
in this repository.

Both are fixed honestly. The project id is `proj-console`. The corpus is seven documents under
`.memory/reference/`, written in the same front-matter schema as the reference corpus, each one resting on
facts recorded in this repository. The ground-truth document has eight queries, all answerable from that
corpus. Its passed rate is measured, not asserted:

```
indexed 73 chunks from 7 documents in .memory/reference
pass rate: 8/8 (100.0%) against the 80% floor
HARNESS_RESULT passed=8 total=8 rate=100.0 floor=80.0
HARNESS_EXIT_STATUS=0
```

The rate depends on the embedding model, and that is worth stating plainly: the run above uses
`BAAI/bge-small-en-v1.5` with the retrieval query prefix, which is what the workflow sets. Against the
server's own default model the same corpus scores 6 of 8, below the floor. The floor, the threshold and every
query were left exactly as the reference project had them; nothing was lowered to reach a pass.

## CI, running the whole pipeline

The workflow runs on GitHub's runners, and its own artifacts are the evidence. Run `36748467830` on pull
request 1: all five jobs green, four minutes end to end, every step executed rather than skipped.

| job | what it did on the runner |
|---|---|
| Change Classifier | classified the changed files and published the change type |
| Policy Test Suite | resolved the config, built the image pair, ran the policy suite inside the tools image: 90 of 90 |
| Evaluation Harness | fetched the crates, ran the three cargo gates offline (3 of 3), the conformance gate, and the retrieval harness against this repository's corpus: 8 of 8, 100 percent, against the 80 percent floor, with `BAAI/bge-small-en-v1.5` |
| Advisory Code Review | ran as advisory and non-gating. It reports `not_run` until a repository secret supplies `OPENROUTER_API_KEY`, which is what the reference workflow does too |
| Audit Trail | consumed the six artifacts and wrote the trail for the pull request's merge sha |

Six artifacts came back, and they are the receipt: `policy-report`, `deterministic-report` with its
`conformance-report`, `retrieval-report` with its log and audit log, `advisory-review-report`,
`change-classification` and `audit-trail`.

The retrieval run is the one worth reading twice. The harness is this repository's own quality gate over its
own corpus, and it passed on a hosted runner with no local state: the embedding model downloaded, the server
indexed 7 documents, and 8 of 8 queries resolved above the floor.
