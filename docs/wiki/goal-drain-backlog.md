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
| 2026-11-17 | #351 | Sonnet impl | 00:18 | #352 (00:46) | 2026-11-17, first try; follow-ups #354 | check.sh passed on the 2nd run; 1st wrote no stamp (untracked logs) |
| 2026-11-17 | #350 | Sonnet impl | 00:18 | #353 (00:56) | 2026-11-17 after a Haiku refresh on main (clean); follow-ups #355 | check.sh failed on the 1st run (dependents list); untracked logs forced a 35 min pre-push re-run |
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
In a cloud project session the user chose "continue critical path". The macOS thread owned #169 to #171 (#169 merged as PR #389), so this session took #360 and the review follow-ups that stay out of `oska-native` and `std.rt`. #361 and #363 were held while the macOS thread might be in `std.rt`; at 07:27Z it reported it was finished and had touched neither, so they were cleared. On 2026-11-18 the user also said to auto-merge work that is complete and tested. Before dispatching a ticket, check for an open PR that already closes it ([problem-duplicate-dispatch](problem-duplicate-dispatch.md)).

## Reprioritised 2026-11-17
The user chose critical path first: after #351/#350/#349, run #154 (macOS), #155 (Windows), #209 (Bytes), then a new spec for collections and process launching. Site/release/docs specs fill idle Pi time only. See [research-oska-replacing-rust](research-oska-replacing-rust.md).

## Spec #154 delivery shape (2026-11-17)
The coordinator chose one PR per ticket to main for spec #154, carrying over the #191 trial's lesson (the user memory `spec-191-pr-per-ticket-trial`). Reasons: macOS behaviour is CI-only from the Pi, so each ticket should reach macOS CI early, and long integration branches conflicted in 3 of 4 merges in the baseline. Ticket graph as of 2026-11-17: 167←165(closed); 168←165; 169←168; 170←169; 171←170,167; 172←171,170; 173←172; 174←173,167.

## Lint notes (2026-11-17, `origin/main@bb8b9ac`)
- Superseded: the "Queued" and "Dispatched" labels in the Queue above are states as of 2026-11-17 at `fc411f9`, not present state. At `bb8b9ac`, PRs #352 and #353 are merged (2026-11-17), issue #351 closed 2026-11-17 and #350 closed 2026-11-18 per `gh`; #349, #344, #313 and #314 not re-checked in this lint.
- The Queue's order is superseded by "Reprioritised 2026-11-17" above for work after #349.
- Related: [problem-logs-in-worktree](problem-logs-in-worktree.md), [problem-refresh-after-merge](problem-refresh-after-merge.md), [problem-push-guidance](problem-push-guidance.md), [decision-wiki-location](decision-wiki-location.md).

## Progress report, 2026-11-17 (first scheduled run)
- Merged: #352 (#351) and #353 (#350); both merged first time after review.
- Open: #356 (#349; ADR 0009 fix being pushed), #357 (env fix plus wiki; CI on the refreshed branch), #358 (#168; Opus review). #167 being implemented.
- Follow-ups filed instead of blocking merges: #354, #355, #359, #360, and a comment on #289.
- 2,214 `#[test]` functions in crates/ at `origin/main` on 2026-11-17: the baseline for later test counts.
- Subagent tokens so far about 0.9M (Opus ~480k, Sonnet ~360k, Haiku ~50k); estimated $5–$25 at assumed list prices.

## #169 design first (2026-11-17)
The PR #358 review found the `Platform` table cannot express libSystem calls. On 2026-11-17 the user approved a design pass before implementing #169: an Opus agent is writing docs/research/2026-11-macos-libsystem-calls.md (plus a Proposed ADR if needed) as a docs PR. Its open decisions go to the user one at a time.

## 2026-11-17 later
- #356 (#349) merged after the ADR 0009 fix and CI on the updated branch. All four review follow-ups (#349, #350, #351, plus PR #357's env fix) are merged.
- #358 (#168): test for `linux_then` added at 5c38ffa; updated from main; CI re-testing.
- #167: PR #362 opened 02:59 UTC; under Sonnet review. The macOS errno table is left for #170 (comment there).

