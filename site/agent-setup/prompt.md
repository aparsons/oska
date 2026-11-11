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
- Note `version`, `path`, `bin_dir`, and `on_path`. Run `oskac` below by its full `path`. If `on_path` is `false`, tell me to add `bin_dir` to my `PATH`. Do not edit my shell profile unless I ask.

## 2. Check the toolchain

Run `oskac --version --format=json` and confirm that it prints the same `version`. Then, in a temporary directory outside the project, save this as `hello.osk`:

```oska
fn main(io: Stdout) -> Result<(), IoError> {
    io.println("Hello from Oska!")
}
```

Run `oskac check hello.osk` and then `oskac run hello.osk`. The first must report no diagnostic. The second must print `Hello from Oska!`. Delete the temporary directory afterwards.

## 3. Configure the project

Download each template below with `curl -fsSL` or your fetch tool and read it. A template is plain text. Do not run it, except the hook script in step 3c, and only if I opt in.

**3a. AGENTS.md.** The template is https://oskalang.dev/agent-setup/AGENTS.template.md. It holds a `## Working with Oska` section: the agent loop (check, apply machine fixes, run, test, read the authority report), the package layout (`oska.toml`, `src/main.osk`, `use pkg.…`, `pub(pkg)`), the machine-readable flags (`--format=json`, `oskac explain <code>`), and the links to https://oskalang.dev/llms.txt and the error-code catalog. If `AGENTS.md` does not exist, create it with that section. If it does, append the section, and show me the diff first.

**3b. CLAUDE.md.** Only if I am using Claude Code. The template is https://oskalang.dev/agent-setup/CLAUDE.template.md, which points to `AGENTS.md`. If `CLAUDE.md` exists, add only its first line to it.

**3c. The check-after-edit hook (optional, Claude Code only).** The hook is a POSIX shell script, so it works on Linux and macOS only: on Windows, skip this step and tell me it is not available there yet. Ask me whether I want it, and do nothing if I do not say yes. It runs `oskac check --format=json` on a `.osk` file after Claude Code edits it, and it reads only the edit notification that Claude Code gives it. It sends nothing anywhere and approves no permission. If I say yes:

1. Show me https://oskalang.dev/agent-setup/oska-check.sh, and save it as `.claude/hooks/oska-check.sh` in this project.
2. Show me the diff that merges https://oskalang.dev/agent-setup/claude-settings.json into the project's `.claude/settings.json` (the `hooks` key only; keep every other key), and apply it when I agree. Never edit `~/.claude/settings.json`.

## 4. Run the tests in CI

Look for the project's CI or task runner: `.github/workflows`, a `Makefile`, a `justfile`, or scripts in `package.json`. If there is one, propose a step that runs `oskac test <file.osk>` for each file that has tests, show me the diff, and apply it when I agree. A CI job needs `oskac` too: install it with the same installer, pinned with `--version` to the version you installed. If there is no CI or task runner, say so and add nothing.

## 5. Report

Tell me:

- the `version` you installed, and its `path`;
- each file you added or changed, and each one you left alone;
- how to update: run the installer again. It installs the latest release over the old one, and there is no self-update command.

The documentation index for agents is https://oskalang.dev/llms.txt.
