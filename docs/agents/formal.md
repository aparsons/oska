# Formal models

Oska Core's Lean model (ADR 0018) lives in `formal/lean/`, and the TLA+ models of concurrency (ADR 0019) live in `formal/tla/`. The `formal` step of `scripts/check.sh` checks both, and it blocks merge like every other step.

## Keep the Lean model in step

- A change to how a construct of Oska Core is typed or run (the checker or the interpreter) updates the Lean model in the same pull request. Do not defer it to a follow-up issue.
- When the change touches Oska Core and the Lean model does not need to change, say why in the pull request's Summary.
- No `sorry` and no `admit`, anywhere in a `.lean` file, comments included. The step fails on the word before it looks for Lean, and the build runs with warnings as errors. An unfinished proof may not merge, even with an issue (ADR 0018).
- Every new Lean module is imported, directly or not, by `formal/lean/OskaFormal.lean`, so `lake build` checks it.
- Use the glossary's terms in Lean names and comments: Oska Core, core IR, authority safety.

## TLA+ models

- Each model is a `formal/tla/NAME.tla` with a `formal/tla/NAME.cfg` beside it. A `.tla` with no `.cfg` fails the step.
- A design for parallel tasks, `Sched` channels or `select` is modelled here, and TLC passes it, before an implementation ticket opens (ADR 0019).
- Keep each model's state space small enough that TLC finishes in a minute or two in CI.

## Running the step

- Run it alone: `scripts/formal/run.sh`. It is also the `formal` step of `scripts/check.sh`, and in the fast tier when `formal/` or `scripts/formal/` changes.
- Install the pinned tools on Linux (x86_64 or aarch64): `scripts/formal/install.sh`. It puts Lean and `tla2tools.jar` in `~/.cache/oska/formal`, where `run.sh` finds them. TLC also needs Java 11 or newer on `PATH`. With elan installed instead, `lake` picks the Lean that `formal/lean/lean-toolchain` names.
- Without Lean or TLC the step skips and says what is missing. In CI (`CI` or `GITHUB_ACTIONS` set) it fails instead: the `ci.yml` test job installs the pinned tools.
- The step's own tests, `scripts/formal/test.sh`, need neither tool.

## Moving a pin

The versions and checksums are in `scripts/formal/pins.sh`. Moving Lean also changes `formal/lean/lean-toolchain`; `scripts/formal/test.sh` fails if the two differ. Take each SHA-256 from a published source, such as the digest on the GitHub release page, never from a file you downloaded yourself.
