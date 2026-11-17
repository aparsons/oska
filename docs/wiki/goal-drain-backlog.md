# Goal: drain the backlog

On 2026-11-17 the user chose to drain the backlog: review follow-ups and the flaky test first, then the open specs one at a time, each merged when green and reviewed (merge policy: user memory `merge-prs-when-complete`).

## Queue (as of 2026-11-17, `aparsons/oska@fc411f9`)
1. #351: follow-ups from the PR #267 review (agent tooling). Dispatched 2026-11-17, Sonnet.
2. #350: follow-ups from the PR #269 review (eval harness). Dispatched 2026-11-17, Sonnet.
3. #349: follow-ups from the PR #263 review (Windows confinement). Queued; security-sensitive, so Opus.
4. #344: flaky kernel segment-permissions test under load. Queued; needs diagnosis.
5. #313 and #314: small agent-ready fixes.
6. Specs: #288 (site follow-ups), #294 (API reference), #295 (release provenance), #296 (versioned docs).

## Ledger
| Date | Item | Agent | Start | PR | Merged | Notes |
|---|---|---|---|---|---|---|
| 2026-11-17 | #351 | Sonnet impl | 00:18 | #352 (00:46) | | check.sh passed on the 2nd run; 1st wrote no stamp (untracked logs) |
| 2026-11-17 | #350 | Sonnet impl | 00:18 | #353 (00:56) | | check.sh failed on the 1st run (dependents list); untracked logs forced a 35 min pre-push re-run |

Related: [problem-pi-cpu](problem-pi-cpu.md).

## Schedules
On 2026-11-17 the user approved two session-only schedules (they expire after 7 days or when the session ends):
- a progress report every 3 hours (job `026fd424`);
- a daily wiki lint at about 08:47 local (job `2e196e7a`).

## Reprioritised 2026-11-17
The user chose critical path first: after #351/#350/#349, run #154 (macOS), #155 (Windows), #209 (Bytes), then a new spec for collections and process launching. Site/release/docs specs fill idle Pi time only. See [research-oska-replacing-rust](research-oska-replacing-rust.md).
