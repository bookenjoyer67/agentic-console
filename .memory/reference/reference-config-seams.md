---
classification: internal
project: proj-console
doc_type: reference
---

# Reference: the seam table where a fork changes a value

Where does a fork change a value, and what proves that the change reached the consumer?

The seam table is the single place where a fork departs from the reference project, and it lives in `agentic.config.json` (`AGENTS.md:44` `The seam table is the single source`). Nothing about a repository is compiled into the console binary: the console is pointed at a repository and reads that repository's own config file (`docs/config.md:3` `The console is pointed at a repository and reads that repository's`). The consumers named in that file are the gate server, the role launcher, the change classifier, the conformance validator and the policy suite (`agentic.config.json:5` `The consumers are:`).

## What happens when the config is missing or unreadable?

Absent or unreadable, every consumer falls back to the values embedded in it, and those embedded values are this repository's (`agentic.config.json:9` `Absent or unreadable, every consumer falls back to the values below, which are this repository's.`). The loader is stdlib only and falls back to its own embedded defaults (`AGENTS.md:91` `The loader is stdlib only, and it falls back to its embedded defaults`). The console reports a missing key rather than inventing a value for it (`docs/config.md:79` `A missing key is reported, not defaulted.`).

## Which seams did this fork change, and which guard proves one?

The project name is `agentic-console` (`agentic.config.json:13` `"name": "agentic-console",`), the project key is `proj-console` (`agentic.config.json:206` `"project_key": "proj-console",`), the tools image is `agent-sandbox:console-m1` (`agentic.config.json:113` `"tools_image": "agent-sandbox:console-m1",`), and the console's container is `agent-console-m1` (`agentic.config.json:288` `"container": "agent-console-m1",`). The clippy cache guard names this crate, so a cached lint is caught rather than trusted (`agentic.config.json:42` `"marker": "Checking agentic-console",`), and the guard touches the console's entry point before the run (`agentic.config.json:44` `"touch_file": "src/main.rs",`).
