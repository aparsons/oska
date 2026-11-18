# Chief-of-staff wiki

A dated historical record kept by the coordinating agent. Pages never describe the present; every claim carries a date or commit. Sources (issues, PRs, memory files) are cited, not copied.

## Goals
- [Handoff 2026-11-18](handoff-2026-11-18.md): rules, queue and state for the next session. Start here.
- [Drain the backlog](goal-drain-backlog.md): the goal set on 2026-11-17, with its queue and ledger.

## Forecasts
- [Forecast 2026-11-18](forecast-2026-11-18.md): critical-path dates and KPIs.

## Research
- [Oska replacing Rust](research-oska-replacing-rust.md): estimates and critical path, 2026-11-17.

## Decisions
- [Eval escape detection fails closed](decision-fail-closed-escape-detection.md): 2026-11-18, after two rounds of bypasses in #380.
- [AI-tooling research follow-up](decision-ai-tooling-followup.md): what shipped, and the plan for the rest, 2026-11-18.
- [macOS libSystem calling path](decision-macos-libsystem-design.md): six design decisions accepted 2026-11-17.
- [Wiki location](decision-wiki-location.md): why the wiki lives in `docs/wiki/`.

## Recurring problems
- [Finished worktrees fill the SD card](problem-disk-fills.md): 92% full on 2026-11-18; idle `target/` directories cleaned.
- [The worktree command guard refuses ordinary shell forms](problem-worktree-guard.md): compound git commands and heredocs, 2026-11-17.
- [Finished agents keep re-reporting](problem-agent-repeat-handbacks.md): repeated hand-backs on 2026-11-17.
- [Refreshing every open PR after each merge serialises the Pi](problem-refresh-after-merge.md): about 35 Pi-minutes per PR per merge.
- [Logs in the worktree defeat the pass stamp](problem-logs-in-worktree.md): two agents lost a check run to it on 2026-11-17.
- [Pi CPU is the bottleneck](problem-pi-cpu.md): limits on concurrency.
- [Conflicting push guidance](problem-push-guidance.md): `--no-verify` advice that contradicted itself (resolved 2026-11-17).

Lint 2026-11-17: all 7 pages are listed above. The "Open"/"Queued" wording in linked pages is dated and superseded by notes on those pages. Related research not yet summarised here: `docs/research/2026-11-oska-text-tools.md`, `docs/research/agent-scripting-python-vs-oska.md` (untracked as of 2026-11-17).

## Lint 2026-11-18 (`origin/main@fd7696a`)
- Orphan fixed: [problem-duplicate-dispatch](problem-duplicate-dispatch.md) (2026-11-18) was missing from the problem list above. [handoff-2026-11-18](handoff-2026-11-18.md) and [forecast-2026-11-18](forecast-2026-11-18.md) are listed above.
- Superseded: "all 7 pages are listed above" (2026-11-17); at `fd7696a` there are 17 pages including this index.
- Superseded: the two research notes called untracked on 2026-11-17 are tracked at `fd7696a` (`docs/research/2026-11-oska-text-tools.md`, `docs/research/2026-11-agent-scripting-python-vs-oska.md`); still not summarised here.
