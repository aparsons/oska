# Coding standards

Reviewers apply these rules to every diff. The mechanical rules are enforced by `scripts/check.sh` and don't appear here: rustfmt, clippy with `unwrap_used` denied, `unsafe_code` forbidden, no crate from outside the repo, and only official actions in workflows, with `dtolnay/rust-toolchain` pinned to a commit. Everything below needs judgement.

## Language and runtime

- **Grow Oska rather than route around it.** When an Oska program in this repo, such as the site generator, needs something the language lacks, add it to the prelude and the runtime with checker and runtime tests. Follow the existing `DirRead.list_dir` and `DirWrite.create_dir` additions: the prelude, `Host`, `RealHost`, `FakeHost`, intrinsics, and the coverage test. A shell step that does the job instead is a violation.
- **Use the glossary's words.** Docs and messages use the terms in `GLOSSARY.md`, never the ones it lists under _Avoid_.

## Site generator (`site/build.osk`)

- **The site generator is an Oska program,** run with `oskac run`.
- **`main` narrows its file access before doing anything else.** It may read only the inputs it uses and write only `_site`.
- **Everything emitted goes through `std.html`.** Hand-built markup is allowed only for static literals.
- **Third-party assets need a pinned version, SRI, and a `<meta>` CSP naming only their origins. No analytics.** The site conformance tests in `crates/oska-run/tests/site.rs` check fonts and scripts. A new kind of asset needs a matching test.

## Release and install path

- **Verification fails closed.** In the installers and `scripts/release/`, any check that cannot complete, such as a missing tool, an unreadable file, or an unexpected format, refuses with an error code. It never skips.
- **Security fixes come with an attack test.** Each fix gets a test that feeds the bad input, such as a tampered file, an older signed release, a symlink, or an `http://` redirect, and is red without the fix.
- **Secrets reach only jobs that use them.** They never reach a `pull_request` job or a step that runs untrusted input.
- **Workflows never change repository settings.** A workflow that needs configuration says so in its header comment, and the release runbook lists it.

## Tests

- **Test through the highest seam.**
  - The generator runs through the `oska-run` API with `FakeHost` and fixture docs under `site/tests/<case>/`. Golden output is refreshed with `UPDATE_GOLDEN=1`, and only for intentional changes.
  - The CLI uses golden files under `crates/oskac/tests/golden/`.
  - Language additions use the existing `oska-check` and `oska-run` test helpers.
- **Each piece of work adds its own integration test file** rather than growing a shared one.
- **No `#[ignore]`.** A test that needs a missing tool, such as `node`, says so and skips.
