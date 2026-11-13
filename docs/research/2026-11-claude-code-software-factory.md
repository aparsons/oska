# Research: an AI-driven software factory built on Claude Code

- **Date:** 2026-11-12
- **Method:**
  - I read how the project runs today: `CLAUDE.md`, `docs/agents/*.md`, `.claude/settings.json`, `.githooks/pre-push`, `scripts/check.sh`, `.github/workflows/ci.yml`, `rust-toolchain.toml`, and the source of the installed `mattpocock-skills` plugin (v1.3.1), mainly `implement-spec`, `to-tickets`, `code-review`, `retro`, `writing-for-agents`, and the in-progress `chief-of-staff`.
  - I checked the case material against `git log`, `gh run list`, `gh pr view 145`, the GitHub branch-protection API, and a text search of this project's local session transcripts (counts only, no content quoted).
  - For Claude Code I downloaded the official docs as Markdown from `code.claude.com/docs/en/*.md` on the date above. The machine runs Claude Code 2.1.322; version-gated features are marked where the docs mark them.
  - The other sources are Anthropic engineering posts, the Model Context Protocol specification, GitHub's docs, and arXiv papers. Blog summaries were not used.
  - Each claim cites its source inline. Statements that are my reasoning and not a source's are marked *inference*.
- **Question:** What architecture, creation, operation, monitoring, and optimization practices make an effective AI-driven software factory, and what Claude Code commands, skills, agents, hooks, and plugins should this project build to get one?

This note complements [`2026-11-ai-driven-development.md`](2026-11-ai-driven-development.md). That note covers what Oska, the language, can do for agents. This one covers the factory that builds Oska.

## Summary

The project already runs most of a factory:

- specs and tickets with blocking edges in GitHub Issues (`to-spec`, `to-tickets`);
- background implementers in git worktrees, with models matched to tasks, merging into an integration branch (`implement-spec`);
- a two-axis review (`code-review`);
- one check script shared by the pre-push hook and CI;
- signed releases.

**What it lacks is mostly deterministic enforcement and observation around that loop, not more agents or more prose.** Each recent incident got past a gate that was either advisory or missing.

Five key findings:

