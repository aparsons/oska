# Language and compiler prototype

Use this branch for a design question about the Oska language, the checker, the runtime, or code generation. Examples: "Can a closure that captures a `DirRead` cross a task boundary?" or "Does this attenuation read well at the call site?"

The prototype is code that runs, not a page. Use the HTML shell from [LOGIC.md](LOGIC.md) only when a person who does not write code must drive the answer.

## Pick the form

| Question | Form | Run it with |
|---|---|---|
| How does the surface syntax read? Which programs does a rule accept? | A small `.osk` program, or a few of them side by side | `cargo run -q -p oskac -- check <file>.osk` |
| What does a program do at run time? | A small `.osk` program with a `main` that declares only the capabilities it needs | `cargo run -q -p oskac -- run <file>.osk` |
| Does a checker or runtime change have the right shape? | A throwaway Rust test that calls the `oska-check` or `oska-run` API | `cargo test -p <crate> --test prototype_<name>` |
| Does native output match the interpreter? | A throwaway case for the differential harness in `crates/oskac/tests/differential/` | `cargo test -p oskac --test prototype_<name>` |

## Steps

1. **Read the decisions first.** Read the ADRs in `docs/adr/` and the terms in `GLOSSARY.md` that touch the question. Write the ADR numbers at the top of the prototype.
   Done when: the prototype header names each ADR that applies, or says "none".

2. **Write the variants.** Write 2 or 3 variants of the program or the API, a maximum of 5. Put each variant in its own file, named `prototype_<name>_<variant>.osk` or `.rs`. Each variant has the question as a comment at the top.
   Done when: each variant runs with one command from the table.

3. **Write the edge cases.** For each variant, include these cases:
   - the common case, as a user would write it;
   - a tricky edge case, such as a generic, a closure, or a nested module;
   - an attempt that must fail, such as a capability that `main` did not declare, untrusted text in a trusted slot, or a path outside the root.
   Done when: the attempt that must fail gives a diagnostic or a refusal in each variant.

4. **Show the results.** Put the output of each run next to its variant. Show the diagnostics in full, with their codes.
   Done when: the user can compare the variants from the output alone.

## Rules

- Keep the prototype files outside `crates/*/tests/` and outside `examples/`, or on the `prototype/<name>` branch only. A Rust prototype test that must sit in `tests/` stays on that branch.
- Do not change the prelude, `Host`, or the checker for a prototype on the main line of work. Make such changes on the `prototype/<name>` branch.
- The real change later follows `CODING_STANDARDS.md` and the `tdd` skill, with new tests.
