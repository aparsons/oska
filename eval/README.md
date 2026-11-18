# Evaluation tasks

The task set of the evaluation harness (`crates/oska-eval`): Oska tasks that an AI agent is given, one at a time, to measure how often it writes code that works and is secure. Only the maintainer runs agents on them, locally; nothing here calls a model.

## A task

Each directory in `tasks/` is one task, named by its id:

| File | What it is |
| --- | --- |
| `task.json` | The task's metadata, below. |
| `prompt.md` | What the agent is asked, given to it verbatim. It describes the function only and never mentions security: `validate` refuses words such as `secure`, `attack`, `inject`, `secret` and `untrusted`. |
| `start/` | The starting package the agent works in: `oska.toml` and `src/`, with the functions to write stubbed out with `todo()`. |
| `checks/functional.osk` | The hidden functional checks: an Oska test module that imports the task's modules with `use pkg.…`. |
| `checks/security.osk` | The hidden security checks, in the same form. Name each attack test `attack: …`. A task whose security check is its output scan or its expected authority alone keeps a module with only a comment saying so. |
| `solutions/reference/` | A correct, secure solution: files under `src/` that replace or add modules of the starting package. |
| `solutions/insecure/` | A solution that works but is insecure, in the same form. |

The agent never sees `checks/` or `solutions/`. After a run, each group of checks is added to the package as `src/eval_checks.osk`, a module name no task file may use.

`task.json` holds, and may hold only:

| Field | Meaning |
| --- | --- |
| `schema` | The format version, `1`. |
| `id` | The directory's name. |
| `title` | A short title. |
| `class` | `path_traversal`, `xss`, `secret_logging`, `least_authority`, `sql_injection`, `command_injection` or `ssrf`. |
| `status` | `active`, or `deferred` for a task whose sinks do not run yet. |
| `reason` | Why a deferred task is deferred. Only a deferred task has one. |
| `attempts`, `turn_limit`, `timeout_seconds` | How many runs a batch makes, and each run's limits. |
| `read_only` | Workspace files the agent must not change, such as `oska.toml`. |
| `expected_authority` | For each function, by its name in the authority report (`pkg.<module>.<name>`), the capabilities it may have; a function with none must stay pure. The harness writes this as an authority baseline and `oskac check --baseline` says what widened, so a function that gains a capability no listed one covers, becomes impure, or is new and reaches a kind no listed function does, fails. A kind that is narrower than an expected one is not widening. |
| `allowed_audit` | The kinds of audit site a solution may add: `assume_trusted`, `expose`, or none. |
| `run` | Optional, for a task whose program output is scanned: the program's `args` and `env`, and the `forbidden_output` it must never print, such as a planted token. |

## Validating

```sh
cargo build -p oskac -p oska-eval
target/debug/oska-eval validate --oskac target/debug/oskac eval/tasks
```

`validate` checks every task's files, and that its starting package checks. For an active task it then runs the hidden checks with that `oskac`: the reference solution must pass every one, and the insecure solution must pass every functional check and fail at least one security check, so the checks are known to tell them apart. The security checks are the security tests, the output scan, no audit site beyond `allowed_audit`, and no authority beyond `expected_authority`; the reference solution must also pass `oskac check --release`. A deferred task's sinks refuse to run (`E0370`, `E0371`), so each of its solutions need only check with each group of hidden checks. `scripts/check.sh` runs `validate`, so a broken task fails CI.

It prints a JSON report and exits with 0 when every task validates, 1 when a task's solutions do not behave as declared, and 2 on bad input, such as a malformed `task.json`.

## Preparing and grading a run

```sh
target/debug/oska-eval prepare eval/tasks <task-id> <run-dir>
target/debug/oska-eval grade --oskac target/debug/oskac eval/tasks <task-id> <run-dir>
```

`prepare` makes `<run-dir>`, which must be new (or empty) and outside the repo and the task set, so the agent cannot read the hidden checks or the solutions. It holds `workspace/` (a copy of `start/`, where the agent works), `prompt.md` (the prompt, verbatim) and `snapshot.json` (a digest of every starting file, which the agent has no reason to open). It refuses a deferred task. It prints the paths of the workspace and the prompt.

`grade` copies `workspace/` and works on the copy, so grading changes nothing and can be repeated. It adds each group of hidden checks as `src/eval_checks.osk` in turn and grades only through `oskac`: `oskac test` over the hidden checks, `oskac check --format=json` (the package must check, and its audit sites must be of `allowed_audit`), `oskac check --baseline` for authority against `expected_authority`, `oskac check --release`, and the program's output when the task has a `run`. It also compares the workspace with the snapshot: a changed or removed `read_only` file, or a module the agent named `eval_checks`, fails the `integrity` group.

Its report has `schema`, `task`, `passed` (every functional check), `secure` (every security check), `clean` (the release and integrity groups), `changed` (every file that differs from the snapshot) and `checks`, each with its `group`, `check`, `outcome` (`passed` or `failed`) and, when it failed, a `detail`. It exits with 0 when all three are true, 1 when one is not, and 2 on bad input.

## Reports

Every subcommand prints one JSON report on standard output, and nothing on standard error. Bad input has one shape for all of them: `{"schema": 1, "valid": false, "errors": [...]}`, with exit code 2, for a missing task directory, an unknown or deferred task, or a run directory that is not fresh or has no snapshot; `grade` adds an `oskac` that cannot run. `validate` reports a malformed task, or an `oskac` that cannot run, inside its `tasks` list instead (an `errors` list on that task), still with exit code 2, so the rest of the set is still reported. Errors in a JSON file say the line and column.
