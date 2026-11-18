# Feedback loop options

Try these options in about this order. Take the first one that reaches the bug. A higher option is usually faster and more repeatable.

In aparsons/oska, export the lean cargo environment from `docs/agents/implementer.md` before each cargo command. Put loop logs outside the worktree, for example in a directory from `mktemp -d`.

## Oska loops

1. **Failing test at a seam.** Write one test at the seam that `OSKA-SEAMS.md` in the `tdd` skill names for the area. Run only it: `cargo test -p <crate> --test <file> <test name>`. A checker bug uses `assert_errors` in `oska-check`. A runtime bug uses `run_on(FakeHost::new()...)` in `oska-run`.
2. **A fixture `.osk` file.** Write the smallest program that shows the bug, in a temporary directory. Run `cargo run -q -p oskac -- check <file>.osk` or `cargo run -q -p oskac -- run <file>.osk`. Assert on the exact diagnostic code or the exact output with `grep -q` or `diff`.
3. **Golden diff.** For a CLI output bug, run the golden test of that command, for example `cargo test -p oskac --test check_golden`. Read the diff that it prints. Do not refresh the golden file to make the loop pass.
4. **Interpreter against native.** For a code generation bug, add a case to the differential harness in `crates/oskac/tests/differential/`. It runs the program on both backends and compares the outcomes.
5. **Bisect.** For a regression, write a script for `git bisect run`. It runs loop 1 or 2, exits 0 on a good commit, 1 on a bad commit, and 125 when the commit does not build. Run it in a separate worktree, so your branch stays as it is.
6. **Windows or macOS on CI.** For a bug that shows only on one platform, push the branch and dispatch one test: `gh workflow run windows.yml --ref <branch> -f crate=<crate> -f test=<test target>`. The section "Windows on demand" of `docs/agents/implementer.md` lists the inputs. Each dispatch is slow, so make the local loop sharp first.
7. **Human in the loop.** Use `scripts/hitl-loop.sh` when a step needs a person, such as a check on a real macOS machine. Use it last.

## Generic loops (other repos)

Use the same order. Replace loops 1 to 4 with: a failing test at a seam, a CLI run on a fixture compared with known-good output, an HTTP script with `curl`, a headless browser script, a replay of a captured request, or a small harness that calls only the failing part. A property or fuzz loop and a differential run of the old and new version also work.

## Make the loop better

- **Faster.** Cut setup. Run one test, not the suite. Use a small fixture. Do not run `scripts/check.sh` as a loop.
- **Sharper.** Assert the exact symptom: the wrong value, the diagnostic code, the exit code, or the time limit.
- **Deterministic.** Use `FakeHost`: its clock and its random seed are fixed. Use a fresh temporary directory, through `Scratch` in `crates/oskac/tests/common/portable.rs`. Block or stub the network.

## Flaky bugs

A flaky bug fails only on some runs. Raise the failure rate before you debug it:

- run the loop many times in a row and count failures, with a maximum of 200 runs;
- add load, such as parallel test threads;
- narrow the timing window with a fixed delay at a suspect point.

On the shared machine, add load only with your own processes. Continue when the loop fails on most runs.
