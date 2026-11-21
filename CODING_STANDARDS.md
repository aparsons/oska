# Coding standards

Reviewers apply these rules to every diff. The mechanical rules are enforced by `scripts/check.sh` and don't appear here:

- rustfmt;
- clippy with `unwrap_used` denied;
- `unsafe_code` forbidden;
- no crate from outside the repo;
- only official actions and `dtolnay/rust-toolchain` in workflows, each pinned to a full commit SHA with its version in a trailing comment;
- a `timeout-minutes` of 1 to 60 on every workflow job.

Everything below needs judgement.

## Language and runtime

- **Grow Oska rather than route around it.** When an Oska program in this repo, such as the site generator, needs something the language lacks, add it to the prelude and the runtime with checker and runtime tests. Follow the existing `DirRead.list_dir` and `DirWrite.create_dir` additions: the prelude, `Host`, `RealHost`, `FakeHost`, intrinsics, and the coverage test. A shell step that does the job instead is a violation.
- **The parity gate: every change lands on all five platforms** ([ADR 0014](docs/adr/0014-every-supported-platform-builds-and-runs-natively.md)). A change that adds or changes what a program does (syntax, types, the prelude, `std`, capabilities, the runtime) is done only when it works natively, with the same output and the same security guarantees, on aarch64 and x86_64 Linux, aarch64 and x86_64 macOS, and x86_64 Windows, with the differential suite green on each platform's CI runner. A feature on some platforms only, or one that leaves the native backend refusing it (`E0390`), is a violation. The interpreter is the reference, behind `--interpret`, never a fallback.
- **Use the glossary's words.** Docs and messages use the terms in `GLOSSARY.md`, never the ones it lists under _Avoid_.

## Site generator (`site/build.osk`)

- **The site generator is an Oska program,** run with `oskac run`.
- **`main` narrows its file access before doing anything else.** It may read only the inputs it uses and write only `_site`.
- **Everything emitted goes through `std.html`.** Hand-built markup is allowed only for static literals.
- **Pages load nothing from another origin, and the `<meta>` CSP names none. No analytics.** Fonts are the system's. A third-party asset would need a pinned version, SRI, and a decision to add its origin to the CSP. The site conformance tests in `crates/oska-run/tests/site.rs` check that no page loads from another origin and check its scripts. A new kind of asset needs a matching test.

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
  - Exception: a test that reads the real site (the generator on the repository's own docs) goes in `crates/oska-run/tests/site.rs`, whatever its ticket. The build takes seconds, and each test program makes it once.
- **Tests that build a path, write a path into JSON, or set a home use `crates/oskac/tests/common/portable.rs`**: `Scratch`, `json_str`/`json_path`, `set_home` (`HOME`, `USERPROFILE`, `XDG_CONFIG_HOME`), and `literal_path` (keeps `..`; `join("..")` drops it on Windows `\\?\` paths). Do not hand-escape backslashes or set only `HOME`.
- **No `#[ignore]`.** A test that needs a missing tool, such as `node`, says so and skips.
