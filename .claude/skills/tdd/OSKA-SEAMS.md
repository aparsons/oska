# Oska seams, helpers, and commands

Read this file when you write tests in aparsons/oska. `CODING_STANDARDS.md` (Tests) holds the rules. This file only maps each kind of change to its seam and its helpers. If the two disagree, `CODING_STANDARDS.md` wins.

## Pick the seam

| Change | Seam | Helpers | Where the test goes |
|---|---|---|---|
| A checker rule, a diagnostic, a fix | `oska-check` test helpers | `assert_ok`, `assert_errors(src, &[(code, span text)])`, `assert_fix`, `assert_authority`, `assert_modules_errors` in `crates/oska-check/tests/common/mod.rs` | `crates/oska-check/tests/<topic>.rs` |
| Runtime behaviour, a prelude addition, a capability | `oska-run` API on `FakeHost` | `assert_runs`, `assert_refused`, `assert_panics`, `assert_exits`, `run_on(FakeHost::new().with_file(..), ..)`, `test_fake` in `crates/oska-run/tests/common/mod.rs` | `crates/oska-run/tests/<topic>.rs` |
| Confinement against the real OS (symlinks, hard links) | `oska-run` on `RealHost` with a real temporary directory | the `Sandbox` pattern in `crates/oska-run/tests/capabilities.rs` | `crates/oska-run/tests/<topic>.rs` |
| Native code generation | interpreter and native binary compared (the differential harness) | `assert_same(&Case)` in `crates/oskac/tests/differential/mod.rs` | `crates/oskac/tests/native_<topic>.rs` |
| CLI output (`oskac check`, `run`, `test`, `build`, `explain`) | the `oskac` binary | golden files under `crates/oskac/tests/golden/` | `crates/oskac/tests/<topic>_golden.rs` |
| The site generator `site/build.osk` | `oska-run` with `FakeHost` and fixture docs | fixtures under `site/tests/<case>/`; `crates/oska-run/tests/common/site.rs` | `crates/oska-run/tests/site_<topic>.rs` |
| An eval task | the eval harness | `checks/functional.osk`, `checks/security.osk` | `eval/tasks/<id>/`; see `eval/README.md` |

Use the highest seam in the table that reaches the behaviour. A language addition needs both a checker test and a runtime test. Follow the "Grow Oska" rule in `CODING_STANDARDS.md`: prelude, `Host`, `RealHost`, `FakeHost`, intrinsics, and the coverage test.

## Rules that are easy to miss

- **One new test file for each piece of work.** Start it with `mod common;` and a `//!` line that names the issue, for example `//! Trust on text (#50): ...`.
- **Paths, JSON paths, and homes.** Use `crates/oskac/tests/common/portable.rs`: `Scratch`, `json_str`, `json_path`, `set_home`, `literal_path`. Then the test passes on Windows the first time.
- **Golden files.** Refresh a golden file only for an intended change: `UPDATE_GOLDEN=1 cargo test -p oskac --test <name>_golden`. Read the golden diff before you commit it. Show it in the pull request.
- **`FakeHost` is deterministic.** Its clock is fixed at the Unix epoch, and its generator has a fixed seed. Give it files, directories, symlinks, stdin, and environment with its `with_*` builder methods.
- **No `#[ignore]`.** A test that needs a missing tool says so and returns early.
- **Attack tests.** A security fix gets a test that feeds the bad input, such as a `..` path, a symlink, untrusted text into a trusted slot, or a tampered file. It is red without the fix. In an eval task, name each attack test `attack: <what it tries>`.

## Run commands

Export the lean cargo environment from `docs/agents/implementer.md` first.

- One test file: `cargo test -p <crate> --test <file>`.
- One test: `cargo test -p <crate> --test <file> <test name>`.
- An Oska program's own `test` blocks: `cargo run -p oskac -- test <path>`.
- Before a push: `scripts/check.sh --fast`. At the end: the full `scripts/check.sh`, once.
