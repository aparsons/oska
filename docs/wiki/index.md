# Chief-of-staff wiki

A dated historical record kept by the coordinating agent. Pages never describe the present; every claim carries a date or commit. Sources (issues, PRs, memory files) are cited, not copied.

## Goals
- [Drain the backlog](goal-drain-backlog.md): the goal set on 2026-11-17, with its queue and ledger.

## Research
- [Oska replacing Rust](research-oska-replacing-rust.md): estimates and critical path, 2026-11-17.

## Decisions
- [Wiki location](decision-wiki-location.md): why the wiki lives in `docs/wiki/`.

## Recurring problems
- [Logs in the worktree defeat the pass stamp](problem-logs-in-worktree.md): two agents lost a check run to it on 2026-11-17.
- [Pi CPU is the bottleneck](problem-pi-cpu.md): limits on concurrency.
- [Conflicting push guidance](problem-push-guidance.md): `--no-verify` advice that contradicted itself (resolved 2026-11-17).
