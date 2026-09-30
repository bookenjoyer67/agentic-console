---
classification: confidential
project: proj-console
doc_type: decision
---

# Decision: the sandbox holds no real credential

Where do the provider credentials live, and what does the agent container receive instead?

The sandbox runs an agent container beside a credential broker, and the broker is the only thing the agent can reach: the agent container sits on an internal docker network with no route off the host (`sandbox/run-agent.sh:5` `--network console-net (--internal, no internet at all)`). The broker holds the provider credentials so the agent container never does (`sandbox/broker/broker.py:2` `holds the provider credentials so the agent container never does.`). The agent container receives no credential mounts and no real key material, only a dummy token and the broker's in-network URL (`sandbox/run-agent.sh:6` `broker container : console-net + bridge; holds the real keys, injects them upstream`).

## What token does the agent container hold?

The launcher passes a dummy token, `-e ANTHROPIC_AUTH_TOKEN=sandbox-dummy-token`, and mounts no credential file into the agent container (`sandbox/run-agent.sh:91` `-e ANTHROPIC_AUTH_TOKEN=sandbox-dummy-token`). The repository's own guide repeats the rule and says the agent container holds a dummy token only (`AGENTS.md:40` `ANTHROPIC_AUTH_TOKEN=sandbox-dummy-token`).

## Where are the real credentials staged, and what may the broker forward?

The broker's inputs are staged on the host, outside the repository, under the home directory (`sandbox/stage-secrets.sh:14` `STATE="${HOME}/.config/console-sandbox"`). The broker refuses every path that is not a provider API path (`sandbox/broker/broker.py:256` `broker only forwards provider API paths`). The console itself is read-only by construction and offers no command that would move key material (`AGENTS.md:162` `The console is read-only by construction`).
