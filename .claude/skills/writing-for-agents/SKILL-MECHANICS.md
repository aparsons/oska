# Skill mechanics

Read this file when you write the frontmatter of a skill, or when you decide how a skill starts.

## Frontmatter fields

Each `SKILL.md` starts with YAML frontmatter between two `---` lines.

| Field | Use |
| --- | --- |
| `name` | Required. The same as the folder name. |
| `description` | Required. The pointer to the skill. See below. |
| `disable-model-invocation: true` | Makes the skill user-invoked. |
| `argument-hint` | A short hint for the argument, for example `"[issue number]"`. Add it when the skill takes an argument. |

## Invocation choice

There are two kinds of skill.

**Model-invoked.** The description is always in context. The agent can start the skill by itself, and other skills can call it with the Skill tool. The user can also type its name. Use this kind when the agent or another skill must reach the material. The description names what the skill does, then each trigger case once. Keep it under 300 characters, because it costs context on every turn.

**User-invoked.** Add `disable-model-invocation: true`. The skill has zero context cost until the user starts it. Only the user can start it. The description is one short line for a human.

## Shared reference

Sometimes two user-invoked skills need the same reference. Put it in a plain doc, for example under `docs/agents/`. Point to that doc from both skills. Do not make a model-invoked skill only to share text.

## When to split off a model-invoked skill

Split off a model-invoked skill only for one of these reasons:

- It has a distinct trigger word that the agent will meet in normal work.
- Another skill must call it.

Otherwise keep the material as a reference file inside the skill that uses it.

## Router skills

A router is one user-invoked skill that lists the other skills and says when each one fits. It can only suggest a skill. It cannot call a user-invoked skill, because the Skill tool cannot start those. The user types the suggested name.
