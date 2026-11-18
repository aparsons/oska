# Problem: finished worktrees fill the Pi's SD card

On 2026-11-18 the SD card reached 92% (4.8G free), and the spec agent's first `scripts/check.sh` failed on a full disk. The cause was 14 finished agent worktrees under `.claude/worktrees/`, each still holding a `target/` of 0.7–7.8G. The coordinator deleted the idle `target/` directories (no cargo running), which freed 21G (55% used after).

Rule from then on (2026-11-18): when an agent's PR is pushed and its agent has stopped, the coordinator deletes that worktree's `target/`. Relates to the user memory `pi-disk-budget`.

Related: [problem-pi-cpu](problem-pi-cpu.md).
