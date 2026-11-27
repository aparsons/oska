# Concurrency audit of the `scripts` group

Spec #810, tickets #812 and #813. For each step of `scripts/check.sh --group scripts`, the state it shares with the other steps and a verdict: `safe` to run beside another suite, or `serial`. `scripts/check.sh --group scripts` runs the `safe` steps concurrently, up to the number of cores (`OSKA_CHECK_JOBS=N` sets the number; `1` runs one at a time). A `safe` step carries the `--concurrent` tag in `scripts/check.sh`; a `serial` step has none, runs in the group's one serial lane (never beside another serial step), and may run beside safe ones. Each suite's output is buffered in a private temp dir and prints as one block, in step order.

`scripts/check-steps/test.sh` fails when the `--concurrent` tags and the `safe` rows differ, when a step of the group has no row here, when a row names no step of the group, or when a verdict is not `safe` or `serial` with a reason. A new suite adds its row.

## How it was checked

Each `safe` verdict was checked by running the suites side by side on a 4-core machine, not only by reading them (`bash SUITE` per suite, all started at once, each with its own log and exit code):

- The 22 suites that do not build in the repo's `target` ran together (about 2 minutes, the longest being the installer tests at 116 s under load). All exited 0.
- `scripts/check-steps/test.sh`, which runs `cargo metadata` on the repo, ran beside the eval task set, the eval batch driver tests and the release script tests. It passed beside them.
- The three `serial` suites ran together on a warm `target` (all passed), and then on an empty `CARGO_TARGET_DIR`: the second `cargo build` printed `Blocking waiting for file lock on build directory` and waited for the first.

## Shared state that matters

| State | Where it is |
| --- | --- |
| temp dir | Every suite makes its own `mktemp -d` and removes it on exit. None names a fixed path in `/tmp`. |
| build slot | `scripts/check.sh` takes one slot for the whole run, before the first step that builds; no suite takes its own. The build slot tests give each case a private `XDG_RUNTIME_DIR`. |
| target lock | `cargo build` in the repo takes a lock on `target`, and the built `target/debug/oskac` and `oska-eval` are read by the suite. |
| HOME | Suites that run an installer or `git` set `HOME` to a directory in their scratch dir. |
| ~/.cache/oska | Only `scripts/actionlint/run.sh` writes it, and only when its pinned actionlint is not cached (#404). |

## Verdicts

| Step | Shared state | Verdict | Reason |
| --- | --- | --- | --- |
| `eval task set` | repo `target` lock, `target/debug/oskac` and `oska-eval` | serial | `cargo build -p oskac -p oska-eval` in the repo; beside another build of the repo it waits for the lock, and a source change by one would replace the binary the other runs. |
| `eval batch driver tests` | repo `target` lock, `target/debug`, a `target/eval-batch-test-PID` path it only probes | serial | Builds the same two crates in the repo `target` as the eval task set; its own scratch dir is private. |
| `installer tests` | none | safe | Own scratch dir, `HOME`, `TMPDIR` and `file://` site; no cargo, no network. |
| `release script tests` | repo `target` lock, `target/debug/oskac` | serial | `cargo build --locked -p oskac` in the repo to run `verify.osk`; scratch dir and `HOME` are private. |
| `release notes tests` | none | safe | Writes its own `CHANGELOG.md` in its scratch dir. |
| `release image tests` | none | safe | Own scratch dir and fake binaries; reads `tar` and `jq`. |
| `release image push tests` | none | safe | Own scratch dir and a fake `skopeo` on its own `PATH`; no network. |
| `Linux build check tests` | none | safe | Own scratch dir; runs `readelf` on files it makes. |
| `unsafe guard tests` | cargo package cache, briefly | safe | Builds throwaway workspaces in its own scratch dir (own `target`); the package cache lock is shared and brief, and nothing is downloaded. |
| `intrinsic guard tests` | none | safe | Own scratch trees; the file under `target/` is in its scratch tree. |
| `SPDX guard tests` | none | safe | Own scratch trees and own `git` repositories. |
| `kill guard tests` | none | safe | Feeds command text to the guard; kills nothing and starts no process. |
| `workflow guard tests` | none | safe | Own scratch trees. |
| `temp-leak wrapper tests` | none | safe | Sets `TMPDIR` to its own scratch dir; the wrapper's private dir is made under that, not under the shared `/tmp`. |
| `rust-stable tests` | none | safe | Own scratch dir; reads fake manifests. |
| `cross-build manifest tests` | none | safe | Own scratch trees. |
| `cargo cache tests` | cargo package cache, briefly | safe | Real `cargo build --offline` of a one-crate project in its own clones, each with its own `target`; no dependencies, so the shared lock is brief. |
| `build slot tests` | none | safe | Each case has its own `XDG_RUNTIME_DIR` and `TMPDIR` for the lock files, so it never touches the machine-wide slots. |
| `pass stamp tests` | none | safe | Own bare remote, clones and worktree; the stamp is in a scratch git directory, not the repo's. |
| `docs-test list tests` | none | safe | Reads `crates/` and `docs/`, writes only its scratch dir. |
| `test shard tests` | none | safe | Reads `crates/` and `.github/workflows/ci.yml`; writes nothing. |
| `ci changes tests` | none | safe | Writes no files; runs the docs-only script on made-up change lists. |
| `check step registry tests` | cargo package cache, briefly | safe | Runs the real `check.sh` in a scratch tree with stubs; one drift guard runs `cargo metadata` on the repo, which reads the manifests and takes no build lock. |
| `workflow validation tests` | `~/.cache/oska` on a cold cache | safe | Reads the cached actionlint. On a cold cache it downloads into its own temp dir and moves the binary into place, and a reader that sees no checksum file yet downloads its own copy, so two runs only repeat work. The `workflow validation` step in the `lint` group uses the same cache. |
| `x509 roots generator tests` | none | safe | Own scratch dir; runs `python3 -I` on a fixture. |
| `expected workflows tests` | none | safe | Own scratch dir; runs `python3 -I` on fixtures and reads `.github/workflows/`. |
