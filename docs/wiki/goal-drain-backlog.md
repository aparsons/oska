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
| 2026-11-18 | #360 | cloud impl | 05:47Z | #383 (06:17Z) | 06:28Z | the cloud sandbox runs as root and lacked ssh-keygen; a receipt test failed under root on main too, and #383 fixed it |
| 2026-11-18 | #354 | cloud impl | 06:29Z | #387 (06:38Z) | 06:49Z | |
| 2026-11-18 | #355 | — | 06:50Z | — | | not dispatched: the eval thread's PR #384 already closes it |
| 2026-11-18 | #359 | cloud impl | 06:51Z | #393 (06:58Z) | 07:10Z | flagged a NUL-byte wording gap between Unix and Windows (both refuse it) |

Related: [problem-pi-cpu](problem-pi-cpu.md).

## Schedules
On 2026-11-17 the user approved two session-only schedules (they expire after 7 days or when the session ends):
- a progress report every 3 hours (job `026fd424`);
- a daily wiki lint at about 08:47 local (job `2e196e7a`).

## Resumed 2026-11-18
In a cloud project session the user chose "continue critical path". The macOS thread owned #169 to #171 (#169 merged as PR #389), so this session took #360 and the review follow-ups that stay out of `oska-native` and `std.rt`. #361 and #363 were held because they touch `std.rt`. On 2026-11-18 the user also said to auto-merge work that is complete and tested. Before dispatching a ticket, check for an open PR that already closes it ([problem-duplicate-dispatch](problem-duplicate-dispatch.md)).

## Reprioritised 2026-11-17
The user chose critical path first: after #351/#350/#349, run #154 (macOS), #155 (Windows), #209 (Bytes), then a new spec for collections and process launching. Site/release/docs specs fill idle Pi time only. See [research-oska-replacing-rust](research-oska-replacing-rust.md).
