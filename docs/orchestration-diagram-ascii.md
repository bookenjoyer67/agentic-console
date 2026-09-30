# Orchestration Diagram (ASCII)

Which handoff does each role receive, and what does each one return?

The same gate as `docs/orchestration-diagram.md`, drawn as ASCII boxes for a reader working in a plain-text terminal.

```text
            +-------------------------------------------+
            | Orchestrator                              |
            |                                           |
            | Receives:                                 |
            |   Change request, every subagent return,  |
            |   and both human checkpoint decisions     |
            |                                           |
            | Returns:                                  |
            |   Task briefs to each subagent, and the   |
            |   assembled run summary to the human      |
            +-------------------------------------------+
                                  | handoff: change request + repository path + acceptance criteria
                                  v
            +-------------------------------------------+
            | Planner                                   |
            |                                           |
            | Receives:                                 |
            |   Change request, repository path,        |
            |   acceptance criteria                     |
            |                                           |
            | Returns:                                  |
            |   Ordered plan + file list                |
            +-------------------------------------------+
                                  | handoff: ordered plan + file list
                                  v
            +-------------------------------------------+
            | Human Checkpoint 1 - plan approval        |
            |                                           |
            | Receives:                                 |
            |   Ordered plan + file list                |
            |                                           |
            | Returns:                                  |
            |   Approved plan, amended plan, or halt    |
            +-------------------------------------------+
                                  | handoff: approved plan + file list + AGENTS.md constraints
                                  v
            +-------------------------------------------+
            | Implementer                               |
            |                                           |
            | Receives:                                 |
            |   Approved plan, file list,               |
            |   AGENTS.md constraints                   |
            |                                           |
            | Returns:                                  |
            |   Modified files + change notes           |
        +-->+-------------------------------------------+
        |                         | handoff: modified files + acceptance criteria
        |                         v
        |   +-------------------------------------------+
        |   | Tester                                    |
        |   |                                           |
        |   | Receives:                                 |
        |   |   Modified files + acceptance criteria    |
        |   |                                           |
        |   | Returns:                                  |
        |   |   Gate results with pass/fail evidence    |
        |   +-------------------------------------------+
        |                         | handoff: modified files + review standards
        |                         v
        |   +-------------------------------------------+
        |   | Reviewer                                  |
        |   |                                           |
        |   | Receives:                                 |
        |   |   Modified files + review standards       |
        |   |                                           |
        |   | Returns:                                  |
        |   |   Review report with findings             |
        |   +-------------------------------------------+
        +-------------------------+ handoff: assembled run summary
          rework loop: failing gate results or review findings return to the Implementer
          as a rework brief; repeat to the loop limit, then halt and escalate.
                                  v
            +-------------------------------------------+
            | Project Manager                           |
            |                                           |
            | Receives:                                 |
            |   Assembled run summary                   |
            |                                           |
            | Returns:                                  |
            |   Ticket update confirmation              |
            +-------------------------------------------+
                                  | handoff: run summary + gate evidence + review report
                                  v
            +-------------------------------------------+
            | Human Checkpoint 2 - release approval     |
            |                                           |
            | Receives:                                 |
            |   Run summary, gate evidence,             |
            |   review report                           |
            |                                           |
            | Returns:                                  |
            |   Merge approval or halt                  |
            +-------------------------------------------+
                                  | optional stretch only: one external-documentation question
                                  v
            +-------------------------------------------+
            | Researcher  (optional / stretch)          |
            |                                           |
            | Receives:                                 |
            |   One external-documentation question     |
            |                                           |
            | Returns:                                  |
            |   Findings document with citations        |
            +-------------------------------------------+
```
