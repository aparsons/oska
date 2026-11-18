# Problem: refreshing every open PR after each merge serialises the Pi

On 2026-11-17 the merge rule (user memory `merge-prs-when-complete`: after each merge, merge `main` into the other open branches, re-check and re-push) cost one full `scripts/check.sh` on the Pi, about 35 minutes, per open PR per merge. With four PRs open at once (#356, #357, #358, then #167's), that is the Pi's main cost, ahead of implementation.

Candidate fix (2026-11-17): a GitHub merge queue (or "require branches up to date" plus CI) moves the re-check onto CI runners, so the Pi checks each ticket only once.

Related: [problem-pi-cpu](problem-pi-cpu.md).

On 2026-11-17 the user chose a merge queue. Merge queues are only available to org-owned repos and `aparsons/oska` is owned by a user account, so the coordinator adopted the fallback: refresh with `gh pr update-branch` and let CI re-test; no Pi re-check. The user memory `merge-prs-when-complete` was updated.
