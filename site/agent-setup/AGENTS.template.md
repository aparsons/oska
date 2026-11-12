## Working with Oska

This project is written in [Oska](https://oskalang.dev), a language where a function can reach only the capabilities it is handed, and text from outside is tracked as `Untrusted`. The compiler `oskac` is built for this loop.

### The loop

1. Check: `oskac check <file.osk> --format=json`. Read `diagnostics`, and match on each `code`, not on the message.
2. Apply machine fixes: if a diagnostic has a `fix`, apply its `edits` (the bytes `span.start` to `span.end` are replaced by `replacement`), then check again. Apply one fix at a time.
3. Fix the rest by hand, one diagnostic at a time. `oskac explain <code>` explains a code offline, with an example and its fix. `oskac explain <code> --format=json` says the same as JSON.
4. Run: `oskac run <file.osk> -- args`. On aarch64 Linux it compiles the program natively and runs it; elsewhere it uses the interpreter. Output, reports and exit codes are the same either way. If a native result looks wrong, compare with `oskac run --interpret` and report any difference as a bug.
5. Test: `oskac test <file.osk>` (native or interpreted, as `run`). Add a test for every behavior you add.
6. Read the authority report from `oskac check <file.osk> --format=json` (`functions` and `audit`). It says what each function can reach.
7. Before you finish, `oskac check <file.osk> --release` must pass.

### Layout

A program is one `.osk` file, or a package: an `oska.toml` (`[package]` with a `name` and a `version`) and modules under `src/`, where `src/main.osk` is the entry and `use pkg.a.b` imports `src/a/b.osk`. For a package, run `oskac check`, `oskac run` and `oskac test` in its directory, with no file. Items are private to their module until marked `pub(pkg)` (the whole package) or `pub` (its API); when `E0206` offers a fix, it adds that marker. There are no dependencies, re-exports or import renaming yet. `oskac build <file.osk>` (or `oskac build` in a package) writes a static aarch64 Linux executable, on any host, that runs without `oskac`; `-o out` names it.

### Rules

- Do not widen authority to make an error go away: do not add capabilities to `main`, and do not call `.assume_trusted()` or `.expose()` without saying why. Prefer a narrower capability, such as `fs.read_only()` or `fs.restrict_to("dir")`.
- Anything from files, arguments, standard input, or the environment is `Untrusted`. Check it before you use it.
- Never write a secret into a source file, a test, or a log.
- Say in your summary when the authority report changed.

### Where to read more

- Index for agents: https://oskalang.dev/llms.txt (all pages: https://oskalang.dev/llms-full.txt).
- Every error code, with an example and its fix: https://oskalang.dev/codes.json, or one code at https://oskalang.dev/codes/E0300.
- Agent guide: https://oskalang.dev/guide/agent-guide.md.
- Packages and modules: https://oskalang.dev/guide/packages.md.
- To update Oska, run the installer again: https://oskalang.dev/install.sh or https://oskalang.dev/install.ps1.
