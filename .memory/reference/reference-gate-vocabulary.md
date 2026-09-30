---
classification: internal
project: proj-console
doc_type: reference
---

# Reference: the gate vocabulary and its two tables

Which commands can this repository's gate server run, and where is that list kept?

The command names under `toolchain.commands` in `agentic.config.json` are the gate vocabulary: a fork adds a command by adding one entry there and changing no Python (`mcp/gate/gate_vocabulary.py:8` `ARE the vocabulary: a fork adds a command by adding one entry there and`). The file declares eight commands, and each declares its mode with a `writes` boolean (`mcp/gate/gate_vocabulary.py:12` `The vocabulary is split on that boolean into two`).

## Which commands are gates, and which one writes?

Seven commands are check-mode gates and one is a write-mode fix command. The vocabulary is split on the `writes` boolean into two disjoint tables (`mcp/gate/gate_vocabulary.py:245` `GATES: dict[str, dict[str, Any]] = {` and `mcp/gate/gate_vocabulary.py:248` `FIX_COMMANDS: dict[str, dict[str, Any]] = {`). The check-mode gates are test, clippy, fmt, policy, conformance, webcheck and webtest; the write-mode command is fmt-fix, which rewrites files. Neither tool can reach the other's table (`mcp/gate/gate_vocabulary.py:251` `GATE_NAMES: tuple[str, ...] = tuple(GATES)`).

## Which gate carries a cache-hit guard, and why?

The clippy gate carries a guard because a cached clippy run prints nothing and exits 0, which is indistinguishable from a clean lint (`agentic.config.json:45` `a cached clippy run prints nothing and`). The guard touches the console's entry point before the check (`agentic.config.json:44` `"touch_file": "src/main.rs",`), so a cached run cannot pass as a clean one.

## Which probe asks the running gate server for its own gate list?

The console's cadence table tunes one probe called gate_allowlist, whose time to live is 30 seconds (`agentic.config.json:377` `"gate_allowlist": 30`). It is the expensive one, because it asks the running gate server for its own gate list and costs about a second and a half (`docs/config.md:54` `the expensive one, which asks the running gate server for its own gate`).
