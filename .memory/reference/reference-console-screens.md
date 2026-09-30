---
classification: public
project: proj-console
doc_type: reference
---

# Reference: the console's three screens

What are the console's three screens, and how does an operator switch between them?

The console draws three screens, one primary panel each, and overlays for anything that asks you something (`docs/screens.md:3` `Three screens, one primary panel each, and overlays for anything that asks you something.`). The keys `1`, `2`, `3`, `Tab` and `Shift-Tab` switch between the screens, and those keys work from every mode, including the ones that ask for a ruling (`docs/screens.md:4` `those keys work from every mode, including the ones that ask for a ruling.`). The three screens are FLOW, LIVE and INSPECT, on keys `1`, `2` and `3` (`AGENTS.md:120` `The three screens are FLOW, LIVE and INSPECT, on keys`).

## What does the FLOW screen draw?

FLOW is the map of the pipeline the repository declares, drawn as four lanes with live lights. Lane A is a pull request arriving, with the CI jobs parsed from the workflow file and their `needs` chain (`docs/screens.md:13` `the CI jobs parsed from the workflow file`). Lane B is a brief driving an orchestrated run, with the ordered steps and their human checkpoints marked (`docs/screens.md:14` `with the human checkpoints marked as decisions`).

## What does the LIVE screen read?

LIVE shows what is happening now: whether a run is in flight, the last 20 gate-journal rows, the last storage entries, the config's ports, and the gate server's own `list_gates`. The checkpoint card is the most important element on the screen (`docs/screens.md:32` `The checkpoint card is the most important element on the screen.`).

## What does the INSPECT screen read, and what is the width target?

INSPECT shows the machinery, and its first panel is the config's seam table (`docs/screens.md:39` `the config's seam table, each marked as still the kit's or changed for this repository`). The design target is 80 to 100 columns, and wider terminals get more panels rather than more truncation (`docs/screens.md:48` `The design target is **80 to 100 columns**`). Every action shows the exact command before it runs (`docs/screens.md:67` `The preview is the argv, not a description of the argv.`).
