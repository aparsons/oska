# Domain docs

This file tells skills how to use the domain docs of this repo.

## Read before you explore

Before you explore the code for a task, read:

- `GLOSSARY.md` at the root. If `GLOSSARY-MAP.md` exists instead, read it. Then read each glossary that it lists for the area of the task.
- The ADRs in `docs/adr/` that touch the area of the task. In a multi-context repo, also read the ADR folder of the context.

An ADR (architecture decision record) is a short dated file that records one decision and its reason.

If a file or folder does not exist, continue silently. Do not report that it is missing. Do not propose to create it.

## Use the glossary terms

In all output, use the terms that the glossary defines: issue titles, proposals, test names, and commit messages. Do not use a synonym that the glossary rejects. If you need a concept that the glossary does not have, note it as a gap.

## Report ADR conflicts

If your output contradicts an ADR, say so explicitly. Name the ADR and give the reason to reopen it. Do not override the decision silently.