## Progress report, 2026-11-18 ~03:15 UTC (second scheduled run)
- Merged since the first report: #357 (env fix plus wiki), #356 (#349), #358 (#168; #168 closed by hand because the PR didn't auto-close it). All four review follow-ups (#349–#351) are done.
- First try: #357 yes. #356 needed one doc fix (ADR 0009, after the user's decision). #358 needed one review fix (a test for `linux_then`).
- In flight: #364 (research notes; auto-merges if CI is green), #362 (#167; reviewed clean, conflict with #356 being resolved by Opus), the #169 design note (Opus).
- Follow-ups filed: #361 (#358 review), #363 (#362 review); macOS gaps commented on #167, #169, #170 and #171.
- Subagent tokens since the first report about 0.65M; cumulative about 1.55M; estimated cumulative cost $10–$45 at assumed list prices.

## 2026-11-18 ~04:00 UTC
- #364 (research notes) and #362 (#167) merged; #167 closed.
- #365 (design) under independent Opus review; #366 (Linux-only reshape) being implemented by Opus. See [decision-macos-libsystem-design](decision-macos-libsystem-design.md).
- On 2026-11-18 the user asked for #169 to start once #366 merges (Opus implementer, design from PR #365).
- On 2026-11-18 the user asked for #170 (macOS runtime through libSystem) to start once #169 merges.
- On 2026-11-18 the user asked for the eval-harness tickets to start once the Pi is idle. The coordinator defined idle as a 1-minute load average below 4 (on 4 cores) for 3 straight minutes, and runs at most one harness implementer beside the macOS chain. Frontier on 2026-11-18: #214 and #215 (both unblocked); #216 needs both; #217 needs #216. #355 (oska-eval follow-ups) goes with #214.
- On 2026-11-18 the user asked for #216 to start once #214 and #215 merge.
- On 2026-11-18 the user asked for #217 to start once #216 merges. #217 ends with the maintainer running a costed live `claude -p` batch, so the coordinator will have an agent build the driver and docs, then show the cost plan and ask the user before any live run.
- On 2026-11-18 the user asked for #171 (macOS confined files) to start once #170 merges. #171 is blocked by both #170 and #167; #167 closed on 2026-11-18.
- On 2026-11-18 the user asked for #172 (full parity and native by default on macos-aarch64) to start once #171 merges. #172 is blocked by #171 and #170.
- On 2026-11-18 the user asked for #173 (macos-x86_64) to start once #172 merges. After it, only #174 (docs, notarization guidance, v0.5.0) remains in spec #154; #174 is blocked by #173 and #167.
- On 2026-11-18 the user asked for #174 to start once #173 merges. Per the user memory `merge-prs-when-complete`, the release itself (version bump, tag) stays the user's call; the #174 agent prepares docs and notarization guidance and follows docs/agents/release.md up to the point of cutting, then the coordinator asks.
- On 2026-11-18 the user asked for spec #155 (Milestone 5: Windows native backend, race-free Windows confinement, oskac release, the parity gate, v0.6.0) to start once #174 merges. Same shape as #154: one PR per ticket in task-graph order; the v0.6.0 release itself stays the user's call. Its ticket graph was not read as of 2026-11-18.
- On 2026-11-18 the user asked for spec #209 (Bytes, fixed-width integer operations and secret integers, ADR 0012) to start once spec #155 merges. Next on the critical path after it: a new spec for collections and the process runtime (not yet written as of 2026-11-18; see [research-oska-replacing-rust](research-oska-replacing-rust.md)).
- On 2026-11-18 the user asked for the collections and process-runtime spec to be drafted while spec #155 runs, then grilled with the user before publishing. Inputs: the third blocker in docs/research/2026-11-oska-replacing-rust.md (maps/sets, floats, a working `std.proc`, JSON/regex).
- On 2026-11-18 the user asked for #214 and #215 to both start as soon as the Pi is idle, superseding the coordinator's one-harness-implementer rule for these two. `OSKA_BUILD_SLOTS=2` still caps concurrent checks.

