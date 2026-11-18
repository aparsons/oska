# Glossary format

Read this file before you write or edit a glossary.

## Structure

A glossary file has these parts, in this order:

1. A title: the name of the project or the context.
2. One paragraph of context. It says what the system does and who uses it.
3. The terms. Each term has:
   - the term in bold, followed by a colon;
   - a definition of 1 or 2 sentences;
   - an `_Avoid_:` line that lists the rejected synonyms, when there are any.

When terms form clusters, put each cluster under a `##` heading.

## Oska

`GLOSSARY.md` at the repo root already has this structure. Its title is `# Oska`, and its clusters are `##` sections such as Authority and Untrusted data. Read the cluster that you will change before you add a term, and copy its style.

## Rules

- Pick one word for each concept. Put the other candidate words in the `_Avoid_:` line.
- Define what a term is. Do not define what it does step by step.
- Include only terms that are special to this project. Leave out general programming terms, such as "function" or "queue", unless the project gives them a special meaning.
- Keep implementation detail out. Do not name files, classes, or database tables.
- Keep plans out. Write only what is true now.
- When two terms are near each other, say how they differ in one of the two definitions.

## Example: a library lending domain

```markdown
# Lending desk

The lending desk system records which members borrow which copies of the library's titles. Librarians use it at the desk, and members use it to reserve copies.

## Collection

**Title**:
A work that the library holds, such as one novel in one edition. A title can have many copies.
_Avoid_: book, item

**Copy**:
One physical object of a title that a member can take home. Each copy has its own barcode.
_Avoid_: book, volume, unit

## Lending

**Loan**:
The record that one member has one copy for a fixed period. A loan ends when the copy comes back.
_Avoid_: checkout, borrow (as a noun)

**Hold**:
A member's place in the queue for the next free copy of a title. A hold is on a title, not on a copy.
_Avoid_: reservation, request

**Overdue loan**:
A loan whose due date is in the past and whose copy has not come back.
```

## Single context and multi-context layouts

Most repos have one context. They use one `GLOSSARY.md` at the repo root.

A repo with more than one context has a `GLOSSARY-MAP.md` at the root. The map lists each context, the path of its glossary, and how the contexts relate. Example:

```markdown
# Glossary map

| Context  | Glossary                      | Relation                                     |
| -------- | ----------------------------- | -------------------------------------------- |
| Lending  | `src/lending/GLOSSARY.md`     | Uses "Member" from Accounts.                 |
| Accounts | `src/accounts/GLOSSARY.md`    | Owns "Member". Sends fines to Lending.       |
```

When the same word means different things in two contexts, define it in each glossary. Add a line about the difference in the map.
