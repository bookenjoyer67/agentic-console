# Answers — 2.2 Exercise 1 (managed context run, questions 1–4)

Paste-ready text for the LMS boxes. Records the same answers as the Iteration Log entry, written
from the run's own evidence: session `aea13263-d840-4dc6-81d5-4f1e413661a9`
(`~/komun-agent-exercise-2-2/transcript-run-001.jsonl`), the host rule measurement
(`~/komun-agent-exercise-2-2/rule-check-output.txt`) and the pre-run ground truth
(`~/komun-agent-exercise-2-2/ground-truth/`).

---

## Question 1 — Where did the agent's context management break down or hold up better than you expected, and what did that tell you about the technique you chose?

It held up better than I expected at the requirement change. The agent's phase-A work stayed put under
a rule set that had just been replaced, and when it was told to revisit those sections it re-read all
four sections from disk before editing them, then reported six violations with the rule each one broke
and fixed them. The session also compacted itself in the middle of the final phase, and coherence
survived that too — for the same reason: the phase-C instruction forced a fresh read of the files
rather than a reliance on remembered text.

Where it broke down was in the agent's account of its own process rather than in the files. Its closing
report claimed 17 edits; the transcript holds 19 Edit calls, 18 of which succeeded and one of which
failed with "string to replace not found". A second, quieter breakdown was provenance: the sentence
about Docker builds keeps the word "verified" resting on an authority that is an earlier session's log
record (`docs/iteration-log.md:857`), not on output this run produced.

What that tells me about the technique is that restating the rules at a boundary reliably controls what
the agent writes and where it looks, but it does not control what the agent says about itself or where
it inherited a fact from. Both of those need their own explicit rule — a number has to be recounted
from a command whose output the run just saw, and "verified" has to mean verified in this session.

## Question 2 — Why did you place your explicit context boundaries where you did?

Boundary 1 sits at phase A → B because that is the only point in the run where the rule set changes.
`docs/DOC-STYLE.md` goes from v1 to v2 there: the sentence limit rises from 25 to 35 words, R1 is
replaced by a question opener, R2 is strengthened so the parenthetical must quote the literal text,
value or count that settles the claim, and R5 is withdrawn. Everything written before that point was
written under v1, so that is exactly where an unanchored agent starts reaching back for a withdrawn
rule. The boundary therefore says the previous phase is complete, names the sections that follow, lists
the rules that survive, states the four changes, and — the part that turned out to be load-bearing —
says explicitly what from phase A still matters: those sections now lead with purpose sentences that
v2 deletes and with bare-location parentheticals that v2's R2 fails.

Boundary 2 sits at phase B → C because the type of work changes rather than the rules. Phase B produces
new prose; phase C revises prose that was produced two phases earlier under a rule set that no longer
exists. That is a different failure mode — a document that mixes v1 openings with v2 rules — so it gets
its own boundary, and its instruction includes re-reading both sections from the file instead of
working from memory of them.

## Question 3 — What information did your proactive summary preserve?

It preserved the goal, the full current rule set verbatim (the five rules plus the "What counts as a
claim", "Authority forms" and "A claim you cannot trace" sections), both operator rulings on R1's reach
and R4's reach, five decisions with the sections each one touched, the complete current text of the two
edited sections, and six open questions. The host-side check before I confirmed it is the part worth
keeping: the quoted "What this is" text (1,579 characters) and "Critical rules" text (4,434 characters)
were byte-identical to the files on disk and the rules matched `docs/DOC-STYLE.md` exactly, so the
summary was a copy rather than a paraphrase.

What it omitted is worth naming too. At the moment it was taken it recorded that no claims needed
verification, which was true then — the later phases flagged four claims with `[UNVERIFIED]` across
three "Claims needing verification" lists, so the summary describes phase A's state rather than the
run's final state. The larger omission came from outside the plan: Claude Code auto-compacted the
session in the final phase (166,700 tokens down to 12,837), and its generated summary has no
unresolved-questions field at all. It kept the rules and the rulings, and it dropped the category my
summary contract says matters most for continuing a session.

## Question 4 — How well did the agent respond to the requirement change?

It applied the change correctly, in both directions in time. Forward: it revised `docs/DOC-STYLE.md` to
v2 before applying the new rules to anything, so the standard on disk matched the rules it was
following, and it edited the two `docs/DEVELOPMENT.md` sections under v2. Backward: in phase C it
revisited the two sections it had written under v1 and brought them to v2 — the rule measurement over
the four sections shows ten headings with no non-question opener, no surviving "This section" purpose
sentence, no sentence over 35 words, no bare-location parenthetical and no hedging word, so no v1
construct was left unflagged.

It also restated the changed rules in its own words before editing, including R5's withdrawal. Its
consistency pass found and fixed six violations, all in the R2 family: a claim with no authority at
all, an "only" that was wider than its evidence, two authorities pointing at the wrong line, a
cross-file authority, and a quoted literal that did not settle its claim. Two residuals are deliberate:
it kept one fact-shaped bullet without a parenthetical, which is what the Q2 ruling instructed, and it
refused to rewrite "SvelteKit 5" because that would change an assertion, marking it `[UNVERIFIED]` with
what would settle it instead. It also found, and correctly left alone as outside the run's scope, a
contradiction between `docs/DEPLOY.md:16-17` and `crates/server/src/main.rs:123-129`.
