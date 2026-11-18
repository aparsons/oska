# Learning record format

Read this file when you write a file in `learning-records/`.

A learning record says what the user has shown that they know. You use the records to choose the next lesson.

## When to write one

Write a record for each of these events:

1. The user showed understanding, for example in a quiz, a task, or an explanation in their own words.
2. The user stated knowledge that they had before.
3. The user had a misconception, and it is now corrected.
4. The mission changed.

Do not write a record only because a lesson covered a subject. Do not use the records as an activity log.

## Rules

- File name: `learning-records/NNNN-slug.md`. `NNNN` goes up by one for each record.
- Content: a title, then one to three sentences.
- Optional fields: **Status**, **Evidence**, **Implications**.
- To change a record, write a new record that supersedes it. Do not delete the old one. Set the status of the old one to `Superseded by NNNN`.

## Format

```markdown
# <What the user knows, as one statement>

<One to three sentences.>

Status: Active | Superseded by NNNN
Evidence: <what the user did or said>
Implications: <what this changes for the next lessons>
```

## Examples

```markdown
# Reads starter activity from the rise and the smell

The user judged three starters as ready, not ready, and over-ripe, with the correct reason for each.

Status: Active
Evidence: lesson 0004 practice set, 3 of 3 correct.
Implications: next lessons can use "peak" without explanation.
```

```markdown
# Bulk fermentation ends on dough volume, not on the clock

The user thought that bulk fermentation always takes 4 hours. They now judge it by a 50% rise and a domed surface.

Status: Active
Evidence: corrected misconception in lesson 0006; then two correct calls in the practice set.
Implications: give a temperature table in the reference sheet.
```
