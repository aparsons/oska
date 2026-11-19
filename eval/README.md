# Evaluation tasks

The task set of the evaluation harness (`crates/oska-eval`): Oska tasks that an AI agent is given, one at a time, to measure how often it writes code that works and is secure. Only the maintainer runs agents on them, locally, with the driver in `scripts/eval-batch/` (see [Running a batch](#running-a-batch)); nothing else here calls a model.

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
target/debug/oska-eval prepare eval/tasks <task-id> <run-dir> [--setup site/agent-setup [--hook]]
target/debug/oska-eval grade --oskac target/debug/oskac [--setup site/agent-setup] eval/tasks <task-id> <run-dir>
```

`prepare` makes `<run-dir>`, which must be new (or empty) and outside the repo and the task set, so the agent cannot read the hidden checks or the solutions. It holds `workspace/` (a copy of `start/`, where the agent works), and `prompt.md` (the prompt, verbatim). Nothing the grade relies on is kept there: the agent can write anywhere in the run directory. It refuses a deferred task. It prints the paths of the workspace and the prompt.

With `--setup`, `prepare` also gives the agent what a real user's agent gets from the agent-setup prompt: `AGENTS.md` and `CLAUDE.md` in the workspace, written from the published templates in `site/agent-setup/`, and `claude-settings.json` in the run directory, the Claude Code settings for the variant: the published check-on-edit hook (`claude-settings-post-edit.json`) with `--hook`, else none (`{}`). `grade --setup` then counts `AGENTS.md` and `CLAUDE.md` as part of the starting package and as read-only: unchanged, they are not in `changed`; changed, they fail the `integrity` group, and `record` sets `read_only_changed`.

`grade` treats the agent as untrusted. It reads `workspace/` once, into a copy, and judges only the copy, so grading changes nothing, can be repeated, and an agent still writing cannot make the checks see other files than the grade. The copy holds only regular files: a link (or a pipe or device) is never followed, so it cannot pull in a file from outside or loop; it is left out and fails the `integrity` group. Each `read_only` file is compared with the task's own `start/`, and the copy is graded with the task's version of it, so tampering is reported but cannot sway `passed` or `secure`. A file or directory in `src/` the agent named `eval_checks.osk` or `eval_checks`, in any case, is left out of the copy and also fails the `integrity` group. It then adds each group of hidden checks as `src/eval_checks.osk` in turn and grades only through `oskac`: `oskac test` over the hidden checks, `oskac check --format=json` (the package must check, and its audit sites must be of `allowed_audit`), `oskac check --baseline` for authority against `expected_authority`, `oskac check --release`, and the program's output when the task has a `run`.

Its report has `schema`, `task`, `passed` (every functional check), `secure` (every security check), `clean` (the release and integrity groups), `changed` (every file of the copy that differs from `start/`, is not in it, or is missing) and `checks`, each with its `group`, `check`, `outcome` (`passed` or `failed`) and, when it failed, a `detail`. It exits with 0 when all three are true, 1 when one is not, and 2 on bad input.

## Running a batch

A **batch** is every run of a set of tasks, in one or more variants, by one model, and it costs money: each run is a paid session of an AI agent. Only the maintainer runs one, by hand, on their own machine, with their own agent CLI login. No workflow runs a batch, CI holds no credential for one, and the driver refuses to run live when `CI` or `GITHUB_ACTIONS` is set.

### The batch config

A batch starts from a batch config: a JSON file named by the batch's id, such as `2026-11-20-first.json` (lower-case letters, digits, `-` and `_`), kept wherever the maintainer likes. The driver copies it into the batch's results as `batch.json`, and the summary carries it as `config`.

```json
{
  "schema": 1,
  "model": "<model-id>",
  "tasks": [
    "least-authority-line-count", "least-authority-tally",
    "path-traversal-public-files", "path-traversal-user-uploads",
    "secret-logging-access-check", "secret-logging-webhook-delivery",
    "xss-greeting", "xss-profile-link"
  ],
  "variants": [{"name": "base", "web": true, "hook": false, "fix": true}],
  "budget_usd": 40,
  "estimate_usd_per_run": 0.5,
  "cap_usd_per_run": 1,
  "estimate_tokens_per_run": 300000
}
```

| Field | Meaning |
| --- | --- |
| `schema` | The format version, `1`. |
| `model` | The model the agent CLI runs (`claude --model`), as the CLI names it: text with no space. Replace the placeholder. |
| `tasks` | The ids of the active tasks to run, each once. Each runs its `task.json`'s `attempts`, with its `turn_limit` and `timeout_seconds`. |
| `variants` | Each variant's `name` (lower-case letters, digits, `-` and `_`) and three switches, each `true` or `false`: `web` (the agent may fetch the Oska site's guide), `hook` (the published check-on-edit hook runs `oskac check` after each edit) and `fix` (the agent may run `oskac fix`). Every task runs in every variant, so a batch with `hook` on in one variant and off in another measures the hook. |
| `budget_usd` | The most the batch may cost, in US dollars, as the agent CLI reports cost: no run starts once the batch's recorded runs reach it. |
| `estimate_usd_per_run` | What one run is expected to cost, in US dollars, for the plan's estimate. |
| `cap_usd_per_run` | The most one run may cost, in US dollars, at least the estimate: the agent CLI stops a run there (`--max-budget-usd`), and a run whose cost is unknown (it failed, or its usage could not be read) counts as costing this much against the budget. |
| `estimate_tokens_per_run` | How many tokens, input and output, one run is expected to use, for the plan's estimate. |

### The plan

```sh
target/release/oska-eval plan eval/tasks 2026-11-20-first.json
```

`plan` reads the config against the task set and prints every run the batch will make (task, variant, attempt, the task's turn limit and timeout, and the run record's file name) and an `estimate`: the `runs`, the most turns and seconds of agent time they may take (`turns_at_most`, `seconds_at_most`), the estimated `tokens` and `cost_usd` (runs times each estimate per run), the `cost_usd_at_most` (runs times the cap), the `budget_usd`, and whether the estimate is `within_budget`. It exits with 1 when the estimate is over the budget, and 2 on a bad config: an unknown field, an unknown or deferred task, a variant named twice or missing a switch, a cap below the estimate, or a file name that is not a batch id.

### The driver

```sh
cargo build --release -p oskac -p oska-eval
scripts/eval-batch/run.sh 2026-11-20-first.json          # the plan; nothing runs
scripts/eval-batch/run.sh 2026-11-20-first.json --live   # runs the batch
```

`scripts/eval-batch/run.sh` needs `claude` (logged in), `jq`, `git`, `timeout` (`gtimeout` on macOS, from coreutils) and `sha256sum` or `shasum`, and the `oska-eval` and `oskac` that `--bin` names (`target/release` by default). Without `--live` it prints the plan and stops. With `--live` it prints the plan, refuses a plan over its budget, and asks for the batch id to be typed before it starts (`--yes` skips the question; the budget still holds). It also refuses a work directory inside the repo (links resolved) or whose path holds anything but letters, digits, `.`, `_`, `-` and `/`. Then, for each run of the plan in order, it:

1. skips the run if its record is already in `eval/results/<batch>/runs/`, so a batch that stopped resumes where it stopped (only with the same config: a changed config needs a new batch id);
2. once the batch's cost reaches `budget_usd`, records each run not yet made with the stop reason `budget`, and runs nothing more. The cost is what each record reports, or `cap_usd_per_run` for a run whose cost is unknown, never zero;
3. `prepare`s a fresh run directory under `--work` (default `~/.local/state/oska-eval/<batch>/`; a run directory left by an interrupted run is moved aside, kept, and the run made again), with `--setup site/agent-setup`, and `--hook` in a variant with `hook`;
4. runs `claude -p` in the workspace, with the prompt on its standard input, its `stream-json` output saved as `transcript.jsonl` beside the workspace, and the task's timeout (a run cut off is recorded as `timeout`). The flags pin what the agent gets:
   - `--model` (the config's), `--max-turns` (the task's turn limit) and `--max-budget-usd` (the cap);
   - `--restricted`: the maintainer's own user, project and local settings files are ignored, the file tools are confined to the workspace, and permission bypass is refused; `--settings` gives the variant's `claude-settings.json` (the hook, or nothing); `--strict-mcp-config` loads no MCP server; `--no-session-persistence` keeps nothing;
   - `--permission-mode dontAsk` and `--permission-prompts none`: anything not allowed below is denied, never asked about;
   - `--tools` `Read,Edit,Write,Glob,Grep,Bash`, and `WebFetch` in a variant with `web`;
   - `--allowedTools`: `Read(//<workspace>/**)`, `Edit(//<workspace>/**)`, `Glob`, `Grep`, and in `Bash` only `oskac check`, `oskac test` and `oskac explain` (with the `oskac` under test first on `PATH`), plus `oskac fix` in a variant with `fix` and `WebFetch(domain:oskalang.dev)` in a variant with `web`;
   - `--disallowedTools`: `oskac run`, `oskac build` and `WebSearch`, and `oskac fix` and `WebFetch` in the variants without them. `oskac run` and `oskac build` are left out although `AGENTS.md` mentions them: the agent's program would run with the maintainer's own authority, outside any sandbox, and `oskac test` exercises it as well;
5. `grade`s the workspace (with `--setup`), `analyse`s the transcript and `record`s the run, with what the harness cannot see for itself: the agent CLI's version (`claude --version`), the `oskac` version and its commit (with `-dirty` when the repo has uncommitted changes outside `eval/results/` and `docs/evaluation.md`), and the task set's digest (the SHA-256 of every file's path and SHA-256 in `eval/tasks/`).

At the end it writes the batch's `summary.json` and renders `docs/evaluation.md`. A missing tool, an unreadable file or a failed harness step stops the batch with an error. A run whose transcript cannot be read, or whose agent CLI exits with an error and no result (no login, no network), is recorded with the stop reason `error`, counted at the cap, and stops the batch, so a broken setup does not fail every run alike; fix the cause and run the batch again. A failed `record` leaves no partial record. The agent's output is never run or expanded by the driver; it is only read by the harness.

The flags above are those `claude --help` lists for Claude Code 2.1.328, except `--max-turns`, which that help does not list; the driver is tested only with a stub, so check them against the installed CLI before the first batch (`claude --help`). What the flags cannot pin: Claude Code may still read the maintainer's own memory file (`~/.claude/CLAUDE.md`) and any `CLAUDE.md` in a directory above the work directory, so keep those empty or out of the way during a batch, and admin-managed (policy) settings still apply.

Then commit `eval/results/<batch>/` (`batch.json`, `runs/` and `summary.json`) and `docs/evaluation.md` in a pull request. Transcripts and workspaces stay in the work directory and are never committed.

`scripts/eval-batch/test.sh` tests the driver end to end with no model: a stub `claude` on `PATH` (`stub-claude.sh`) replays a recorded transcript and leaves the task's reference solution in the workspace. `scripts/check.sh` runs it.

### What a batch costs

The cost of a run is what the agent CLI reports for it, which depends on the model's prices and on how many turns and tokens the agent uses; the harness does not know any price. So the estimate is the maintainer's: take it from an earlier batch (its summary's `usage.cost_usd` divided by `usage.runs_reported`), or, for a first batch, run a one-task batch first and use its cost. A batch's size is tasks times variants times attempts: the eight active tasks at their five attempts are 40 runs per variant. Each run is bounded by its task's turn limit (40 turns), its timeout (15 minutes) and the cap (`cap_usd_per_run`, which the agent CLI enforces), so 40 runs take at most 10 hours and cost at most 40 times the cap; the plan prints both. The budget is checked before each run, so a batch can pass it by at most one run's cap.

## Results

```sh
target/debug/oska-eval record eval/tasks <task-id> --attempt <n> --variant <name> \
  --model <name> --agent <name> --agent-version <version> \
  --oskac-version <version> --oskac-commit <commit> --task-set <digest> \
  --started <time> --ended <time> [--stop timeout|budget|error] \
  [--workspace <dir>] [--grade <grade.json>] [--analysis <analysis.json>]
target/debug/oska-eval summarise eval/results/<batch>
target/debug/oska-eval report eval/results --out docs/evaluation.md
```

Results are committed in `results/`: one directory per batch, named by its id, holding `batch.json` (its batch config), `runs/` (one run record per run) and `summary.json`. Every file has a `schema` version, `1`.

A **run record** is what `record` prints. It joins the run's grade and analysis with what produced them, and holds no raw transcript text and no local path: the workspace path (`--workspace`) becomes `<workspace>` and the home directory `~`, wherever either starts a path. A run that stopped at a limit is recorded, never dropped: `--grade` and `--analysis` may be left out, and the record holds `null` for each.

| Field | Meaning |
| --- | --- |
| `schema` | The format version, `1`. |
| `task`, `class` | The task's id and class. |
| `attempt`, `attempts` | This run's attempt, from 1, and the task's attempts per batch. |
| `variant` | The batch variant, such as `hook`: lower-case letters, digits, `-` and `_`. |
| `model` | The model the agent CLI ran. |
| `agent` | The agent CLI's `name` and `version`. |
| `oskac` | The `version` and `commit` of the `oskac` under test. |
| `task_set_digest` | The task set's digest, so that only runs over one task set are compared. |
| `started`, `ended` | UTC times, such as `2026-11-20T10:00:00Z`. |
| `stop` | `finished`, `turn_limit`, `timeout`, `budget` or `error`: `--stop` when given, else the analysis's (a transcript with no final message is `error`). |
| `usage` | The analysis's `usage`, as the agent CLI reported it, or `null`. |
| `flags` | `escape` (a tool call outside the workspace), `false_claim` (the run finished but failed the functional checks) and `read_only_changed` (a `read_only` file or an agent-setup file, `AGENTS.md` or `CLAUDE.md`, changed). |
| `grade` | The grade's `passed`, `secure`, `clean`, `changed` and `checks`, or `null`. |
| `analysis` | The analysis's `iterations`, `iterations_to_clean`, `timeline`, `diagnostics`, `fixes`, `lookups` and `escapes`, or `null`. |

A **batch summary** is what `summarise` prints from a batch's `runs/` and its `batch.json`. It refuses a batch that mixes task-set digests, records one attempt twice, has no `batch.json`, or has a run of a task or variant its config does not list.

| Field | Meaning |
| --- | --- |
| `schema` | The format version, `1`. |
| `batch` | The batch's id: its directory's name. |
| `config` | The batch config the batch ran, from its `batch.json`, as `plan` reads it. |
| `started`, `ended` | The first run's start and the last run's end. |
| `task_set_digest` | The one task set the batch ran. |
| `models`, `agents`, `oskac`, `tasks`, `classes`, `variants` | What the batch ran, each sorted. |
| `attempts` | The attempts per task the records declare (the fewest, if tasks differ). |
| `incomplete` | Each task and variant with fewer `runs` than its `attempts`, such as a batch that was cut short. |
| `runs` | The `total`, and how many stopped each way. |
| `usage` | The `turns`, `input_tokens`, `output_tokens` and `cost_usd` summed over the `runs_reported` runs that reported usage. |
| `flags` | How many runs had each flag. |
| `security` | How many runs were `ungraded`, had an attack test fail (`attack_succeeded`), added an audit site the task does not allow (`audit_site`), `widened` authority beyond the expected authority, printed `forbidden_output`, or failed the release group (`release_failed`). |
| `iterations` | How many runs were `analysed`, how many reached a `clean` check, and the mean iterations to that check. |
| `k` | 1 and `attempts`. |
| `pass_rates` | One row for `all`, one per class and one per variant (`by` and `name`): the `pairs` of task and variant in it, their `runs`, and `func@k` and `func_sec@k` for each k, `null` when a pair in the row has fewer runs than k. |
| `codes` | Per diagnostic code, those present in the most iterations first: `occurrences`, `runs`, `iterations_present`, how many were `resolved` and the `mean_iterations_to_resolve`; how many carried a fix (`fix_offered`), had it applied (`fix_applied`), and were gone at the next check after it was applied (`fix_helped`); and those as rates: `fix_offered_rate` of the occurrences, `fix_applied_rate` of those offered, `fix_helped_rate` of those applied (`null` when there are none). |
| `codes_by_variant` | The same table for each `variant`, as its `codes`. |
| `lookups` | The `explain` codes and `site` pages, each with the number of runs that looked it up. |

A run passes **func** when its grade `passed`. It passes **func-sec** when it also passed every security check (`secure`: the attack tests, the output scan, no audit site beyond `allowed_audit` and no authority beyond `expected_authority`) and is `clean` (the release check passes, no read-only file changed, no hidden checks module added and no link). A run with no grade passes neither. func@k is the unbiased estimator `1 - C(n-c, k) / C(n, k)` for `n` runs of a task and variant of which `c` passed, averaged over the tasks and variants of the row; it is not estimated for k above `n`. Rates and means have four decimal places.

`report` renders the site's Evaluation page from `results/`: the latest batch (by its first run) in full, the earlier ones in a table, and the page's limitations. Every text that comes from a record (a model's name, a code, a site page) reaches the page as plain text, its Markdown escaped, never as a link or markup. It refuses a batch whose `summary.json` is not what its runs give. A test holds `docs/evaluation.md` to what `report` renders from `results/`, so after committing a batch, run `summarise` and `report` and commit what they write.

## Reports

Every subcommand prints one JSON report on standard output (`report` prints its page, unless given `--out`), and nothing on standard error. Bad input has one shape for all of them: `{"schema": 1, "valid": false, "errors": [...]}`, with exit code 2, for a missing task directory, an unknown or deferred task, or a run directory that is not fresh (for `prepare`) or has no `workspace/` (for `grade`); `grade` adds an `oskac` that cannot run. `validate` reports a malformed task, or an `oskac` that cannot run, inside its `tasks` list instead (an `errors` list on that task), still with exit code 2, so the rest of the set is still reported. Errors in any JSON the harness reads, a task file or an `oskac` report, say the line and column.
