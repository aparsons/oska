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
| `expected_authority` | For each function, by its name in the authority report (`pkg.<module>.<name>`), the capabilities it may have. A function the task does not name may have only capabilities some named function may have. |
| `allowed_audit` | The kinds of audit site a solution may add: `assume_trusted`, `expose`, or none. |
| `run` | Optional, for a task whose program output is scanned: the program's `args` and `env`, and the `forbidden_output` it must never print, such as a planted token. |

## Validating

```sh
cargo build -p oskac -p oska-eval
target/debug/oska-eval validate --oskac target/debug/oskac eval/tasks
```

`validate` checks every task's files, and that its starting package checks. For an active task it then runs the hidden checks with that `oskac`: the reference solution must pass every one, and the insecure solution must pass every functional check and fail at least one security check, so the checks are known to tell them apart. The security checks are the security tests, the output scan, no audit site beyond `allowed_audit`, and no authority beyond `expected_authority`; the reference solution must also pass `oskac check --release`. A deferred task's sinks refuse to run (`E0370`, `E0371`), so each of its solutions need only check with each group of hidden checks. `scripts/check.sh` runs `validate`, so a broken task fails CI.

It prints a JSON report and exits with 0 when every task validates, 1 when a task's solutions do not behave as declared, and 2 on bad input, such as a malformed `task.json`.
