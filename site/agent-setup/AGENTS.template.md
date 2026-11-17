## Working with Oska

This project is written in [Oska](https://oskalang.dev), a language where a function can reach only the capabilities it is handed, and text from outside is tracked as `Untrusted`. The compiler `oskac` is built for this loop.

### The loop

1. Check: `oskac check <file.osk> --format=json`. Read `diagnostics`, and match on each `code`, not on the message.
2. Apply machine fixes: `oskac fix <file.osk>` applies every fix that is certain, checks again after each, writes each changed file once, and prints what it changed and which diagnostics remain. `oskac fix <file.osk> --dry-run` shows the diff and writes nothing. It never applies a fix whose `applicability` is `maybe`, and applies one that is `placeholders`, such as the `todo()` arms of `E0253`, only with `oskac fix <file.osk> --placeholders`: read what it did and fill them in. Do not apply fixes by hand with the bytes of their `edits`.
3. Fix the rest by hand, one diagnostic at a time. `oskac explain <code>` explains a code offline, with an example and its fix. `oskac explain <code> --format=json` says the same as JSON.
4. Run: `oskac run <file.osk> -- args`. On aarch64 and x86_64 Linux it compiles the program natively and runs it; elsewhere it uses the interpreter. Output, reports and exit codes are the same either way. If a native result looks wrong, compare with `oskac run --interpret` and report any difference as a bug.
5. Test: `oskac test <file.osk>` (native or interpreted, as `run`). Add a test for every behavior you add.
6. Compare the authority: `oskac check <file.osk> --baseline=authority.json` compares what each function can reach with the project's committed baseline (see below), and lists the functions that gained a capability, the new audit sites, and the functions that stopped being pure. It exits with exit code 3 when authority widened. Without a baseline, save the report before you change anything (`oskac check <file.osk> --format=json > before.json`) and compare with `oskac check <file.osk> --baseline=before.json`. The authority report is in `functions` and `audit` of `oskac check <file.osk> --format=json`.
7. Before you finish, `oskac check <file.osk> --release` must pass.

### The authority baseline

`authority.json` is the program's authority baseline: a saved record of its authority, committed with it and reviewed in any change that edits it. `oskac check <file.osk> --update-baseline=authority.json` writes it, and only when the program has no diagnostics. CI runs `oskac check <file.osk> --baseline=authority.json`. There is no flag that lets a widening pass: approving one is a reviewed change to `authority.json`.

### Layout

A program is one `.osk` file, or a package: an `oska.toml` (`[package]` with a `name` and a `version`) and modules under `src/`, where `src/main.osk` is the entry and `use pkg.a.b` imports `src/a/b.osk`. For a package, run `oskac check`, `oskac run` and `oskac test` in its directory, with no file. Items are private to their module until marked `pub(pkg)` (the whole package) or `pub` (its API); when `E0206` offers a fix, it adds that marker. There are no dependencies, re-exports or import renaming yet. `oskac build <file.osk>` (or `oskac build` in a package) writes a static Linux executable, on any host, that runs without `oskac`: for the host's platform (`linux-aarch64` where the host has no backend), or with `--target linux-x86_64` or `--target linux-aarch64` for another one (`--target all` writes both, named `<name>-<platform>`, into the `-o` directory); `-o out` names it.

### Rules

- Do not widen authority to make an error go away: do not add capabilities to `main`, and do not call `.assume_trusted()` or `.expose()` without saying why. Prefer a narrower capability, such as `fs.read_only()` or `fs.restrict_to("dir")`.
- Anything from files, arguments, standard input, or the environment is `Untrusted`. Check it before you use it.
- Never write a secret into a source file, a test, or a log.
- Do not refresh a committed baseline to make the gate pass without telling the user why. Updating it is the approval of a wider authority, and that is the user's decision.
- Say in your summary when the authority report changed.

### Where to read more

- Index for agents: https://oskalang.dev/llms.txt (the documentation in one file: https://oskalang.dev/llms-full.txt).
- Every error code, with an example and its fix: https://oskalang.dev/codes.json, or one code at https://oskalang.dev/codes/E0300.
- Agent guide: https://oskalang.dev/guide/agent-guide.md.
- Packages and modules: https://oskalang.dev/guide/packages.md.
- How this `oskac` was installed: `oskac self info --format=json` (offline; it reads the install receipt).
- Whether an update exists: run the installer with `--check --json` (it installs nothing). To update Oska, run the installer again: https://oskalang.dev/install.sh or https://oskalang.dev/install.ps1.
