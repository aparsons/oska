# Formal models

Oska Core's Lean model (ADR 0018) lives in `formal/lean/`, and the TLA+ models of concurrency (ADR 0019) live in `formal/tla/`. The `formal` step of `scripts/check.sh` checks both, and it blocks merge like every other step.

## Keep the Lean model in step

Oska Core's model is in `formal/lean/OskaFormal/Core/`. `Syntax.lean` covers exactly the core IR version that `crates/oska-check/src/core_ir.rs` exports. `Typing.lean` holds the typing rules, `Semantics.lean` the small-step semantics with the interpreter's panics, and `Soundness.lean` the proofs of progress and preservation, and that a mutating method changes only its receiver (`method_writes_only_receiver`). `Host.lean` models the interpreter's `FakeHost` and its confinement, and proves its answers to effects are ones the semantics allows (`perform_allows`). `Eval.lean` is the executable semantics: `step`, proved to take exactly the steps of `Semantics.lean` (`step_sound`, `step_complete`), and `run`, which repeats it within a fuel on the fake host. `Authority.lean` proves authority safety and that attenuation never widens: `authority_safety` bounds a call by `callAuthority`, its function's authority and every kind below the capabilities its `cap fn` arguments hold; `authority_safety_ofArgs` computes that bound from the arguments, and `authority_safety_noCapFn` drops it when no parameter is a `cap fn`. `Examples.lean` holds worked typing examples, among them ones the typing rejects. `CoreIR.lean` reads core IR. A new core IR version extends each of them that it touches.

The Lean reference is the `oska_core` executable (`formal/lean/OskaReference.lean`): it reads core IR, runs `main` with the capabilities its signature declares on a fake host that starts empty, and prints what the program wrote (its stdout, then the files and directories it left) and how `main` ends (`result ()` and `exit 0`, or `panic CODE` and `exit 101`), and refuses input that is not core IR with exit 2. `cargo test -p oska-run --test lean_reference` builds it with `lake build oska_core`, runs every runtime test program that the export accepts on it and on the interpreter's `FakeHost`, fails on any difference in stdout, files, panic or exit code, and prints how many programs it covered and how many the export refused. Without Lean it skips, except where `OSKA_LEAN_REFERENCE=required`, which `scripts/check.sh` sets in CI. To cover a construct, add a runtime test whose program is in Oska Core as a whole string literal (`crates/oska-run/tests/oska_core.rs`); a program built with `format!` is not collected. The test fails if it covers fewer programs than its `MIN_COVERED`; raise that as Oska Core grows. It bounds `lake build` and each run of the reference with a wall-clock limit (`tests/common/process.rs`), and a run past it is a difference.

Two known gaps, kept on purpose. `confinement.yml` runs `cargo test -p oska-run` on Linux without Lean, so there the test skips; `ci.yml`'s test job, which runs `scripts/check.sh` with Lean installed, is the gate, and changing the other workflow needs a human merge. Locally the pass stamp covers a run in which the test skipped for want of Lean, as it covers a skipped shellcheck step; CI runs it.

- A change to how a construct of Oska Core is typed or run (the checker or the interpreter) updates the Lean model in the same pull request. Do not defer it to a follow-up issue.
- When the change touches Oska Core and the Lean model does not need to change, say why in the pull request's Summary.
- No `sorry`, `admit`, `axiom` or `native_decide` anywhere in a `.lean` file. Comments count too: the step matches each as a whole word in the file's text, before it looks for Lean, so `admittedly` passes and `-- remove this sorry` fails. The build runs with warnings as errors. An unfinished proof may not merge, even with an issue (ADR 0018).
- Every new Lean module is imported, directly or not, by `formal/lean/OskaFormal.lean`, so `lake build` checks it.
- Use the glossary's terms in Lean names and comments: Oska Core, core IR, authority safety.

## TLA+ models

- Each model is a `formal/tla/NAME.tla` with a `formal/tla/NAME.cfg` beside it. A `.tla` with no `.cfg` fails the step.
- A design for parallel tasks, `Sched` channels or `select` is modelled here, and TLC passes it, before an implementation ticket opens (ADR 0019).
- Keep each model's state space small enough that TLC finishes in a minute or two in CI.

## Running the step

- Run it alone: `scripts/formal/run.sh`. It is also the `formal` step of `scripts/check.sh`, and in the fast tier when `formal/` or `scripts/formal/` changes.
- Install the pinned tools on Linux (x86_64 or aarch64): `scripts/formal/install.sh`. It puts Lean and `tla2tools.jar` in `$XDG_CACHE_HOME/oska/formal` (`~/.cache/oska/formal` when `XDG_CACHE_HOME` is unset), where `run.sh` finds them. TLC also needs Java 11 or newer on `PATH`. With elan installed instead, `lake` picks the Lean that `formal/lean/lean-toolchain` names.
- Without Lean or TLC the step skips and says what is missing. In CI (`CI` or `GITHUB_ACTIONS` set) it fails instead: the `ci.yml` test job installs the pinned tools.
- The step's own tests, `scripts/formal/test.sh`, need neither tool.

## Moving a pin

The versions and checksums are in `scripts/formal/pins.sh`. Moving Lean also changes `formal/lean/lean-toolchain`; `scripts/formal/test.sh` fails if the two differ. Take each checksum from a published source, never from a file you downloaded yourself. Lean's SHA-256s are the digests GitHub shows beside each asset on the release page. The TLC release publishes only a SHA-1 for `tla2tools.jar`, so its SHA-256 is cross-checked from the one nixpkgs pins for the same URL; `install.sh` verifies both.
