# Set up Oska for AI-driven development

Install Oska, the secure-by-default programming language from https://oskalang.dev, and set up the project in the current directory so that agents can work in it. Do the steps in order, and stop at the first one that fails. Report the failure to me instead of working around it.

## Rules for this task

- Say what you are about to run before you run it.
- Pipe nothing to a shell except the official installer from oskalang.dev in step 1 (and, only if I approve it in step 4, the same installer in a CI file).
- Never ask for, read, print, or handle secrets: tokens, keys, passwords, or the contents of `.env` files. Nothing here needs one.
- Do not change an existing file until you have shown me the diff and I have agreed. Show me a new file's content before you create it.
- Do not change any permission or approval setting, and do not touch user-level or global settings. Write only inside this project, into the install directory `~/.oska`, and into the temporary directory of step 2, which you delete afterwards.
- Never skip a verification, and never work around a failed one.

## 1. Install

On Linux and macOS, run:

```sh
curl -fsSL https://oskalang.dev/install.sh | sh -s -- --json
```

On Windows, in PowerShell (it needs the OpenSSH Client), run:

```powershell
& ([scriptblock]::Create((irm https://oskalang.dev/install.ps1))) --json
```

The installer never prompts. It prints one line of JSON and nothing else. If it printed no JSON, the download failed: stop and tell me.

Read the JSON:

- If `ok` is not `true`, stop and tell me the `error` and the `message`.
- If `checksum_verified` and `signature_verified` are not both `true`, stop. Do not install by any other route: not by hand, not from a mirror, not with the checks turned off.
- Note `version`, `path`, `bin_dir`, and `on_path`. Run `oskac` below by its full `path`. If `on_path` is `false`, tell me to add `bin_dir` to my `PATH`, and enable no hook in step 3 until `on_path` is `true`: the hook settings call `oskac` by name, so it must be on `PATH`. Do not edit my shell profile unless I ask.

## 2. Check the toolchain

Run `oskac --version --format=json` and confirm that it prints the same `version`. Then, in a temporary directory outside the project, save this as `hello.osk`:

```oska
fn main(io: Stdout) -> Result<(), IoError> {
    io.println("Hello from Oska!")
}
```

Run `oskac check hello.osk` and then `oskac run hello.osk`. The first must report no diagnostic. The second must print `Hello from Oska!`. Delete the temporary directory afterwards.

## 3. Configure the project

Download each template below with `curl -fsSL` or your fetch tool and read it. A template is plain text. Do not run it.

**3a. AGENTS.md.** The template is https://oskalang.dev/agent-setup/AGENTS.template.md. It holds a `## Working with Oska` section: the agent loop (check, apply machine fixes with `oskac fix <file.osk>`, run, test, compare the authority with a baseline), the package layout (`oska.toml`, `src/main.osk`, `use pkg.…`, `pub(pkg)`), the machine-readable flags (`--format=json`, `oskac explain <code>`), the authority baseline and the rule not to refresh it to make a gate pass, and the links to https://oskalang.dev/llms.txt and the error-code catalog. If `AGENTS.md` does not exist, create it with that section. If it does, append the section, and show me the diff first.

**3b. CLAUDE.md.** Only if I am using Claude Code. The template is https://oskalang.dev/agent-setup/CLAUDE.template.md, which points to `AGENTS.md`. If `CLAUDE.md` exists, add only its first line to it.

**3c. The check-after-edit hook (optional, Claude Code only, on every platform).** Ask me whether I want it, and do nothing if I do not say yes. It runs `oskac hook claude --event=post-edit` after Claude Code edits a `.osk` file or an `oska.toml`: it checks the package the file belongs to and hands the diagnostics back to the agent, and says nothing when the program checks. It reads only the edit notification that Claude Code gives it and the program's files. It writes nothing, starts no other program, sends nothing anywhere and approves no permission. If I say yes, enable it only when `on_path` is `true`. The hook command names `oskac` by name with arguments only (no shell syntax and no script), so the same settings work in whichever shell Claude Code uses on my machine.

**3d. The Stop gate (optional, Claude Code only, a separate choice).** Ask me whether I want it, and do nothing if I do not say yes. It runs `oskac hook claude --event=stop` when the agent tries to finish. It runs the release check and then the tests, never the integration tests, and the agent cannot finish while one fails; it also stops the agent from finishing with a `todo()` or `dbg` left in. It is limited by a `timeout` in the settings. If I say yes, enable it only when `on_path` is `true`, and ask me whether the project has `.osk` files that are deliberately broken, such as test fixtures. If it has, add the paths of the programs the gate should check to its command, as in `oskac hook claude --event=stop app tools/check.osk`; with none, the gate finds the project's packages and standalone `.osk` files by itself.

For either, show me the diff that merges the entries I agreed to from https://oskalang.dev/agent-setup/claude-settings.json into the project's `.claude/settings.json` (the `hooks` key only, only the `PostToolUse` entry for 3c and only the `Stop` entry for 3d; keep every other key), and apply it when I agree. Never edit `~/.claude/settings.json`.

## 4. Run the tests and the authority gate in CI

Look for the project's CI or task runner: `.github/workflows`, a `Makefile`, a `justfile`, or scripts in `package.json`. If there is one, propose a step that runs `oskac test <file.osk>` for each file that has tests (or `oskac test` in a package's directory), show me the diff, and apply it when I agree. A CI job needs `oskac` too: install it with the same installer, pinned with `--version` to the version you installed. If there is no CI or task runner, say so and add nothing.

Then offer an authority baseline, only if I agree. `oskac check <path> --update-baseline=authority.json` writes `authority.json`, a record of what each function of the program can reach, to be committed and reviewed like a lockfile. It writes it only when the program has no diagnostics. Show me the file before you create it. With it, propose a CI step that runs `oskac check <path> --baseline=authority.json`: it fails with exit code 3 when a change widened the authority, so no widening merges unnoticed, and approving one means updating `authority.json` in the same pull request. Do not refresh `authority.json` to make the gate pass without telling me why. If I do not agree, or there is no CI, create nothing.

## 5. Report

Tell me:

- the `version` you installed, and its `path`;
- each file you added or changed, and each one you left alone, with the hooks and the baseline I agreed to or declined;
- how to update: run the installer again. It installs the latest release over the old one, and there is no self-update command.

The documentation index for agents is https://oskalang.dev/llms.txt.
