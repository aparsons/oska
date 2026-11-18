# Research: when Oska can replace Rust

On 2026-11-17 a research agent estimated this from `aparsons/oska@fc411f9`. Full findings: `docs/research/2026-11-oska-replacing-rust.md`.

Headline (2026-11-17):
- CLIs and dev tooling in Oska: 2026-12-10 to 2027-01-31, medium-low confidence.
- Networked services: 2027-02 to 2027-05, low confidence.
- Self-hosting `oskac`: 2027-03 to 2027-07, low confidence; unscheduled, deferred by ADR 0011.
- Critical path: parity gate (#154 macOS, #155 Windows), then #209 (Bytes), then a new spec for collections plus the process runtime.

The backlog queue in [goal-drain-backlog](goal-drain-backlog.md) contains none of these.

Lint note (2026-11-17): the line "The backlog queue ... contains none of these" is superseded by "Reprioritised 2026-11-17" in [goal-drain-backlog](goal-drain-backlog.md), which puts #154, #155 and #209 ahead of site/release specs. Estimates are dated 2026-11-17 at `fc411f9`; `origin/main` was `bb8b9ac` at lint time.

Lint note (2026-11-18): progress on the critical path is in [handoff-2026-11-18](handoff-2026-11-18.md) and [forecast-2026-11-18](forecast-2026-11-18.md). The estimates above are not revised.
