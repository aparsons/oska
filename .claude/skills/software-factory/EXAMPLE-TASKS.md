# Example tasks

Read this file when you choose the tasks of a factory. Each task has a trigger, an action, and a next trigger. Use only the tasks that solve the user's problem.

## Tasks

| Task | Trigger | Action | Next trigger |
| --- | --- | --- | --- |
| **Explore and triage** | A new ticket arrives. | Read the ticket and the code. Add a category, a state, and notes. Ask the reporter for missing facts. | The ticket gets the "ready for agent" state. |
| **Implement** | A ticket gets the "ready for agent" state. | Write the change and its tests on a new branch. Open a pull request. | A pull request opens. |
| **Automated review** | A pull request opens or changes. | A separate run reviews the diff against the standards. It posts findings. It never approves. | Findings start a rework run. No findings ask a human to review. |
| **Rework** | Review findings or a failed check on a factory pull request. | Fix the findings and push. Stop after 3 tries and hand the work to a human. | The pull request changes. |
| **Decompose a spec** | A spec ticket gets the "ready for agent" state. | Split the spec into small tickets with dependencies. | Each new ticket starts triage. |
| **Ratchet a check** | A clock, one time each day. | Make one file pass a stricter check. Open a small pull request. | A pull request opens. |
| **Fix failed factory runs** | A run fails. | Find the cause and propose a fix to the factory. A human always reviews it. It never starts another fix run. | None. |
| **Nightly architecture scan** | A clock, one time each night. | Look for structural problems. File at most a few tickets. | Each new ticket starts triage. |
| **Throttled retro** | A pull request merges. Run on a sample only, for example one in five. | Review the runs behind it and propose environment changes as tickets. | Each new ticket starts triage. |

## Ticket sources

| Source | What it files | What stops duplicates |
| --- | --- | --- |
| **Feedback button** | A ticket with the user's text, the page, and the app version. | A rate limit for each user, and a search for similar open tickets before the run files a new one. |
| **Production monitor** | A ticket for each new error or alert. | A fingerprint of the error. If an open ticket has the same fingerprint, the monitor adds a count to it. |
