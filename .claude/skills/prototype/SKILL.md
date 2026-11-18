---
name: prototype
description: Build short-lived code to settle one design question. Use to try a state machine, a rule, or a language feature before the real work, or to compare UI layouts.
---

# Prototype

A prototype is short-lived code with one job: to settle one design question. The question decides the shape of the prototype. When the question has an answer, the prototype has done its job.

## Steps

1. **Write the question.** Write the design question as one sentence. Example: "Can a function that gets a `DirRead` give a narrower one to a callee without the caller seeing it?"
   Done when: the question is one sentence that has a clear answer.

2. **Pick the branch.**
   - A question about the Oska language, the checker, the runtime, or code generation: read [LANGUAGE.md](LANGUAGE.md) and follow it. In aparsons/oska, this is the default.
   - A logic or state question that a non-developer must drive: read [LOGIC.md](LOGIC.md) and follow it.
   - A look-and-feel question, for example about the site in `site/`: read [UI.md](UI.md) and follow it.

   If the type is not clear and the user is away, look at the code near the question. Code under `crates/` or an `.osk` file means language. A page or a template means UI. Other backend code means logic. Write this assumption at the top of the prototype.
   Done when: you chose one branch, and any assumption is at the top of the prototype.

3. **Build the prototype.** Follow the rules below and the branch file.
   Done when: the prototype runs with one command or one double-click, and it shows the full state or output after each action.

4. **Get the answer.** Show the prototype to the user. Record the question and the answer.
   Done when: the question has an answer from the user, or you recorded "no answer yet" with the reason.

5. **Close the prototype.** Do these actions:
   1. Move the decision that the prototype proved into the real code, with tests. If the decision is hard to reverse, record it as an ADR in `docs/adr/`.
   2. Commit the prototype to a branch named `prototype/<name>`. Do not merge that branch.
   3. Link the branch from the implementation issue or the spec issue.
   4. Write the question and the answer in the issue or in the pull request.

   Done when: the real code has the decision, and the issue links the prototype branch.

## Rules for both branches

- Mark the code clearly as a prototype, in its file name and at its top.
- Put the prototype near the code that it serves.
- Make it run with one command or one double-click.
- Keep the state in memory. Use storage only when persistence is the question. Then use a scratch store with a clear throwaway name, such as `prototype-scratch.db`.
- Do not write tests that check the prototype. A throwaway Rust test that is the prototype itself is allowed. Do not polish. Do not add abstractions.
- Show the full relevant state after each action and after each variant switch.

## Security

- **Self-contained.** The logic HTML file loads no remote scripts, styles, or fonts. Put all code and styles inline.
- **No switcher in production.** The UI variant switcher renders only in non-production builds.
- **Fake data only.** Prototypes use fake data and stubbed writes. They never use real credentials or production data.
- **Pinned dependencies.** If a prototype needs a new dependency, pin it to an exact version. In Oska, add no crate from outside the repo, even in a prototype.
- **Least authority in `.osk` prototypes.** `main` declares only the capabilities that the question needs. Point file access at a temporary directory, not at your home or the repo.
