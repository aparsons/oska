# Problem: conflicting push guidance

On 2026-11-17 two user memories disagreed:
- `push-checks-get-cut-off` says: check once, push normally, never `--no-verify` (the pass stamp makes the pre-push check cheap).
- `spec-191-pr-per-ticket-trial` (2026-11-15) says the push uses `--no-verify` after the implementer's one check.

Open: which one wins. On 2026-11-17 the coordinator briefed agents to push normally, because the pass stamp postdates the trial note.

On 2026-11-17 the coordinator resolved it in favour of pushing normally and updated the `spec-191-pr-per-ticket-trial` memory, marking the old line superseded.
