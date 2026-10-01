# Keys

Every key, and every mode a key can land in. `1`, `2` and `3` switch screens from **every** mode, including
the confirmation and the ruling chooser, so a screen is never more than one keystroke away.

CONVERSATION is the screen the console opens on: the transcript, and the composer under it. FLOW, LIVE and
INSPECT are drawn in front of it -- an overlay, not a tab you have to leave the conversation to reach.

## The screens

| key | action |
| --- | --- |
| `1` `2` `3`, `Tab`, `Shift-Tab` | switch screen (always; from CONVERSATION this opens an overlay) |
| `i`, `/` | focus the composer, on CONVERSATION |
| `Esc` | close an overlay, back to CONVERSATION; in the composer, keep the text and leave it |
| `j` / `k`, arrows, `PageUp` / `PageDown` | move the selection; scroll INSPECT |
| `g` | back to the first box |

## The composer

| key | action |
| --- | --- |
| `i`, `/` | focus it |
| printable keys | are text; a screen key stops switching screens while you are typing |
| `Backspace`, `Ctrl-U` | delete a character, clear the line |
| `Enter` | review the exact argv this prompt will run |
| `Esc` | leave the composer with the text kept |

`Enter` in the composer does not send anything. It opens the confirmation, in the composer box itself, with
the argv, the guards and the session the turn will use; `y` runs it, `n` puts you back in the text.

## Acting

| key | action |
| --- | --- |
| `a` | the action menu: eight actions, the exact command for each |
| `Enter` | **approve**: with a ruling offered, opens the confirmation for the default canned ruling |
| `e` | the **ruling chooser** |
| `t` | start a brief |
| `r` | re-read every reading now, forcing every probe |
| `L` | show or hide the action log |
| `?` | the key reference |
| `q`, `Ctrl-C` | quit |

`r` matters more than it looks: the console refreshes on its own on `console_probe_cadence.refresh_seconds`,
and a probe is never re-run inside its own TTL. `r` ignores both.

## The ruling chooser

| key | action |
| --- | --- |
| `1`, `2`, `3` | pick a canned ruling as written |
| `j` / `k` | move between the canned rulings |
| `Enter` | pick the selected one, or the default when the selection is unmoved |
| `c` | free text: an empty field, because a free-text ruling is one you wrote |
| `Esc` | close without sending |

The canned rulings and their labels come from `console.rulings` in the seam table, so the wording is yours to
change without a rebuild. A ruling whose `prefill` is true opens the field with its text in place, which is how
"approve with a rework first" and "halt" ask you to name the change before they are sent.

## Modes

| mode | keys |
| --- | --- |
| composer | printable keys are text, `Backspace` deletes, `Ctrl-U` clears, `Enter` reviews the argv, `Esc` leaves with the text kept |
| prompt confirmation | `y` runs the turn, `n` or `Esc` back to the composer with the text kept |
| action menu | `j` / `k` move, `Enter` choose, `Esc` close |
| action input | type, `Backspace`, `Enter` to review the command, `Esc` cancel |
| confirmation | `y` or `Enter` runs it, `n` or `Esc` cancels |
| ruling chooser | as above |
| action log | `L` hides it, `j` / `k` scrolls |

Every action ends at the confirmation, which shows the exact argv it will run. Nothing runs on the keystroke
that chose it -- the prompt included: the composer's `Enter` reviews the turn, it does not send it.
