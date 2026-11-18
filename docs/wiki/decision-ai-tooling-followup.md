# Decision: follow-up to the AI-driven development research

On 2026-11-18 the user grilled `docs/research/2026-11-ai-driven-development.md` (written 2026-11-12 at v0.3.0). A status check at `aparsons/oska@2222739` found:
- done: A1, A2, A3 (core), B1 (file baseline), D4, G3;
- specced: D1 (#207), C2 (#275), C4 (#206), D2 (#294), G2 (#12);
- open questions already settled: 2 (`--placeholders` opt-in), 3 (committed baseline file), 6 (#206 skill/plugin shape).

The user chose to (a) settle the remaining open questions and write one spec for the unspecced proposals worth doing, scheduled behind the macOS chain, and (c) add a dated status section to the research note.

Related: [goal-drain-backlog](goal-drain-backlog.md).

## Decisions, 2026-11-18 (grilling rounds 2–4)
- Q2: unused authority is an error only under the release check; no warning tier. Severity stays `error`/`note`.
- Q3: one new spec with A5, F3, E4, F2 and G1. C1 (`oskac mcp`) and E1 (property tests) become separate future specs; B2/B3 wait for #111.
- Q4: order is the macOS chain first, then the eval harness (#214–#217) in idle Pi time, then the new spec.
- Q5: an ADR records "unused authority is a release-check error; Oska has no warning tier" (0015 or 0016, depending on merge order with PR #365).
- Q6: A5 covers capability parameters (`machine` fix on `main`, `maybe` elsewhere) and unread `cap struct` fields; locals, imports and the read-only hint are later follow-ups.
- Q7: F3 adds `frames` to panic JSON on both the interpreter and native, held equal by the differential suite.
- Q8: snapshots are one file per assertion under `snapshots/<test-file>/<test-name>.snap`, updated with `oskac test --update`.
- Q9: `oskac new` writes `oska.toml`, `src/main.osk` (`src/lib.osk` with `--lib`) and the agent-setup `AGENTS.md`/`CLAUDE.md`, with a flag to opt out.
- Q10: A5, F2 and E4 are measured with and without, using the eval harness; F3 and G1 ship unmeasured.
- Q11: methods whose signature a trait fixes are exempt from A5; there is no `_name` escape hatch.
- Q12: the E0102 fix is `machine` when the hoisted expression calls only pure functions, otherwise `maybe`.
- GLOSSARY.md gained "Unused authority" and "Release check" (uncommitted on 2026-11-18; to ship with the spec).

Lint note (2026-11-18, `origin/main@fd7696a`): the GLOSSARY.md entries "Unused authority" and "Release check" are committed (PR #379, with ADR 0016), so "uncommitted" above is superseded; ADR 0016 is the number chosen for Q5. Spec #367 has tickets #368 to #376, scheduled after the eval harness per [handoff-2026-11-18](handoff-2026-11-18.md).
