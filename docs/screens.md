# Screens

Three screens, one primary panel each, and overlays for anything that asks you something. Switch with `1`,
`2`, `3`, `Tab` or `Shift-Tab` — those keys work from every mode, including the ones that ask for a ruling.

## FLOW — the map

The pipeline the repository declares, drawn as four lanes, with live lights where a reading can say
something about a box:

| lane | what it draws |
| --- | --- |
| A | a pull request arriving: the CI jobs parsed from the workflow file, their `needs` chain, and which of them can block a merge |
| B | a brief driving an orchestrated run: the ordered steps, with the human checkpoints marked as decisions |
| C | a repeated agent step being converted into a script, with its review date |
| D | a role box being probed one shot at a time |

Every box names the artifact behind it when it is selected, so the map stays checkable: a box exists because a
file says so, not because the diagram looked better that way.

## LIVE — what is happening now

| panel | what it reads |
| --- | --- |
| run | whether a run is in flight, from a read-only `docker exec <container> ps` |
| checkpoint card | the state table in [`docs/provenance.md`](provenance.md) |
| gate journal | the last 20 gate rows, as a table |
| storage entries | the last entries the run recorded |
| ports | the ports the config declares, and whether each answers |
| gates | the gate server's own `list_gates` |

The checkpoint card is the most important element on the screen. It never decides that a checkpoint is open;
it looks for evidence in a fixed order, names the evidence it used, and prints each line's source and age.

## INSPECT — the machinery

| panel | what it reads |
| --- | --- |
| seams | the config's seam table, each marked as still the kit's or changed for this repository |
| role mounts | the role × mount matrix, so you can see what a role box can and cannot read |
| grants | who may do what |
| suites | the policy and step suites, with their last result |
| conversion candidates | repeated steps with their next review dates |
| ADRs | the accepted decision records |

## Layout and width

The design target is **80 to 100 columns**, and wider terminals get more panels rather than more truncation.
Above 160 columns the layout widens gracefully instead of stretching a column of text.

The known defect is the opposite end: the frame tests were written at 230x60 and 200x50, so a green suite
never exercised a narrow terminal, and at 86x38 the panels cram into each other. Tasks T1.1 to T1.8 in
[`docs/roadmap.md`](roadmap.md) are the repair, and they add a size matrix to the frame tests so the narrow
case is covered.

## Overlays

| overlay | opens with | keys inside |
| --- | --- | --- |
| action menu | `a` | `j`/`k` move, `Enter` choose, `Esc` close |
| action input | choosing an action that needs text | type, `Backspace`, `Enter` to review the command, `Esc` cancel |
| confirmation | `Enter` on a checkpoint, or reviewing an action | `y` or `Enter` runs it, `n` or `Esc` cancels |
| ruling chooser | `e` | `1`-`9` or `Enter` picks a canned ruling, `j`/`k` move, `c` free text, `Esc` close |
| action log | `L` | `L` again hides it |
| key reference | `?` | any key closes it |

Every action shows the exact command before it runs. The preview is the argv, not a description of the argv.
