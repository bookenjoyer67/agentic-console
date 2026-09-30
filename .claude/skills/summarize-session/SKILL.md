---
name: summarize-session
description: >
  Produces a structured summary of the current session's goal, rules, decisions, artifact state, open
  questions and next action, for continuing a long session with a clean working context. Use when a
  phase is ending and new rules or new material are about to arrive, when the context window is
  filling, or when the user asks for a session summary.
---

# Skill: Summarize Session

When this skill is invoked, produce a structured session summary using exactly the format below. The
summary is the working context the session continues from, so accuracy matters more than brevity.

Do not summarize or paraphrase rules, acceptance criteria or file contents. Copy them verbatim. A
summary that says "the style rules" instead of listing them has destroyed the information it was
written to preserve.

## SESSION SUMMARY

### Current Goal
(One paragraph: what this session is trying to produce and why.)

### Active Rules and Constraints
(The full, current rule set. Copy it verbatim from the standard or the user's message. If rules
changed during the session, state the current rules only — the superseded versions are not part of the
current state, but say explicitly which rules are no longer in effect.)

### Decisions Made
(What has been decided or produced so far, and why. Include which sections or files were touched and
what was changed in each.)

### Current State of the Artifacts
(For every file or section created or modified, give the section name and its full current text,
verbatim. Do not describe the text — include it, because the next phase edits it.)

### Open Questions
(Anything unresolved: claims that could not be verified, decisions the user has not made, information
that is missing. Name what would settle each one.)

### Next Planned Action
(The single next step, stated as an instruction to whoever continues the work.)

After producing the summary, ask the user to review it for accuracy before proceeding. Do not continue
with any task until the user confirms the summary is correct or provides corrections.