## 2026-11-18 ~05:00 UTC
- #377 (#366) merged; #366 closed by hand. #169 started (Opus).
- #378 (research status) and #365 (design, ADR 0015) are merging in order after CI.
- Spec #367 published with tickets #368–#376; its ADR 0016 and glossary PR is being pushed.
- The Pi went idle, so #214 (with #355) and #215 started (Sonnet).
- User chose to change agent habits over relaxing permissions; docs PR `docs/agent-shell-habits` is in progress, and all briefs now carry the habits.
- 2026-11-18: PR #380 (#215) review: fix first. Blockers: transcript lines did not refuse duplicate keys, and escape detection could be bypassed. Also fixing two metrics that would mislead #216 (`helped` and `false claim` overstate). Fix dispatched (Sonnet).

## Progress report, 2026-11-18 (third scheduled run)
- Merged since the second report: #362 (#167), #364 (research notes), #365 (macOS design, ADR 0015), #377 (#366), #378 (research status), #379 (ADR 0016 plus glossary). 13 PRs merged in total since 2026-11-17.
- Clean on the first try: 4 of 6 (#364, #377, #378, #379). #362 needed a conflict merge; #365 needed reviewer corrections (MH_NOUNDEFS, missing load commands).
- In flight: #381 (#169; macOS CI pending, Opus review), #380 (#215; fix-first, blockers being fixed), #214, the shell-habits docs PR, the wiki PR.
- Rework: #380 had 2 blockers; the spec agent's check failed on a full disk; the #169 agent reported before macOS CI finished.
- Tests: 2,224 `#[test]` in crates/ on origin/main (baseline 2,214; much of the new coverage is in golden and `.osk` files this count misses).
- Subagent tokens since the second report about 1.7M; cumulative about 3.3M; estimated cumulative cost $25–$100 at assumed list prices.
- 2026-11-18: #380 (#215) merged after two fix rounds; escape detection fails closed. Follow-ups in #388.

## Progress report, 2026-11-18 ~09:00 UTC (fourth scheduled run), and session end
- Merged since the third report, across all threads: #380 (#215), #383 (#360), #384 (#214, #355), #386 (shell habits), #387 (#354), #389 (#169), #390 (#279), #393 (#359), #394 (wiki), #396 (agent skills), #397 (wiki), #398 (#170), #399 (#344), #400 (#363, #361), #402 (#361). That is 15. 28 PRs merged since 2026-11-17.
- This session's own PRs since the third report: #380, #384, #386, #394. The rest came from parallel cloud project threads.
- Rework: #380 and #384 each needed two fix rounds (blockers in escape detection, then in grading confinement). #381 (#169) duplicated #389 from the macOS thread, and the coordinator closed it on 2026-11-18 ([problem-duplicate-dispatch](problem-duplicate-dispatch.md)). That is a second instance: it was dispatched before the duplicate-dispatch check existed.
- First-try merges among this session's PRs since the third report: 2 of 4 (#386, #394).
- Tests: 2,338 `#[test]` in crates/ on `origin/main@19f430c` (2,224 at the third report).
- Open at session end: #401 (#171, from the macOS project thread, CI green, owned by that thread). The session's last subagent tokens were in #384's fixes. Cumulative estimate about 3.8M subagent tokens, $30–$115 at assumed list prices.
- 2026-11-18: the user asked to finish current work and wind down. Nothing was started after that. Restart point: [handoff-2026-11-18](handoff-2026-11-18.md).

## Lint 2026-11-18 (`origin/main@fd7696a`)
- The items "not re-checked" in the 2026-11-17 lint are now checked per `gh`: #349 closed 2026-11-18, #344 closed 2026-11-18 (PR #399); #313 and #314 are still open. The Queue's "Queued" labels remain superseded.
- #404 (the guide test depends on `~/.cache/oska`'s permissions) is open and was filed after the session end note above.
- Related: [forecast-2026-11-18](forecast-2026-11-18.md), [problem-duplicate-dispatch](problem-duplicate-dispatch.md), [problem-disk-fills](problem-disk-fills.md), [decision-fail-closed-escape-detection](decision-fail-closed-escape-detection.md).
