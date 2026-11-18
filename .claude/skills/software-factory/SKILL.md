---
name: software-factory
description: Shared vocabulary and design questions for a software factory of agent runs. Use it in a grilling session.
disable-model-invocation: true
argument-hint: "[problem the factory must solve]"
---

# Software factory

A software factory is a system of agent runs. It turns tickets, pull requests, and schedules into merged code. Use this skill during a grilling session, to design a factory with the user.

## Glossary

Use these terms with these meanings only. The "Avoid" column lists words that cause confusion.

| Term | Meaning | Avoid |
| --- | --- | --- |
| **Task** | A kind of work. You define it one time, and it runs many times. Example: "implement a ticket". | job, workflow, agent |
| **Trigger** | An event that starts a run. It comes from a tracker change, a git host change, a clock, or another run. | hook, signal |
| **Run** | One execution of a task on one runner. A run never waits for a human. A later human change starts a new run. | session, job |
| **Runner** | A disposable machine. Nothing on it survives a run. | worker, server, agent |
| **Ticket tracker** | The human interface to the factory. | board, queue |
| **Ticket source** | Anything outside the tracker that files tickets: a feedback button, a monitor, an error tracker, or an inbox. | input, feed |
| **Git host** | The service that holds the repo and the pull requests. | forge, remote |
| **Orchestrator** | Receives triggers, starts runs, and records the state of each run. | scheduler, controller |
| **Observability platform** | Optional. Holds logs and traces of runs. | monitoring |

Memory lives only in the tracker, the git host, and the orchestrator. A runner has no memory.

## Design rules

- **Start from the user's problem.** Ask what work hurts today. Do not start from the factory. For a catalogue of common tasks, read `EXAMPLE-TASKS.md` in this folder.
- **Brakes before accelerators.** First add the brakes: automated checks, an automated review by a separate run, and a merge gate: human review, or a written auto-merge policy with hard conditions and human-only exceptions. Then add triggers, one at a time, with a throttle.
- **Cap every loop.** A loop gets a few tries, for example 3. Then it hands the work to a human.
- **Fill every slot.** Each slot gets a product or a deliberate "none". The slots are: tracker, ticket source, git host, orchestrator, runner, and observability.
- **Buy services, build tasks.** Buy the services in the slots. Build only the tasks. Use an off-the-shelf tracker.
- **Choose products with care.** Do not use a remembered vendor list. Read `CHOOSING-SERVICES.md` in this folder, and research current options on the day of use.

## Three example designs

| Slot | All on one git platform | SaaS tracker with own orchestrator | Fully own app |
| --- | --- | --- | --- |
| Ticket tracker | The git platform's issues | A SaaS tracker | Own app with a ticket view |
| Ticket source | Issue forms, error tracker | Tracker intake form, monitor | Own feedback button, monitor |
| Git host | The git platform | A git platform | A git platform |
| Orchestrator | The platform's CI workflows | Own small service that receives webhooks | Own app |
| Runner | The platform's hosted runners | Disposable cloud VMs or containers | Own disposable containers |
| Observability | CI logs only | A log and trace service | Own log store |

## Worked example: Oska

Oska's own setup is the worked example. Use it to make each slot concrete. For each fact, the source is the named file, as read on 2026-11-18. Read the file again before you rely on the fact.

| Slot | Oska |
| --- | --- |
| Ticket tracker | GitHub Issues on aparsons/oska. A spec is a "Spec: ..." issue with `ready-for-agent`. Its tickets are sub-issues with native blocking edges (`docs/agents/issue-tracker.md`). |
| Ticket source | People, review findings filed as "Follow-ups from the PR #<pr> review (#<spec>)", and the weekly `rust-stable.yml`, which keeps one open "Bump Rust to X.Y.Z" issue with `ready-for-agent`. |
| Git host | GitHub. Branches `spec/<n>-<slug>` and `ticket/<n>-<slug>`. |
| Orchestrator | A coordinator session (`chief-of-staff`) that runs `implement-spec` and records each dispatch in `docs/wiki/`. GitHub Actions for checks and releases. |
| Runner | The shared, CPU-bound Pi for agent runs, and Claude Code cloud sessions. GitHub-hosted runners for CI on Linux, macOS, and Windows. |
| Observability | CI logs, and the dated ledger on each goal page in `docs/wiki/`. |

Oska's brakes: `scripts/check.sh` with its pass stamp and the pre-push hook, CI on each ticket branch, a two-axis review of each ticket PR (`diff-review`), and `scripts/expected-workflows/check.sh` before a merge (`docs/agents/implementer.md`).

Oska's deliberate gap: the Pi is not a disposable runner. Runs share its CPU and its disk. Thus Oska adds build slots (`OSKA_BUILD_SLOTS`), logs named per ticket outside the worktree, and a kill guard hook that refuses a kill by name. Name such a gap, and its compensating controls, in every design.

## Steps

1. **Find the problem.** Ask the user which work hurts today, and how often it occurs. Grill until the problem is concrete.
   Done when: you can state the problem, its frequency, and its cost.

2. **Choose tasks.** Pick the smallest set of tasks that solves the problem. Use `EXAMPLE-TASKS.md` for ideas.
   Done when: each task links to the problem.

3. **Answer the task questions.** For each task, answer:
   - What is the trigger?
   - Which actions does it do? Which services does it read and write?
   - What is the next trigger, if any?
   - Where does a human re-enter?
   - What stops two runs from doing the same work?

   For each ticket source, answer: what stops duplicate tickets?
   Done when: each question has an answer for each task and each ticket source.

4. **Fill the slots.** Put a product or "none" in each slot. Use `CHOOSING-SERVICES.md` for the questions that decide each slot.
   Done when: every slot has a product or a deliberate "none", with a dated reason.

5. **Check the brakes and security.** Compare the design with the Security section below.
   Done when: each security rule is met, or the user accepted a gap in writing.

## Security

Security is a core design concern for a factory. Runs act without a human, so each weakness repeats on each run.

- **Prompt injection (S1).** Runs read untrusted ticket text and pull request text. The task prompt must treat that text as data. A run must not obey instructions in it.
- **Least privilege (S3).** Give each run a dedicated bot identity. Use scoped tokens and short-lived credentials.
- **External contributors (S6).** A run that an external contributor triggers gets no secrets and no write token.
- **Network.** Restrict runner network egress to an allowlist.
- **No self-approval (S5).** A run never approves its own review. A merge happens only under a human's review or a written merge policy that the human approved, such as Oska's `docs/agents/merge-policy.md`.
- **Branch protection (S5).** Protect the default branch with required checks.
- **Audit.** Log every run: the trigger, the identity, the actions, and the result.
- **Factory changes.** Each change to the factory itself, such as a task prompt, a token scope, or a trigger, gets human review.
- **Secrets (S2).** Keep secrets in the platform's secret store. Do not put a secret in a ticket, a prompt, or a log.
