---
name: tdd
description: Test-driven development as a red-green loop at agreed seams. Use to build a feature test-first, to fix a bug test-first, when the user says "red-green", or to write integration tests.
---

# Test-driven development

Build behaviour one small cycle at a time. Each cycle starts with one failing test (red). Then write the least code that makes it pass (green). The tests that stay must be worth keeping.

## Terms

- **Seam**: the public interface where a test observes behaviour. Callers use the same interface. For questions about the shape of an interface, call the Skill tool with `codebase-design`.
- **Good test**: a test that checks behaviour through a public interface. It reads like a statement of one capability. It survives a refactor of the code behind the seam.
- **Horizontal slice**: all the tests first, then all the code. Do not work this way. Work in vertical cycles: one test, then the code for it.

## Before the first test

Read `GLOSSARY.md`. Use its terms in test names. Read the ADRs in `docs/adr/` for the area, and respect them. Read the section "Tests" of `CODING_STANDARDS.md`. It says "test through the highest seam" and names the seams for each area. Read [OSKA-SEAMS.md](OSKA-SEAMS.md) for the Oska seams, test helpers, and run commands.

In another repo, find the test layout and the test command in its contributor guide or CI configuration. Use its highest public interface as the default seam.

## Steps

1. **Propose the seams.** List each seam that you want to test at. For each seam, write one line: what a test there catches, and what it misses. Show the list to the user. If the user is away, choose the highest seam that the repo standards name, and write down that assumption.
   Done when: the user agreed the seams, or you recorded the assumed seams.

2. **Write one failing test.** Pick the next behaviour. Write one test for it at an agreed seam. Use an expected value that you wrote as a literal, not a value that you computed like the code does.
   Done when: the test file compiles and contains exactly one new test.

3. **See it fail for the right reason.** Run only that test, for example `cargo test -p oska-run --test <file> <test name>`. Read the failure message. It must show the missing behaviour, not a typo, a missing import, or a setup error.
   Done when: the test fails, and the message names the missing behaviour.

4. **Write the least code that passes.** Change only the code that this test needs. Do not add code for later tests.
   Done when: the new test passes, and the other tests in the file still pass.

5. **Repeat.** Go back to step 2 for the next behaviour. Each cycle has one seam, one test, and one change. Do not refactor inside the loop. Keep a list of refactor ideas for the review.
   Done when: every required behaviour has a passing test.

6. **Refactor in review.** After the loop, do the refactors from your list. Run the tests after each refactor. The tests must not change during a refactor.
   Done when: the refactors are done and all tests pass with no test edits.

## Anti-patterns

- **Coupled to the implementation.** The test mocks internal parts, calls a private function, or checks through a side channel. A direct database query is a side channel.
- **Tautological.** The test computes the expected value the same way the code does. So the test cannot fail.
- **Horizontal slicing.** All tests first, then all code.

Read [TESTS.md](TESTS.md) when you write or review a test and want examples of good and bad tests. Read [MOCKING.md](MOCKING.md) when the code under test calls an external service, the clock, randomness, the filesystem, or a database.

## Security

- **Hostile input at trust boundaries.** For code at a trust boundary, write at least one test with hostile input. Examples: malformed input, oversized input, injection-shaped input, a symlink, and a `..` path.
- **Attack test for a security fix.** A security fix comes with a test that feeds the bad input. Run it before the fix and see it fail. `CODING_STANDARDS.md` (Release and install path) gives examples.
- **No real secrets or data.** Tests do not use real credentials or production data. Use fake values that look clearly fake.
- **Checks stay whole.** Do not skip, disable, or weaken a test to make the suite pass. Do not add `#[ignore]`. Do not refresh golden files with `UPDATE_GOLDEN=1` to hide a change that you did not intend. Fix the code instead.
