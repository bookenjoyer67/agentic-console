# Screens

Four screens. CONVERSATION is the one the console opens on and the one it is built around: the transcript,
and a composer under it. FLOW, LIVE and INSPECT are drawn *in front of* it — an overlay, so nothing you read
there costs you the conversation, and `Esc` puts it back. Switch with `1`, `2`, `3`, `Tab` or `Shift-Tab` —
those keys work from every mode, including the ones that ask for a ruling.

## CONVERSATION — the transcript, and the composer

This is the only screen whose content the console owns. Everything here is one of three things, and each line
says which:

| the line is | drawn as |
| --- | --- |
| your prompt | your own words, under the label that they are yours |
| the console's own words | the argv it is about to run, what it started, what it read back — labelled as this console's, not a reading |
| the agent's output | the run's own envelopes, streamed as they arrive |

A line from the run is **LIVE** until its identity is found in the run's own session record inside the
container, and **CONFIRMED** once it is. That is the whole point of the screen: the transcript's header counts
them, so a line the console cannot corroborate is never drawn as if it were corroborated — `6 item(s) -- 0
confirmed, 1 live, 4 this console's own` is a true statement about a turn that failed.

The gutter says which it is, one character wide, so the distinction survives every width:

| mark | means |
| --- | --- |
| `✓` | the run's own session record carries this line's identity. The console read it back and found it |
| `·` | the run's own words, not yet found in its record — live, and nothing more is claimed |
| `!` | the line failed: the run's error, or the console's own refusal |
| (blank) | this console's own words. Not a reading, never a claim about the run, and never confirmable |

The reconciliation is automatic and it says so in its own words: `reconciled: 1 line(s) found in
<session>'s own record, read from its tail (docker exec … tail -c 262144 …)`. When it read a window rather
than the whole record and matched nothing, it says *that* instead — a read that could not see the whole
record cannot report a line as absent from it.

The header carries the session the turns belong to, when the run has told the console what it is, and the
model. It carries the same buffer line on **every** screen, not only this one, drawn first on its row so
that a long checkout path cannot push it off the glass; on FLOW, LIVE and INSPECT it is the one line that
still says how much of the console's own transcript is corroborated. A turn stopped by the account's rate
limit is drawn as a **failure**, with the console's own refusal under it, because that is what it is.

The composer is where a turn starts. `i` or `/` focuses it, `Enter` reviews the exact argv, `y` runs it. While
a turn is in flight a second prompt is refused rather than queued, and the refusal names the process, its
elapsed time and its short id.

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
| scorecard | the Azathoth scorecard artifact (`console.evidence_dir/scorecards/redteam-scorecard.json`), ranked per revision, with the artifact's own sentence about the axes it does not measure |
| ADRs | the accepted decision records |

## Layout and width

The design target is **80 to 100 columns**, and wider terminals get more panels rather than more truncation.
Above 160 columns the layout widens gracefully instead of stretching a column of text.

INSPECT is one scrolled document, so it has a row budget: the panel measures its own document with the
renderer that draws it, clamps the scroll to its last row (a past-the-end scroll lands on the last row rather
than on a blank panel), and its title carries the row position and how many rows remain below the window.
A row that does not fit is therefore announced and still reachable, never silently cut.

The size matrix is in the frame tests: every screen is rendered at 80x24, 86x38, 100x30, 120x40, 160x50 and
230x60, so a green suite exercises the narrow terminals and not only the wide ones. That closes the defect
tasks T1.1–T1.8 in [`docs/roadmap.md`](roadmap.md) were written for. Below 24 rows or so a pane is dropped
**loudly** — the frame says which one and why — because content that does not fit is announced, never
silently cut. A footer's key line is not exempt: when the terminal cannot hold it, the row it frees says so
and `?` still opens the full reference.

## Overlays

FLOW, LIVE and INSPECT are overlays too — full screens drawn in front of CONVERSATION, opaque (nothing the
conversation wrote shows through them), with their own title row saying what they are and that `Esc` closes
them. The conversation underneath keeps its state while you read one, and switching between them is one
keystroke.

| overlay | opens with | keys inside |
| --- | --- | --- |
| FLOW | `1` | `j`/`k` select a box, `Esc` back to CONVERSATION |
| LIVE | `2` | `L` shows or hides the action log, `Esc` back |
| INSPECT | `3` | `j`/`k` scroll, `Esc` back |
| action menu | `a` | `j`/`k` move, `Enter` choose, `Esc` close |
| action input | choosing an action that needs text | type, `Backspace`, `Enter` to review the command, `Esc` cancel |
| confirmation | `Enter` on a checkpoint, or reviewing an action | `y` or `Enter` runs it, `n` or `Esc` cancels |
| ruling chooser | `e` | `1`-`9` or `Enter` picks a canned ruling, `j`/`k` move, `c` free text, `Esc` close |
| action log | `L` | `L` again hides it |
| key reference | `?` | any key closes it |

Every action shows the exact command before it runs. The preview is the argv, not a description of the argv.
