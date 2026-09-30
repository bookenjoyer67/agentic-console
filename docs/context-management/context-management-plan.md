# Context-management technique plan

Companion to `docs/context-management/pre-session-plan.md`. Names every context intervention planned
for the managed run, where it fires, and why.

## 1. Explicit context boundaries (two, at the two phase transitions)

A boundary is a written preamble at the top of the first message of a new phase. It restates, before
the instruction: the previous phase is complete, what the next phase focuses on, which rules remain in
effect, which rules changed, and what from the previous phase still matters.

**Boundary 1 — phase A -> phase B (the requirement change).**
Placed here because this is the only point where the rule set changes; everything the agent edited
before it was edited under v1, so this is where an unanchored agent starts reaching back for withdrawn
rules.

> Phase A is complete: the "What this is" and "Critical rules" sections of `AGENTS.md` have been
> edited under documentation standard v1.
>
> Phase B focuses on `docs/DEVELOPMENT.md` ("Prerequisites", "Build order"), under a revised standard.
>
> Rules still in effect: R2 (authority parenthetical), R3 (sentence limit), R4 (imperatives, no hedging).
> Rules changed, effective immediately: R3's limit is now 35 words, not 25; R1 is replaced — a section
> opens with the question it answers, and existing "This section ..." purpose sentences are removed;
> R2 is strengthened — the parenthetical must name the artifact and the literal text, value or count
> that settles the claim. R5 is withdrawn: nesting is allowed where it shows real hierarchy.
>
> What still matters from phase A: the sections you edited now lead with purpose sentences that are no
> longer wanted; they are revisited in phase C, not now. First, revise `docs/DOC-STYLE.md` itself to v2
> so the standard on disk matches the rules above.

**Boundary 2 — phase B -> phase C (the revisit).**
Placed here because phase C asks the agent to revisit earlier work, which is exactly when it otherwise
reasons from whatever state those earlier turns left behind.

> Phase B is complete: `docs/DOC-STYLE.md` is at v2 and the `docs/DEVELOPMENT.md` sections are edited
> under it.
>
> Phase C focuses on the phase-A sections of `AGENTS.md`, then a consistency pass over all four edited
> sections.
>
> Rules in effect: v2 only — 35-word sentences, opening question instead of a purpose sentence,
> strengthened authority parenthetical, nesting allowed. R5 is still withdrawn.
>
> What still matters from phase A: the purpose sentences added then must now be removed and replaced by
> the question each section answers.

Why boundaries: the agent's failure mode in a long session is not forgetting the task, it is applying a
rule that was valid three turns ago; a preamble that names the withdrawn rule is the cheapest way to
make the current rule set the most recent thing it read.

## 2. Proactive summarization (one, at the phase-A -> phase-B boundary)

Fires **before** the revised rules are introduced, so the summary describes the work as it actually
stands and is not contaminated by the change. The summary is produced by invoking
`.claude/skills/summarize-session/SKILL.md` (`/summarize-session`) and must preserve, verbatim where
the item is a rule or a document state:

1. the current goal;
2. the active rules and constraints — R1–R5 copied, not paraphrased;
3. the decisions made so far (which sections were edited, what was changed and why);
4. the current state of the artifact being revised — the **full edited text** of both `AGENTS.md`
   sections, because phase C must edit them again;
5. unresolved questions — claims marked `[UNVERIFIED]`, and anything the agent could not settle;
6. the next planned action.

The summary is read and checked against the working tree before the boundary preamble is sent. It is
saved as `docs/context-management/run-001/session-summary-phase-a.md`, and the paste-visible preamble
plus message log as `docs/context-management/run-001/messages.md`.

Why a summary here, rather than later: this is the natural breakpoint the task already has (the rule
change), it is the last moment at which the pre-change state can be captured accurately, and phase C
depends on the phase-A artifact state — the one thing compaction is known to lose.

## 3. Compaction

Not planned for this run, and not triggered artificially: the exercise does not require filling the
window, and a session that reaches for compaction has already missed a proactive step. If the window
does cross ~60%, the policy is: summarize first with `/summarize-session`, then let `/compact` run only
if usage still climbs, and record the pre/post usage figures plus four recall probes (original task,
active rules, current state of the edited `AGENTS.md` sections, remaining work) in the iteration log.
A crossed threshold is itself a finding about the plan, not a failure of the run.

## 4. What is deliberately NOT done

- No fresh-context handoff session: the exercise requires one managed session, and the handoff
  technique belongs to the next lesson.
- No automated summarization trigger in `CLAUDE.md`: a stretch activity, out of scope for this run.
- No workspace relocation of the earlier iteration log: it is not an answer key for a style task.
  Instead the transcript is checked for reads of it, and the result is recorded in the log entry.