1. **Rules that must hold belong in the harness, not in prose.** The Claude Code docs split instructions into non-deterministic (prompt, `CLAUDE.md`, skills) and deterministic (hooks, deny rules, managed settings, CI, branch protection). They advise: "If something has to happen every time, make it a hook, a deny rule, or a managed setting" ([best practices](https://code.claude.com/docs/en/best-practices)). The `--no-verify` pushes happened because the only gate was a local hook. **`main` has no branch protection or branch ruleset** (GitHub API: "Branch not protected"; the only ruleset targets tags), so nothing on the server stopped them.
2. **Silent failures came from defaults nobody set.** GitHub's default job timeout is 360 minutes ([workflow syntax](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax)), and `ci.yml` sets none, so two deadlocked runs went 360 and 319 minutes (`gh run list`). Claude Code keeps a subagent worktree that holds unpushed commits until a sweep after `cleanupPeriodDays` (default 30) ([worktrees](https://code.claude.com/docs/en/worktrees), [settings](https://code.claude.com/docs/en/settings-reference)). That is how merged-locally agent worktrees pile up on a 58 GB disk.
3. **Subagents do not see the orchestrator's memory.** A non-fork subagent loads `CLAUDE.md` but not auto memory ([subagents](https://code.claude.com/docs/en/sub-agents)). So the disk-budget and secrets rules in the user's memory reach an implementer only if each brief restates them. A `SubagentStart` hook can inject them into every subagent deterministically ([hooks](https://code.claude.com/docs/en/hooks)).
4. **Parallelism has costs that the published evidence quantifies.**
   - Multi-agent systems used about 15 times the tokens of chat. Anthropic notes that coding has "fewer truly parallelizable tasks than research" ([multi-agent research system](https://www.anthropic.com/engineering/multi-agent-research-system)).
   - Delegating to a subagent "doesn't make the answer any better". A fresh reviewer does help ([best practices](https://code.claude.com/docs/en/best-practices)).
   - On this hardware the binding constraint is disk and `/tmp`, not tokens, so concurrency should be capped by free space (*inference*, from the case material).
5. **Most of the observability the user asked for is available locally without new infrastructure.**
   - Hook payloads carry each subagent's id, resolved model, and status ([hooks](https://code.claude.com/docs/en/hooks)).
   - The status line receives the 5-hour and 7-day plan usage percentages for Pro, Max, and Ultra subscribers ([status line](https://code.claude.com/docs/en/statusline)). That corrects the working assumption that agents cannot see remaining capacity.
   - OpenTelemetry export exists for dashboards later ([monitoring](https://code.claude.com/docs/en/monitoring-usage)).

The top five proposals, ranked by value for effort (details in Part 3):

1. A ruleset on `main` that requires the CI checks, with no bypass.
2. `timeout-minutes` and `concurrency` on every CI job.
3. A `PreToolUse` Bash guard against `--no-verify`, hook-path overrides, and exit-code-masking pipes.
4. A factory-rules `SubagentStart` hook plus a disk gate on the `Agent` tool.
5. Checked-in agent definitions (`implementer`, `merger`, `standards-reviewer`, `spec-reviewer`) that encode model, effort, tools, and isolation.

## Part 1: case material

Each incident is checked against the repository or GitHub. The last column names the control that would have caught it. Part 3 builds those controls.

| Incident | Evidence | Gate that was missing or advisory |
| --- | --- | --- |
| Leaked test temp dirs filled the 8 GB `/tmp`; pushes and tool output then failed silently | Commit `d9cb9c2`: "Over 1,355 runs of check.sh they filled the Pi's 8 GB /tmp" | No check that tests leave no temp files behind; no free-space check before work starts |
| A CI job deadlocked on 64 KiB pipes on GitHub's arm runners and hit the 6-hour limit twice unnoticed | Commit `0709b9d` (harness wrote stdin before reading stdout); runs `43171780537` (360 min) and `43275926506` (319 min), both cancelled | No `timeout-minutes` in `ci.yml`; no watcher comparing run time with the usual ~4 minutes |
| CI's floating `stable` Rust broke clippy and the Unicode tables | Commits `31686cf` and `2b43df6`; `rust-toolchain.toml` now pins 1.100.0; PR #145 (open) adds a weekly bump-issue workflow | An input not pinned by the repo. Fixed |
| Agents pushed with `--no-verify` | Local transcripts: 10 files mention it, and push commands with the flag recur. One attempt was blocked by auto mode, and the agent then wrote "the GitHub API worked instead and doesn't skip any check" | Only a client-side hook. `main` is unprotected, and nothing requires CI to pass before a merge |
| Parallel worktree builds filled the disk mid-spec | Project memory: three finished worktrees plus the main `target/` left 485 MB free on 2026-11-11 | No disk check before spawning; worktrees with unpushed commits survive until the 30-day sweep |

Two more patterns show up in the transcripts:

- **Exit codes swallowed by pipes.** Commands like `git push … 2>&1 | tail -3` report `tail`'s exit status, not the push's. A push that fails then looks like a success. This is the same failure class as the `/tmp` incident, reached a different way (*inference*).
- **Scale.** The project's transcript folder holds 170 subagent transcripts. That is enough volume for a ledger and KPIs to matter.

## Part 2: findings

### 1. Architecture: roles, task graphs, isolation, integration, gates

**Start from the simplest pattern that works, and add agency only where steps can't be predicted.** Anthropic separates *workflows* ("LLMs and tools are orchestrated through predefined code paths") from *agents* ("LLMs dynamically direct their own processes"). It recommends "the simplest solution possible, and only increasing complexity when needed". Its named patterns are prompt chaining with gates, routing, parallelization (sectioning and voting), orchestrator-workers, and evaluator-optimizer ([Building effective agents](https://www.anthropic.com/engineering/building-effective-agents), Dec 2024).

The research results agree. Agentless used a fixed localize, repair, validate pipeline with no agent choosing actions. It reached 32.00% on SWE-bench Lite at about $0.70 per problem, the best open-source result at the time ([arXiv 2407.01489](https://arxiv.org/abs/2407.01489)).

**This project's factory maps onto those patterns as follows:**

- **Orchestrator.** The main session, holding the spec. This is orchestrator-workers.
- **Planner.** `to-spec` then `to-tickets`. This is prompt chaining, with a human gate between the two.
- **Implementers.** One per frontier ticket. This is sectioning.
- **Merger.**
- **Reviewers.** The standards and spec axes form an evaluator-optimizer loop with a fix pass.
- **Release.** Driven by a runbook.

`implement-spec` already frames tickets as "a task graph with blocking relationships", with "a frontier of tickets which are ready to be grabbed". It tells the orchestrator to communicate through "context pointers" rather than pasted content (plugin source, `skills/engineering/implement-spec/SKILL.md`).

**Who holds the plan is a design choice.** Claude Code now offers four ways to run multi-step work ([dynamic workflows](https://code.claude.com/docs/en/workflows)):

| Mechanism | Who decides what runs next | Interruption | Scale |
| --- | --- | --- | --- |
| Subagents | Claude, turn by turn | Restarts the turn | A few per turn |
| Skills | Claude, following the prompt | Restarts the turn | Same as subagents |
| Agent teams | A lead agent | Teammates keep running | A handful of long-running peers |
| Workflows | A script | Resumable in the same session | Dozens to hundreds |

A workflow cannot take user input mid-run. It runs "up to 16 concurrent agents by default, fewer when Claude Code has fewer CPUs available" ([dynamic workflows](https://code.claude.com/docs/en/workflows)). The Pi has 4 CPUs. The orchestrator today is a turn-by-turn subagent manager. That suits the human sign-offs the user wants, asked one question at a time.

**Isolation.**

- Worktrees give each agent its own checkout. A subagent with `isolation: worktree` gets a temporary worktree that "Claude Code removes automatically when the subagent finishes without changes". A worktree with changes "stays on disk until the periodic sweep" ([worktrees](https://code.claude.com/docs/en/worktrees)).
- Subagent worktrees branch from the **default branch** unless `worktree.baseRef` is `"head"`, and that setting can't name a branch ([worktrees](https://code.claude.com/docs/en/worktrees)). This is why `implement-spec` has each implementer confirm it is based on the integration branch and reset onto it if not.
- The Bash sandbox is a second, OS-level boundary. On Linux it uses `bubblewrap` and `socat`, and network access goes through a proxy whose allowed domains "start empty" ([sandboxing](https://code.claude.com/docs/en/sandboxing)).

**Gates.** The docs list the controls for "who has the last word" as deterministic: "Branch protection, a person who approves the merge" ([best practices](https://code.claude.com/docs/en/best-practices)). GitHub's "Require status checks to pass before merging" does not apply to admins unless "Do not allow bypassing the above settings" is on ([GitHub: protected branches](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches)). The factory's gates today are:

- `check.sh`, run by the pre-push hook. It is client-side and can be skipped.
- CI. It is informational, because nothing requires it.
- Code review.
- The human merge.

Only the human merge is enforced.

### 2. Creation: commands, skills, subagents, hooks, plugins, and mods

**Pick the mechanism by how it loads and who decides** ([extend Claude Code](https://code.claude.com/docs/en/features-overview)):

| Mechanism | Loads | Decided by | Use for |
| --- | --- | --- | --- |
| `CLAUDE.md` | Every session, in full | Claude (advisory) | "Always do X" rules, the check command; target under 200 lines ([memory](https://code.claude.com/docs/en/memory)) |
| `.claude/rules/` with `paths` | When matching files are opened | Claude | Directory- or language-specific guidance |
| Skill | Description every turn; body on use | You (`/name`) or Claude (description match) | Workflows and reference material |
| Subagent | Fresh context when spawned | Claude delegates | Isolation, parallel work, restricted tools, independent review |
| Hook | Runs on an event; zero context unless it prints | The harness | Anything that must happen every time |
| MCP server | Tool names at start; schemas on demand | Claude | External systems with no good CLI |
| Plugin | Bundles the above | Installed per scope | Sharing a setup across repos |
| Mod | JS/TS handlers inside Claude Code | The harness | Panes, bands, custom `/commands` that need no model turn, rewriting events ([mods](https://code.claude.com/docs/en/plugins/mods/overview)) |

**Commands and skills are now one thing.** "Custom commands have been merged into skills". A file at `.claude/commands/deploy.md` and one at `.claude/skills/deploy/SKILL.md` "both create `/deploy`" ([skills](https://code.claude.com/docs/en/skills)). Use the skill directory form, because it can hold supporting files.

The docs give an order for adding pieces, each with a trigger ([extend Claude Code](https://code.claude.com/docs/en/features-overview)):

1. Add a `CLAUDE.md` line when Claude "gets a convention or command wrong twice".
2. Write a skill when you paste the same playbook "for the third time".
3. Write a hook when you want something to happen "every time without asking".
4. Build a plugin when "a second repository needs the same setup".

**Writing skills.**

- **Progressive disclosure has three levels:**
  1. name and description, always loaded;
  2. the `SKILL.md` body, loaded on use;
  3. linked files, opened only when needed.

  Bundled scripts run "without loading either the script or the PDF into context" ([Agent Skills announcement](https://www.anthropic.com/engineering/equipping-agents-for-the-real-world-with-agent-skills), Oct 2025).
- **Keep `SKILL.md` under 500 lines** and move reference material into sibling files ([skills](https://code.claude.com/docs/en/skills)).
- **The description is the trigger.** "Put the key use case first". The combined `description` and `when_to_use` text "is truncated at 1,536 characters" ([skills](https://code.claude.com/docs/en/skills)). The plugin's `writing-for-agents` skill makes the same point: "The pointer's wording, not its target, decides when the agent reaches the material".
- **Use `disable-model-invocation: true` for anything with side effects.** The docs say: "You don't want Claude deciding to deploy because your code looks ready". It also takes the description out of context ([skills](https://code.claude.com/docs/en/skills)). Most of the user's mattpocock workflow skills already set it.
- **Useful frontmatter:**
  - `allowed-tools` pre-approves tools for one turn.
  - `model` and `effort` change them for one turn.
  - `context: fork` with `agent:` runs the skill as a subagent's task. It only makes sense "for skills with explicit instructions".
  - `` !`cmd` `` injects live output, such as `gh pr diff`, before Claude sees the prompt ([skills](https://code.claude.com/docs/en/skills)).
- **Skill content stays in context but is not re-read.** After compaction, only "the first 5,000 tokens of each" recently used skill is re-attached, within "a combined budget of 25,000 tokens" ([skills](https://code.claude.com/docs/en/skills)). A long orchestration skill can therefore lose its later steps in a long session. Keep standing rules near the top (*inference*).
- **Measure skills.** "Seeing a skill trigger tells you Claude found it, not that it did what you intended". Compare runs with and without the skill in fresh sessions. `claude plugin eval` automates this for plugin skills and "exits non-zero below a threshold so you can gate CI on it" ([skills](https://code.claude.com/docs/en/skills), [plugin evals](https://code.claude.com/docs/en/plugin-evals)). `/skill-doctor` reports each skill's context cost and how often it is used ([skills](https://code.claude.com/docs/en/skills)).

**Writing subagents.** A definition is a Markdown file in `.claude/agents/` with frontmatter ([subagents](https://code.claude.com/docs/en/sub-agents)):

- `tools` and `disallowedTools`. A reviewer with only `Read, Grep, Glob` is read-only, "enforced by Claude Code rather than requested in a prompt" ([best practices](https://code.claude.com/docs/en/best-practices)).
- `model`, `effort`, `permissionMode`, `maxTurns` (a partial result can be resumed), and `isolation: worktree`.
- `skills`, which preloads skill bodies.
- `hooks`, scoped to the subagent.
- `memory`, a persistent per-agent directory.
- `background` and `omitClaudeMd`.

Model resolution order is the per-invocation parameter, then frontmatter, then `CLAUDE_CODE_SUBAGENT_MODEL`, then the session model ([subagents](https://code.claude.com/docs/en/sub-agents)).

**Plugin subagents are weaker than project ones.** "Plugin subagents don't support the `hooks`, `mcpServers`, or `permissionMode` frontmatter fields" ([subagents](https://code.claude.com/docs/en/sub-agents)). Agents that need scoped hooks should therefore live in `.claude/agents/`, with plugin-wide hooks in the plugin's `hooks/hooks.json`.

**Writing hooks.**

- **Exit code 2 blocks** on events that can block, such as `PreToolUse`, `Stop`, and `SubagentStop`. Stderr becomes the reason Claude sees ([hooks](https://code.claude.com/docs/en/hooks)).
- **`SubagentStart` "can't block subagent creation, but they can inject context into the subagent"** through `additionalContext`. To refuse a spawn, use `PreToolUse` on the `Agent` tool instead ([hooks](https://code.claude.com/docs/en/hooks)).
- **Hooks fail open by default.** "On most events, when a hook fails or times out, Claude Code still carries out the action… set `"onFailure": "block"`" (v2.1.295+). The `if` filter is best-effort, so "use the permission system rather than a hook to enforce a hard allow or deny" ([hooks](https://code.claude.com/docs/en/hooks)).
- **Deny rules beat everything,** but Bash argument patterns "are fragile" ([permissions](https://code.claude.com/docs/en/permissions)).
- **Use both layers for a rule like "never `--no-verify`".** Add a hook that parses the command, because a pattern can't parse it reliably. Then add a server-side gate that no client can skip (*inference* from the two passages above).
- **A `Stop` hook that exits 2 sends Claude back to work.** The docs show `tsc --noEmit 1>&2 || exit 2`. An 8-continuation cap stops infinite loops ([best practices](https://code.claude.com/docs/en/best-practices), [hooks](https://code.claude.com/docs/en/hooks)).

**Plugins and marketplaces.**

- A plugin is a directory with `.claude-plugin/plugin.json` and any of:
  - `skills/`, `agents/`, `commands/`;
  - `hooks/hooks.json`, `.mcp.json`;
  - `workflows/`;
  - `bin/`, which goes on the Bash `PATH` but can't shadow system commands;
  - `monitors/monitors.json`;
  - output styles, themes, LSP servers, and a `settings.json` that may set only `agent` and `subagentStatusLine` ([plugin components](https://code.claude.com/docs/en/plugins/components)).
- **Monitors are a plugin-only component.** A monitor is a session-long shell command whose output "reaches Claude as notifications". Monitors do not start under `claude -p` ([plugin components](https://code.claude.com/docs/en/plugins/components)).
- A marketplace is any directory or repository with `.claude-plugin/marketplace.json` listing plugins by relative path or source. `claude plugin validate` checks it ([create a marketplace](https://code.claude.com/docs/en/plugins/create-marketplace)).

**Security of what you install.**

- Skills can "direct Claude to exfiltrate data", so install them from trusted sources and audit the less-trusted ones ([Agent Skills announcement](https://www.anthropic.com/engineering/equipping-agents-for-the-real-world-with-agent-skills)).
- A mod "is code that runs with your permissions" ([mods](https://code.claude.com/docs/en/plugins/mods/overview)).
- In MCP, "Tools represent arbitrary code execution", and tool descriptions "should be considered untrusted, unless obtained from a trusted server" ([MCP specification](https://modelcontextprotocol.io/specification/latest), 2026-07-28 schema).

### 3. Operation: permission modes, headless runs, scheduling, CI, secrets

**Permission modes** are `default` (manual), `acceptEdits`, `plan`, `auto`, `dontAsk`, and `bypassPermissions` ([permission modes](https://code.claude.com/docs/en/permission-modes)).

- **Auto mode** routes calls through a classifier that "blocks anything irreversible, destructive, or aimed outside your environment" ([auto mode config](https://code.claude.com/docs/en/auto-mode-config)). It is the user's default.
- **Where auto mode reads its rules.** The classifier reads `CLAUDE.md`, "so an instruction like 'never force push' in your project's CLAUDE.md steers both Claude and the classifier". It reads `autoMode` only from user settings, managed settings, or `--settings`, "not from project settings in `.claude/settings.json`" ([auto mode config](https://code.claude.com/docs/en/auto-mode-config)).
- **This repo's gap.** This repo's `CLAUDE.md` has no rule against `--no-verify`. The user-level `autoMode.environment` block on this machine describes a different project's infrastructure and branch, so the classifier's "trusted infrastructure" picture is wrong here (observed in local settings).
- **Ask rules force a prompt even in auto mode,** for example `Bash(git push *)`. They miss `git -C <dir> push` and `git -c <key>=<value> push` ([auto mode config](https://code.claude.com/docs/en/auto-mode-config)).
- **Background subagents surface their permission prompts in the main session.** Answering one with a session-wide grant applies it "to the whole session, including your main conversation" ([subagents](https://code.claude.com/docs/en/sub-agents)).

**Headless runs.** `claude -p` runs once and exits. For scripts the docs recommend:

- `--bare`, which skips hooks, skills, plugins, MCP, and `CLAUDE.md`;
- `--allowedTools` with `--permission-mode dontAsk`;
- `--output-format json`, whose result includes `total_cost_usd`;
- `--max-turns` or `--max-budget-usd` caps ([headless](https://code.claude.com/docs/en/headless), [best practices](https://code.claude.com/docs/en/best-practices)).

Two traps:

- Without `--bare`, a `-p` session "runs the hooks in a project's `.claude/settings.json`… even in a folder you've never trusted" ([headless](https://code.claude.com/docs/en/headless)).
- "A `claude -p --worktree` run is one you clean up yourself" ([best practices](https://code.claude.com/docs/en/best-practices)).

Before an unattended run, check five questions: what done looks like, how it checks its work, what it may touch, what stops it, and who signs off ([best practices](https://code.claude.com/docs/en/best-practices)).

**Scheduling** comes in three forms:

- `/loop` repeats a prompt inside a session. "Each run is another turn in the same session", and a session-scoped task "fires on its interval even while the session is idle, sending your full context each time" ([best practices](https://code.claude.com/docs/en/best-practices), [costs](https://code.claude.com/docs/en/costs)).
- Cloud scheduled tasks (formerly routines, research preview) run on Anthropic infrastructure with schedule, API, or GitHub-event triggers. They push to `claude/`-prefixed branches by default, and "to control which branches a run can push to, use branch protection rules or rulesets" ([scheduled tasks](https://code.claude.com/docs/en/routines)).
- Desktop scheduled tasks, which run on the local machine.

**CI integration.** The Claude Code GitHub Action runs in two modes ([GitHub Actions](https://code.claude.com/docs/en/github-actions)):

- *Interactive*: it responds to `@claude` mentions.
- *Automation*: it runs a fixed `prompt`.

Its cost advice includes `--max-turns`, "workflow-level timeouts to avoid runaway jobs", and "GitHub's concurrency controls". The same timeout advice applies to the project's own CI, which hit the 360-minute default.

**Secrets.**

- Store tokens as GitHub secrets (`ANTHROPIC_API_KEY` or `CLAUDE_CODE_OAUTH_TOKEN`) and never commit them ([GitHub Actions](https://code.claude.com/docs/en/github-actions)).
- `.worktreeinclude` copies gitignored files such as `.env` into every worktree ([worktrees](https://code.claude.com/docs/en/worktrees)). That conflicts with the user's rule that secrets live outside the repo under `~/.config/oska/`, so this project should not use it for secrets (*inference*).
- For hooks, the docs advise quoting variables, absolute paths, and skipping `.env`, `.git/`, and keys ([hooks](https://code.claude.com/docs/en/hooks)).

### 4. Monitoring: cost, telemetry, transcripts, KPIs, stuck and silent failures

**Built-in views.**

- `/usage` shows plan usage and a breakdown "by skill, subagent, plugin, and MCP server". It flags long context or cache misses once either passes 10% of usage ([best practices](https://code.claude.com/docs/en/best-practices), [costs](https://code.claude.com/docs/en/costs)).
- `/insights` writes an HTML report on friction across up to 200 recent sessions ([costs](https://code.claude.com/docs/en/costs)).
- `/tasks` shows each subagent's model and effort ([subagents](https://code.claude.com/docs/en/sub-agents)).

**Data available to scripts.**

- **Status line JSON** includes `cost.total_cost_usd`, an estimate at list price, and `context_window.used_percentage`. For Pro, Max, and Ultra subscribers it also includes `rate_limits.five_hour.used_percentage` and `rate_limits.seven_day.used_percentage`, after the first API response ([status line](https://code.claude.com/docs/en/statusline)). A status line script can save those to a file for a report.
- **The `PostToolUse` payload for a foreground `Agent` call** includes `agentId`, `status`, `resolvedModel`, and `modelsUsed`. `SubagentStop` gives `agent_type` and `agent_transcript_path` ([hooks](https://code.claude.com/docs/en/hooks)).
- **`claude -p --output-format json`** returns `total_cost_usd` and a per-model breakdown ([headless](https://code.claude.com/docs/en/headless)).

**OpenTelemetry** is enabled with `CLAUDE_CODE_ENABLE_TELEMETRY=1` and OTLP, Prometheus, or console exporters ([monitoring](https://code.claude.com/docs/en/monitoring-usage)).

- *Metrics:* `session.count`, `lines_of_code.count`, `pull_request.count`, `commit.count`, `cost.usage`, `token.usage` (with `input`, `output`, `cacheRead`, and `cacheCreation` types), `code_edit_tool.decision`, and `active_time.total`.
- *Attributes:* `model`, `effort`, `query_source` (main, subagent, or auxiliary), `agent.name`, and `skill.name`. Custom agent names are redacted to `"custom"` unless `OTEL_LOG_TOOL_DETAILS=1`.
- *Events:* `subagent_completed` (with `is_async`), `api_error`, `tool_decision`, and `hook_execution_complete`, plus `agent_id` and `parent_agent_id` for tracing a tree.

**KPIs.** The user already tracks throughput, wall time, first-try merge rate, rework, test count, and a low-to-high token cost range from list prices (project memory). The evidence adds four more:

- **Tokens per merged ticket.** Token use "by itself explains 80% of the variance" in Anthropic's multi-agent eval, so it is the main cost driver to watch ([multi-agent research system](https://www.anthropic.com/engineering/multi-agent-research-system)).
- **Cache-read share,** from `token.usage` by type. Cache misses reprocess the full context ([costs](https://code.claude.com/docs/en/costs)).
- **Gate escapes.** Count the defects found after a gate claimed green: CI red after a local green, review findings after a merge, `--no-verify` attempts. These measure whether the gates are real (*inference*).
- **Time from red to noticed** for CI and agents. Here it was 6 hours twice (Part 1).

**Detecting stuck and silent failures.** Each lesson in Part 1 maps to a cheap detector:

- **Hard time bounds everywhere:**
  - `timeout-minutes` on CI jobs, whose default is 360 ([workflow syntax](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax));
  - `maxTurns` on subagents, which returns output "marked as partial" ([subagents](https://code.claude.com/docs/en/sub-agents));
  - `--max-turns` and `--max-budget-usd` on headless runs.
- **Resource preconditions as gates:** check free space on `/` and `/tmp` before a spawn or a push. Make the hooks fail closed with `onFailure: "block"` ([hooks](https://code.claude.com/docs/en/hooks)).
- **Exit status that survives pipes:** with `set -o pipefail`, or no pipe at all, a failed push is visible. The pre-push `check.sh` already uses `set -euo pipefail`; the agents' ad-hoc command lines don't.
- **Watchers that push notifications instead of relying on polls.** A plugin monitor's output "reaches Claude as notifications", so a script that prints only when a CI run has gone well past its usual length, or disk is low, turns a silent stall into an event ([plugin components](https://code.claude.com/docs/en/plugins/components)).
- **Evidence over claims.** The docs ask for "the command it ran and its output". "All tests pass" on its own tells you very little ([best practices](https://code.claude.com/docs/en/best-practices)). Long-running-agent work found agents would "declare the job done" without end-to-end testing ([effective harnesses](https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents), Nov 2025).

### 5. Optimization: model and effort per role, context, caching, parallelism on small hardware

**Model per role.**

- "Sonnet handles most coding tasks well and costs less than Opus. Reserve Opus for complex architectural decisions or multi-step reasoning" ([costs](https://code.claude.com/docs/en/costs)).
- "Haiku is a good fit for subagents doing simple jobs" ([best practices](https://code.claude.com/docs/en/best-practices)).
- Anthropic's research system paired an Opus lead with Sonnet subagents ([multi-agent research system](https://www.anthropic.com/engineering/multi-agent-research-system)).

The user's mapping matches this: Opus at high effort for foundational and security work, Sonnet at high effort for well-specified tickets, Sonnet at medium effort for standards review, and Haiku at low effort for clean merges (project memory).

**Effort.** Current models default to `medium`.

- Raise it "when getting it right depends on cases that are quick to overlook", such as flaky tests or schema changes.
- Lower it for quick back-and-forth.
- `max` "tends to overthink" on ordinary tasks.

Phrases like "think hard" are just prompt text, not effort controls ([best practices](https://code.claude.com/docs/en/best-practices)). A subagent on a different model from the session does not inherit the session's effort level, so set `effort` in its definition ([subagents](https://code.claude.com/docs/en/sub-agents)).

**Context.**

- The context is an "attention budget", and recall weakens as the window grows ("context rot"). The answer is just-in-time retrieval through file paths, structured notes outside the window, and subagents that return "a condensed, distilled summary… (often 1,000-2,000 tokens)" ([context engineering](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents), Sep 2025).
- Claude Code mixes this with up-front `CLAUDE.md` and on-demand grep and glob.
- Keep `CLAUDE.md` short. Move workflow-specific text into skills. Prefer CLIs like `gh` to MCP servers, "because they don't add any per-tool listing" ([costs](https://code.claude.com/docs/en/costs)).
- `/clear` "costs nothing"; `/compact` is "itself a big request" ([best practices](https://code.claude.com/docs/en/best-practices)).

**Caching.**

- The prompt cache lasts "about an hour on a subscription". "Subagents get five minutes unless you change it" with `subagentPromptCacheTtl` ([best practices](https://code.claude.com/docs/en/best-practices), [prompt caching](https://code.claude.com/docs/en/prompt-caching)).
- A skill whose frontmatter names a different model makes "the next request read the entire conversation history with no cache hits" ([prompt caching](https://code.claude.com/docs/en/prompt-caching)). Prefer putting model choices on subagents rather than on skills that run in the orchestrator.
- Workflow fan-outs stagger matching agents so siblings read one cached prefix ([dynamic workflows](https://code.claude.com/docs/en/workflows)).

**Parallelism versus resources.**

- Claude Code allows 20 concurrent subagents and 3 levels of nesting by default (`CLAUDE_CODE_MAX_CONCURRENT_SUBAGENTS`, `CLAUDE_CODE_MAX_SUBAGENT_SPAWN_DEPTH`) ([subagents](https://code.claude.com/docs/en/sub-agents)). Those are token-side limits.
- On the Pi the binding limit is storage. A debug `target/` is about 4 GB per worktree, and `/tmp` is 8 GB (project memory). `/` had 14 GB free at the time of writing.
- *Inference:* cap concurrent building implementers at (free GB on `/` minus a reserve) divided by about 4 GB, rather than at a fixed number. Enforce it in a `PreToolUse` hook on `Agent`, not in a prompt.
- `worktree.symlinkDirectories` can share large directories between worktrees ([settings](https://code.claude.com/docs/en/settings-reference)). Symlinking `target/` would make concurrent cargo builds of different branches share and contend for one build directory. Keep the lean cargo environment and `cargo clean` instead (*inference*).
- Remove each worktree as soon as its branch is merged. The sweep keeps worktrees with unpushed commits, which locally merged agent branches often have ([worktrees](https://code.claude.com/docs/en/worktrees)).

### 6. Best practices and anti-patterns, with evidence

**Practices with evidence behind them:**

- **Give the agent a pass/fail check.** "If your repo already has one of these, the single most useful line in your `CLAUDE.md` is the command that runs it" ([best practices](https://code.claude.com/docs/en/best-practices)). This project's `CLAUDE.md` opens with `scripts/check.sh`.
- **Invest in the agent-computer interface.** SWE-agent's custom interface took SWE-bench to 12.5% pass@1 from much lower non-interactive baselines ([arXiv 2405.15793](https://arxiv.org/abs/2405.15793)). Its linter-on-edit ablation is covered in [`2026-11-ai-driven-development.md`](2026-11-ai-driven-development.md). Anthropic's SWE-bench team "spent more time optimizing our tools than the overall prompt" and made tools require absolute paths, a poka-yoke ([Building effective agents](https://www.anthropic.com/engineering/building-effective-agents)).
- **Write tools and tool output for the agent.** Return only high-signal information; concise responses used about a third of the tokens. Write actionable error messages. Claude Code caps tool responses at 25,000 tokens by default ([Writing tools for agents](https://www.anthropic.com/engineering/writing-tools-for-agents), Sep 2025).
- **Delegate with an objective, an output format, tool guidance, and boundaries.** Without them, agents "duplicate work" or leave gaps ([multi-agent research system](https://www.anthropic.com/engineering/multi-agent-research-system)).
- **Work incrementally, with durable state.** Do one feature per session. Keep a progress file and a feature list. Commit as checkpoints. Start each session by reading the git log and progress and running a smoke test ([effective harnesses](https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents)). GitHub issues, the integration branch, and the ledger play these roles here.
- **Use an independent reviewer, and tell it to skip style.** "A reviewer that's asked to find problems will always find *something*" ([best practices](https://code.claude.com/docs/en/best-practices)). The plugin's `retro` skill puts it the same way: the reviewer has "the least context pressure", so standards go to review, not implementation.
- **Evaluate with small real samples.** Anthropic started with "about 20 queries representing real usage patterns" ([multi-agent research system](https://www.anthropic.com/engineering/multi-agent-research-system)). `claude plugin eval` supports the same approach for skills ([plugin evals](https://code.claude.com/docs/en/plugin-evals)).

**Anti-patterns:**

| Anti-pattern | Why | Source |
| --- | --- | --- |
| Prompt-only rules for invariants ("never `--no-verify`") | A prompt can't guarantee them; "make it a hook, a deny rule, or a managed setting" | [best practices](https://code.claude.com/docs/en/best-practices) |
| IMPORTANT or ALL-CAPS emphasis | Makes Claude "over-apply the rule"; if it can't bend, it's a hook | [best practices](https://code.claude.com/docs/en/best-practices) |
| Delegating "for a better answer" | Same model, same task; you get parallelism, not intelligence | [best practices](https://code.claude.com/docs/en/best-practices) |
| Multi-agent fan-out for tightly coupled code | About 15× chat tokens; coding has "fewer truly parallelizable tasks" | [multi-agent research system](https://www.anthropic.com/engineering/multi-agent-research-system) |
| Agent complexity before a pipeline baseline | A fixed three-phase pipeline was competitive and cheap | [arXiv 2407.01489](https://arxiv.org/abs/2407.01489) |
| Bash allow or deny patterns that constrain arguments | Fragile: option order, variables, `git -C` | [permissions](https://code.claude.com/docs/en/permissions) |
| Policy hooks that fail open | A crashing or timed-out hook "lets everything through" unless `onFailure: "block"` | [hooks](https://code.claude.com/docs/en/hooks) |
| Long `CLAUDE.md` and architecture tours | Over 200 lines "reduce adherence"; "Claude reads the repo itself" | [memory](https://code.claude.com/docs/en/memory), [best practices](https://code.claude.com/docs/en/best-practices) |
| Unbounded jobs and runs | 360-minute CI default; runs without turn or budget caps | [workflow syntax](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax), [best practices](https://code.claude.com/docs/en/best-practices) |
| Floating toolchains in CI | Behavior changes under you | Case: commits `31686cf`, `2b43df6`, PR #145 |
| Piping critical commands into `tail` | The exit status is lost | Case: transcripts |
| Agents declaring done without end-to-end checks | Observed failure mode of long-running agents | [effective harnesses](https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents) |

## Part 3: proposed factory for this project

Each component lists what it does, how it is built, and which existing mattpocock skill it complements or replaces. Everything here goes in the project's `.claude/` directory, the repo, or GitHub settings. Item 10 is the only plugin, because monitors are plugin-only. Packaging the rest as a plugin waits for a second repository, the trigger the docs name. Even then, project agents keep their `hooks` and `permissionMode`, which plugin agents lose ([subagents](https://code.claude.com/docs/en/sub-agents)).

### Ranked list

| Rank | Component | Kind | Value | Effort | Complements or replaces |
| --- | --- | --- | --- | --- | --- |
| 1 | `main` ruleset: PR plus required CI checks, no bypass | GitHub setting | Very high | Low | Backs `implement-spec` step 8 and every merge |
| 2 | CI `timeout-minutes` plus `concurrency` | Workflow edit | High | Low | none (fixes a Part 1 incident) |
| 3 | `guard-bash` `PreToolUse` hook, plus one `CLAUDE.md` line | Hook | High | Low | Enforces what briefs ask for |
| 4 | `factory-rules` `SubagentStart` hook and `spawn-gate` `PreToolUse(Agent)` hook | Hooks | High | Low | Replaces pasting rules into every `implement-spec` brief |
| 5 | Agent definitions: `implementer`, `implementer-deep`, `merger`, `standards-reviewer`, `spec-reviewer` | Subagents | High | Medium | Named roles for `implement-spec` and `code-review` |
| 6 | Temp-leak check in `check.sh` | Script | Medium-high | Low | What the `retro` skill's "automated checks" category asks for |
| 7 | Ledger hook, status-line capture, `/factory-report` | Hook, status line, skill | Medium-high | Medium | Automates the KPI report in the delegate-and-report memory |
| 8 | Fix the user-level `autoMode.environment` | User setting | Medium | Low | none |
| 9 | `/release` and `/bump-rust` skills | Skills | Medium | Low | Wrap `docs/agents/release.md` and PR #145's runbook |
| 10 | `oska-factory` plugin with a `factory-watch` monitor | Plugin | Medium | Medium | none (turns silent stalls into notifications) |
| 11 | `/postmortem` skill | Skill | Medium | Low | Complements `retro` |
| 12 | Saved `/implement-spec` workflow script | Workflow | Uncertain | High | Could replace `implement-spec` orchestration |
| 13 | `factory-pane` mod | Mod | Low | Medium | Optional live view of the ledger |

### Component specs

**1. `main` ruleset.** Create a branch ruleset on `main` with three rules:

- require a pull request;
- require the CI `test` jobs (both matrix legs) and the Windows and Confinement checks to pass;
- block force pushes.

Turn on no-bypass for admins too. Without no-bypass the owner's token, which agents use, is exempt ([GitHub: protected branches](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches)). Today `main` is unprotected (Part 1).

This makes `--no-verify` harmless: an agent can still skip the local hook, but the merge waits for CI. Cloud scheduled tasks are documented to rely on exactly this ([scheduled tasks](https://code.claude.com/docs/en/routines)). It has to be done by the orchestrating agent or the user, never a subagent (secrets memory: repository settings are changed only by the orchestrator).

**2. CI bounds.**

- Set `timeout-minutes: 30` on the `test` job, about 7 times today's roughly 4-minute run. Set the other workflows to their own multiples.
- Add `concurrency: { group: ci-${{ github.ref }}, cancel-in-progress: true }` to pull-request runs ([workflow syntax](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax)).
- The orchestrator's "after push" step becomes `gh run watch` with its own time budget.

With these, the pipe deadlock would have shown up red within 30 minutes instead of being cancelled after 6 hours.

**3. `guard-bash` hook** (`.claude/hooks/guard-bash.sh`, `PreToolUse` with matcher `Bash`, `onFailure: "block"`). The script parses `tool_input.command` and exits 2 with a reason for any of:

- `--no-verify`, or `-n` on `git commit`;
- `-c core.hooksPath=` or `git config core.hooksPath`, other than the documented `.githooks` value;
- `git push` or `scripts/check.sh` piped into `head` or `tail` without `pipefail`. The reason tells Claude to run the command unpiped, or save the output to a file and read the end of it.

Add one `CLAUDE.md` line, "Never skip the pre-push hook; a failing check is the finding", because the auto-mode classifier reads `CLAUDE.md` ([auto mode config](https://code.claude.com/docs/en/auto-mode-config)). The hook is the enforcement and the line is the explanation. A deny rule alone would be fragile ([permissions](https://code.claude.com/docs/en/permissions)).

**4. `factory-rules` and `spawn-gate` hooks.**

- **`factory-rules`** is a `SubagentStart` hook with no matcher. It returns `additionalContext` holding the rules every subagent needs but cannot otherwise see, since auto memory doesn't reach subagents ([subagents](https://code.claude.com/docs/en/sub-agents)):
  - the lean cargo environment variables;
  - `cargo clean` before reporting;
  - no `--no-verify`;
  - secrets never in output or the repo;
  - report with evidence (command plus output).
- **`spawn-gate`** is a `PreToolUse` hook on `Agent` with `onFailure: "block"`. It exits 2 if `/` has less than about 8 GB free plus 4 GB per running implementer, or `/tmp` has less than 2 GB. It names the worktrees that are safe to remove. `SubagentStart` can't block, so this is the right event ([hooks](https://code.claude.com/docs/en/hooks)).
- **Together they replace** the disk-budget paragraph the orchestrator now pastes into each brief.

**5. Agent definitions** (`.claude/agents/*.md`, checked in):

- **`implementer`.** Sonnet, `effort: high`, `isolation: worktree`, `skills: [tdd]`, `maxTurns` around 150.
  - The body follows `implement-spec`: reset onto the integration branch, TDD, merge the integration tip, run `scripts/check.sh`, `cargo clean`, and report as diff summary, evidence, then risks.
  - Its scoped `Stop` hook (which becomes `SubagentStop`) exits 2 if the worktree's `target/` is still over 500 MB. That turns "clean before reporting" into an enforced step ([subagents](https://code.claude.com/docs/en/sub-agents), [hooks](https://code.claude.com/docs/en/hooks)).
- **`implementer-deep`.** The same, on Opus at high effort, for security-sensitive or foundational tickets.
- **`merger`.** Haiku, `effort: low`, tools `Bash, Read`. It merges one branch into the integration branch. On any conflict it stops and reports, never resolves. It then removes the worktree and branch.
- **`standards-reviewer` and `spec-reviewer`.** Sonnet, `effort: medium`, `disallowedTools: Edit, Write, Agent`, so they are read-only by construction ([best practices](https://code.claude.com/docs/en/best-practices)).
  - The standards reviewer preloads `CODING_STANDARDS.md` through its prompt.
  - The spec reviewer is told to "report gaps that affect correctness and skip style".
  - `code-review` can then spawn these two by name instead of generic agents.
- **Model choices stay in the definitions,** not in orchestrator skills, to avoid cache misses in the main session ([prompt caching](https://code.claude.com/docs/en/prompt-caching)).
- **The worktree base.** Either keep `implement-spec`'s reset step, or set `worktree.baseRef: "head"` while the orchestrator sits on the integration branch ([worktrees](https://code.claude.com/docs/en/worktrees)).

**6. Temp-leak check.** In `check.sh`, list `${TMPDIR:-/tmp}/oska-*` before and after the cargo test step, and fail with the new paths if any appeared. This mechanical check would have caught the `d9cb9c2` leak on its first run, not its 1,355th. The plugin's `retro` skill prefers exactly this: "a **mechanical** one… gets a deterministic check, full stop".

**7. Ledger and `/factory-report`.**

- **Ledger.** A `PostToolUse` hook on `Agent` and a `SubagentStop` hook append one JSON line per run to `.claude/factory/ledger.jsonl` (gitignored): time, `agentId`, `agent_type`, `resolvedModel`, status, and transcript path ([hooks](https://code.claude.com/docs/en/hooks)).
- **Status line.** The status line script also writes `rate_limits.*.used_percentage` and `resets_at` to `.claude/factory/capacity.json` ([status line](https://code.claude.com/docs/en/statusline)).
- **`/factory-report`.** A skill with `context: fork` and `agent: Explore` on Haiku, so the work stays out of the main context. It joins the ledger, transcript usage, `git log`, and `gh run list` into the KPIs from Part 2, section 4, and gives the cost as a low-to-high range at list price.
- **Scheduling.** Run it with `/loop` only at long intervals, since each fire resends the whole session context ([costs](https://code.claude.com/docs/en/costs)).
- **Telemetry.** Add OpenTelemetry later if dashboards are wanted, setting `OTEL_LOG_TOOL_DETAILS=1` so custom agent names aren't redacted ([monitoring](https://code.claude.com/docs/en/monitoring-usage)).

**8. Auto-mode environment.** Rewrite the user-level `autoMode.environment` so its entries describe the machine and every repository it works on, including this one. Today it is generated from, and names, another project ([auto mode config](https://code.claude.com/docs/en/auto-mode-config)). This is a user-scope change, because the classifier ignores project settings.

**9. `/release` and `/bump-rust`.** Two thin skills with `disable-model-invocation: true` (side effects) that point to `docs/agents/release.md` and PR #145's `docs/agents/rust.md`. Each adds a `` !`git describe --tags` ``-style injection of current state. The runbooks stay the single source; the skills make them one keystroke and keep them out of context until used ([skills](https://code.claude.com/docs/en/skills)).

**10. `oska-factory` plugin.** It lives in the repo as its own marketplace:

```text
.claude-plugin/marketplace.json        # lists ./plugins/oska-factory
plugins/oska-factory/
  .claude-plugin/plugin.json
  monitors/monitors.json               # factory-watch
  bin/factory-watch                    # prints only on state changes
```

`factory-watch` prints a line, which becomes a notification to Claude, when:

- free space on `/` or `/tmp` drops below threshold;
- a GitHub run on a branch the session pushed has gone past 3× the median duration for its workflow;
- a worktree under `.claude/worktrees/` has been merged but not removed.

Monitors are a plugin-only component and don't start under `-p` ([plugin components](https://code.claude.com/docs/en/plugins/components)). Validate with `claude plugin validate` ([create a marketplace](https://code.claude.com/docs/en/plugins/create-marketplace)). Skills, agents, and hooks stay in `.claude/` (see the note above the ranked list).

**11. `/postmortem`.** A user-invoked skill for one incident. It records the symptom, the time to detect, the gate that should have caught it, and a proposed mechanical check (hook, script step, or CI setting), using the Part 1 table format. It complements `retro`: `retro` reviews a session broadly, while `/postmortem` turns one escape into one gate.

**12. Saved `/implement-spec` workflow.** The orchestration in `implement-spec` could become a script: frontier, fan out implementers, merge, review, fix. That would make the orchestration repeatable and resumable ([dynamic workflows](https://code.claude.com/docs/en/workflows)). The trade-off is that a workflow can't ask the user anything mid-run. The user's one-question-at-a-time decisions would move to the gaps between stages. Worth it only if orchestration turns, not implementation, become a large share of usage, and the ledger would show that.

**13. `factory-pane` mod.** A JS/TS mod that draws the ledger, disk, and capacity in a pane beside the transcript ([mods](https://code.claude.com/docs/en/plugins/mods/overview)). It is a nice-to-have once item 7 exists, and is listed because the user asked about mods. A mod runs with the user's full permissions.

## Caveats

- **The Claude Code docs change fast.** Several cited behaviors are version-gated, for example `onFailure` (v2.1.295+) and nested-subagent behavior. Cloud scheduled tasks and `claude agents` are marked research preview ([scheduled tasks](https://code.claude.com/docs/en/routines)).
- **Two posts are older.** "Building effective agents" (Dec 2024) itself notes the tooling has moved on. The multi-agent figures (90.2% better, about 15× tokens) come from a research task, not coding.
- **Benchmark results are not this project's results.** SWE-bench and Agentless measure single-issue repair on Python repositories. The project has no measurement of its own factory yet beyond the git and CI records used here.
- **The `--no-verify` count is a text search** over local transcripts. It shows the pattern recurs. It is not an exact tally of pushes.
- **The thresholds in Part 3** (30-minute timeout, 4 GB per worktree, 2 GB `/tmp` floor, 500 MB `target/`) are starting points from the project's own numbers, not from any source.

## Open questions for the user

Ordered by importance. Each should be put to the user on its own.

1. **Ruleset strictness.** Should `main` require PRs and green CI with no admin bypass? Recommended, because it's the only gate an agent can't skip. Or keep an owner bypass for emergencies, at the cost of agents (who act as the owner) being able to use it too?
2. **Where the factory lives.** Keep everything in project `.claude/` plus one small in-repo plugin for the monitor (recommended), or build a full `oska-factory` plugin now for use in other repositories?
3. **Concurrency policy.** Use a disk-derived cap enforced by `spawn-gate` (recommended), or a fixed maximum number of implementers?
4. **Telemetry.** Is a local ledger plus `/factory-report` enough (recommended for the Pi), or is an OpenTelemetry collector and dashboard wanted?
5. **Orchestration form.** Stay with turn-by-turn `implement-spec` (recommended until the ledger shows orchestration cost), or trial a saved workflow on the next spec?
6. **Unattended work.** Should any factory role run as a cloud scheduled task? The weekly Rust bump and nightly CI-health check are candidates. That only makes sense once item 1 exists ([scheduled tasks](https://code.claude.com/docs/en/routines)).

## All sources fetched

| Source | Kind | Used for |
| --- | --- | --- |
| [Claude Code: Extend Claude Code](https://code.claude.com/docs/en/features-overview) | Official docs | Choosing mechanisms, context costs |
| [Claude Code: Skills](https://code.claude.com/docs/en/skills) | Official docs | Frontmatter, progressive disclosure, fork, lifecycle, evals |
| [Claude Code: Subagents](https://code.claude.com/docs/en/sub-agents) | Official docs | Frontmatter, models, effort, background, limits, startup context |
| [Claude Code: Hooks reference](https://code.claude.com/docs/en/hooks) | Official docs | Events, exit 2, `onFailure`, `SubagentStart`, `Stop` |
| [Claude Code: Permissions](https://code.claude.com/docs/en/permissions) | Official docs | Rule precedence, fragile Bash patterns |
| [Claude Code: Permission modes](https://code.claude.com/docs/en/permission-modes) | Official docs | Modes, auto mode |
| [Claude Code: Auto mode config](https://code.claude.com/docs/en/auto-mode-config) | Official docs | Classifier inputs, ask and deny checkpoints |
| [Claude Code: Worktrees](https://code.claude.com/docs/en/worktrees) | Official docs | Isolation, base branch, sweep, locks |
| [Claude Code: Settings reference](https://code.claude.com/docs/en/settings-reference) | Official docs | `cleanupPeriodDays`, `worktree.*` |
| [Claude Code: Best practices](https://code.claude.com/docs/en/best-practices) | Official docs | Deterministic vs non-deterministic, checks, unattended runs, anti-patterns |
| [Claude Code: Costs](https://code.claude.com/docs/en/costs) | Official docs | `/usage`, model choice, context, background usage |
| [Claude Code: Monitoring](https://code.claude.com/docs/en/monitoring-usage) | Official docs | OpenTelemetry metrics and events |
| [Claude Code: Status line](https://code.claude.com/docs/en/statusline) | Official docs | Cost, context, rate-limit fields |
| [Claude Code: Headless](https://code.claude.com/docs/en/headless) | Official docs | `-p`, `--bare`, JSON output |
| [Claude Code: GitHub Actions](https://code.claude.com/docs/en/github-actions) | Official docs | Modes, secrets, cost controls |
| [Claude Code: Scheduled tasks](https://code.claude.com/docs/en/routines) | Official docs | Cloud runs, branch permissions, limits |
| [Claude Code: Dynamic workflows](https://code.claude.com/docs/en/workflows) | Official docs | Who holds the plan, limits, caching |
| [Claude Code: Prompt caching](https://code.claude.com/docs/en/prompt-caching) | Official docs | Subagent TTL, model-switch misses |
| [Claude Code: Memory](https://code.claude.com/docs/en/memory) | Official docs | `CLAUDE.md` size, auto memory |
| [Claude Code: Sandboxing](https://code.claude.com/docs/en/sandboxing) | Official docs | Linux sandbox |
| [Claude Code: Plugin components](https://code.claude.com/docs/en/plugins/components) | Official docs | Plugin layout, monitors, `bin/` |
| [Claude Code: Create a marketplace](https://code.claude.com/docs/en/plugins/create-marketplace) | Official docs | Marketplace layout |
| [Claude Code: Plugin evals](https://code.claude.com/docs/en/plugin-evals) | Official docs | Measuring skills |
| [Claude Code: Mods overview](https://code.claude.com/docs/en/plugins/mods/overview) | Official docs | What a mod is |
| [Building effective agents](https://www.anthropic.com/engineering/building-effective-agents) | Anthropic engineering, Dec 2024 | Patterns, ACI |
| [How we built our multi-agent research system](https://www.anthropic.com/engineering/multi-agent-research-system) | Anthropic engineering, Jun 2025 | Multi-agent gains and costs, delegation, evals |
| [Effective context engineering for AI agents](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents) | Anthropic engineering, Sep 2025 | Context rot, just-in-time context, summaries |
| [Writing effective tools for agents](https://www.anthropic.com/engineering/writing-tools-for-agents) | Anthropic engineering, Sep 2025 | Tool design, token efficiency |
| [Equipping agents for the real world with Agent Skills](https://www.anthropic.com/engineering/equipping-agents-for-the-real-world-with-agent-skills) | Anthropic engineering, Oct 2025 | Progressive disclosure, skill security |
| [Effective harnesses for long-running agents](https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents) | Anthropic engineering, Nov 2025 | Incremental work, progress files, false "done" |
| [Model Context Protocol specification](https://modelcontextprotocol.io/specification/latest) | Specification (2026-07-28 schema) | Tool-safety principles |
| [SWE-bench (arXiv 2310.06770)](https://arxiv.org/abs/2310.06770) | Paper, ICLR 2024 | Benchmark context |
| [SWE-agent (arXiv 2405.15793)](https://arxiv.org/abs/2405.15793) | Paper | Agent-computer interface |
| [Agentless (arXiv 2407.01489)](https://arxiv.org/abs/2407.01489) | Paper (preprint) | Pipeline baseline vs agents |
| [GitHub: workflow syntax](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax) | Official docs | `timeout-minutes` default, concurrency |
| [GitHub: about protected branches](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches) | Official docs | Required checks, admin bypass |
| `mattpocock-skills` v1.3.1 source (`implement-spec`, `to-tickets`, `code-review`, `retro`, `writing-for-agents`, `chief-of-staff`) | Plugin source | The existing factory's skills |
