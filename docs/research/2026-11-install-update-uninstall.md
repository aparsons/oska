# Research: installing, updating and uninstalling Oska

- **Date:** 2026-11-12
- **Method:** Read the installers, release pipeline, site generator, agent-setup files and `oskac` CLI at `release-0.3.0` (commit `102beb0`), citing each claim by file and line. For prior art, fetched official docs and source for rustup, uv, Deno, Bun, the Go toolchain, Homebrew, winget, Scoop, The Update Framework (TUF), minisign and OpenSSH's `ssh-keygen`. No blog posts. Unverified statements are marked as inferences.
- **Question:** How are updates to Oska currently handled, and what functionality would let AI agents and humans easily install, update and uninstall Oska?

## Summary

Today the installer is the only lifecycle tool. `install.sh` and `install.ps1` install, update (re-run), reinstall and pin (`--version`). They verify an SSH signature against a pinned key, a signed tag line, a SHA-256 checksum and the binary's self-reported version. They never prompt and have a `--json` mode with stable error codes. `oskac` itself has no update, update-check or uninstall command and makes no network requests. Uninstall is "delete `~/.oska`".

The main gaps:

1. Nothing tells a user or agent that a new release exists.
2. The unsigned `latest.json` chooses the tag, so a rollback to an older genuine release is not refused, and a downgrade is reported as `"updated"`.
3. Uninstall is manual. It misses a custom `OSKA_HOME`, the PATH line the user added, and the agent-setup hook, which then fails on every edit.
4. Projects cannot state which `oskac` they need.

The recommended path, cheapest first:

1. **Harden and extend the installers.** Add a JSON receipt, a downgrade guard, `--check` and `--uninstall`.
2. **Add a signed, expiring release index.** This is a TUF-lite fix for freeze and rollback.
3. **Add a thin `oskac self` command family.** It reports, checks and uninstalls, and delegates installation to the verified installer.

A self-updater inside `oskac` that does its own TLS and signature checks should wait for the Oska TLS client ([ADR 0012](../adr/0012-tls-git-and-crypto-are-written-in-oska.md)). It should then be written in Oska so that its authority is visible.

## Part 1: current state

### 1. How a user or agent installs

- **Two one-line installers**, published at `https://oskalang.dev/install.sh` and `install.ps1` ([`docs/guide/install.md:29-39`](../guide/install.md)). The sources are [`scripts/install/install.sh`](../../scripts/install/install.sh) and [`scripts/install/install.ps1`](../../scripts/install/install.ps1). They are not under `site/`: the Pages build copies them into the site and pins the key ([`scripts/release/stage-site.sh:13-14,59-60`](../../scripts/release/stage-site.sh)).
- **Options:** `--version X`, `--json` and `--help`. Environment: `OSKA_HOME` (default `~/.oska`) and `OSKA_SITE_URL`, which accepts only `https://` or `file://` ([`install.sh:7-15`](../../scripts/install/install.sh)).
- **Passing options:** under `irm … | iex` options cannot be passed. The workaround is the scriptblock form ([`install.ps1:5-7`](../../scripts/install/install.ps1), [`install.md:55-59`](../guide/install.md)).
- **No prompts.** The installer is meant for scripts, CI and agents ([`install.sh:23`](../../scripts/install/install.sh), [`install.md:43`](../guide/install.md)).
- **Platforms:** Linux and macOS on x86_64 and aarch64, with Rosetta shells mapped to the native build ([`install.sh:122-143`](../../scripts/install/install.sh)). Linux needs glibc 2.35 or newer, and musl is refused with `unsupported_libc` ([`install.sh:145-171`](../../scripts/install/install.sh)).
- **Agents** are given a copy-and-paste prompt ([`site/agent-setup/prompt.md`](../../site/agent-setup/prompt.md)):
  - It runs the installer with `--json` and stops on `ok != true` or on any unverified check ([`prompt.md:14-34`](../../site/agent-setup/prompt.md)).
  - It confirms `oskac --version --format=json` ([`prompt.md:38`](../../site/agent-setup/prompt.md)).
  - It pins CI to the installed version with `--version` ([`prompt.md:63`](../../site/agent-setup/prompt.md)).
- **Building from source** with `cargo install --path crates/oskac` is the alternative ([`install.md:5-25`](../guide/install.md)).

### 2. How a release is produced and published

- **The release flow** ([`docs/agents/release.md:7-15`](../agents/release.md), [`.github/workflows/release.yml:3-17`](../../.github/workflows/release.yml)):
  1. A tag `vX.Y.Z` triggers builds for 5 targets.
  2. [`assemble.sh`](../../scripts/release/assemble.sh) writes `SHA256SUMS`. Its first line is `# oska-release vX.Y.Z` (`assemble.sh:12-15,57`).
  3. [`sign.sh`](../../scripts/release/sign.sh) signs it with `ssh-keygen -Y sign -n oska-release`. It refuses a private key whose public half is not the committed `site/oska-release.pub` (`sign.sh:8-10,30-33`).
  4. The files go to a GitHub Release ([`release.yml:248-251`](../../.github/workflows/release.yml)).
  5. `pages.yml` is dispatched ([`release.yml:253-254`](../../.github/workflows/release.yml)).
- **The Pages build** downloads every GitHub Release. It re-verifies each one with [`verify.sh`](../../scripts/release/verify.sh), which checks the signature, the tag line matching the directory name, every digest, and that no unlisted file is present (`verify.sh:6-14`). It then stages each under `/releases/<tag>/` ([`pages.yml:38-49`](../../.github/workflows/pages.yml), [`stage-site.sh:62-68`](../../scripts/release/stage-site.sh)).
- **`latest.json`** is generated in two layers:
  - The site generator writes `version` (the docs' version), `docs: "main"` and the installer URLs ([`site/build.osk:421-427`](../../site/build.osk)).
  - `stage-site.sh` then merges the newest **stable** release over it ([`stage-site.sh:42,70-100`](../../scripts/release/stage-site.sh)). It adds `version`, `tag`, `release_url`, `checksums_url`, `signature_url`, `signing_key_url`, `signing_namespace`, `installers` and a `binaries` map from platform to URL.
  - Pre-releases are published under `/releases/` but never become `latest`.
  - Before any release, `latest.json` has a `version` but no `tag`. [`smoke-published.sh:46-54`](../../scripts/release/smoke-published.sh) handles that case, so **an update checker must key on `tag`, not `version`**.
  - **`latest.json` is not signed**: nothing in `stage-site.sh` signs it.
- **A post-deploy smoke test** runs the real `curl | sh -s -- --json` into a scratch home. It checks `ok`, both `*_verified` fields, the version, and `oskac --version --format=json` ([`smoke-published.sh:61-85`](../../scripts/release/smoke-published.sh), [`pages.yml:89-92`](../../.github/workflows/pages.yml)).
- **CI tests the installers** on Linux, macOS and Windows (PowerShell 5.1 and 7) against fixtures signed with throwaway keys ([`.github/workflows/installers.yml:1-45`](../../.github/workflows/installers.yml)).

### 3. Verification, and rollback protection as built

The installer does the following, in order:

1. It refuses to run without a pinned `ssh-ed25519` key (`no_signing_key`) ([`install.sh:191-194`](../../scripts/install/install.sh)). The key is substituted at site build time by [`pin.sh`](../../scripts/install/pin.sh) (`pin.sh:6-9,28-38`).
2. It downloads over HTTPS only, refusing `http` redirects (`--proto '=https' --proto-redir '=https' --tlsv1.2`) ([`install.sh:95-106`](../../scripts/install/install.sh)).
3. It checks the SSH signature of `SHA256SUMS` in namespace `oska-release` against the pinned key (`bad_signature`) ([`install.sh:241-244`](../../scripts/install/install.sh)).
4. It requires the signed first line to name the requested tag (`version_mismatch`) ([`install.sh:246-255`](../../scripts/install/install.sh)).
5. It checks that the SHA-256 is listed exactly once and matches (`checksum_mismatch`) ([`install.sh:257-262`](../../scripts/install/install.sh)).
6. It runs the staged binary and requires `oskac <version>` ([`install.sh:283-285`](../../scripts/install/install.sh)).
7. It renames the binary into place atomically from a `mktemp -d` staging directory next to the target, so a planted symlink is never written through ([`install.sh:273-286`](../../scripts/install/install.sh)).

The tag line defeats one kind of rollback: an old signed release served under a newer tag's path. CI tests it ([`scripts/install/test.sh:276-292`](../../scripts/install/test.sh), [`scripts/release/test.sh:130-140`](../../scripts/release/test.sh)).

**It does not defeat a rollback or freeze through `latest.json`.** With no `--version`, the tag comes from the unsigned `latest.json` ([`install.sh:221-230`](../../scripts/install/install.sh)). If `latest.json` names an older genuine tag, the signature, tag line and checksum all pass, because the files really are that release. Nothing compares the result with the installed version or with any freshness date. *Inference from the code:* a party able to change `latest.json` (a compromised Pages deploy, or the GitHub Release source before re-verification) can hold users on an old, possibly vulnerable, release. TUF calls these the rollback and indefinite-freeze attacks ([TUF spec §1.5.2](https://theupdateframework.github.io/specification/latest/)).

### 4. How users learn about a new release, and how they upgrade

- **Finding out:** nothing in `oskac` checks for releases. Its commands are `lex`, `parse`, `check`, `run`, `test`, `build`, `explain`, `--version` and `--help` ([`crates/oskac/src/main.rs:26-54,123-159`](../../crates/oskac/src/main.rs)). None makes a network request. Users can poll `https://oskalang.dev/latest.json` by hand ([`docs/agents/release.md:45`](../agents/release.md)). Every docs page carries a version banner that points at it ([`site/build.osk:704`](../../site/build.osk)).
- **Upgrading:** "Re-run the installer… There is no self-update command" ([`install.md:97-99`](../guide/install.md)). The agent-facing files say the same ([`site/agent-setup/AGENTS.template.md:32`](../../site/agent-setup/AGENTS.template.md), [`prompt.md:71`](../../site/agent-setup/prompt.md)).
- **What `action` and `previous_version` mean:**
  - The installer reads the first line of `$OSKA_HOME/version` as `previous`, but only when both `bin/oskac` and that file exist ([`install.sh:264-271`](../../scripts/install/install.sh)).
  - It then sets `action` ([`install.sh:290-296`](../../scripts/install/install.sh)):
    - `installed` when there was no previous version
    - `reinstalled` when the previous version equals the new one
    - `updated` otherwise, **including a downgrade** such as `--version 0.1.0` over 0.3.0
  - [`install.md:67`](../guide/install.md) documents only the three values.
  - A hand-built `oskac` copied into `~/.oska/bin` with no `version` file reports `installed`.
- **Pinning:** `--version` installs exactly that tag, pre-releases included (`is_tag` accepts `-pre`) ([`install.sh:118-120,203-209`](../../scripts/install/install.sh)). There is no per-project pin: `oska.toml` accepts only `[package]` `name` and `version`, and any other key is `E0402` ([`crates/oska-check/src/manifest.rs:18,499-507`](../../crates/oska-check/src/manifest.rs)).

### 5. Uninstall, verified

- **The documented step:** "Delete `~/.oska`, which holds `bin/oskac` and a `version` file" ([`install.md:101-103`](../guide/install.md)).
- **What the installer actually writes:**
  - `$OSKA_HOME/bin/oskac` (`oskac.exe` on Windows) and `$OSKA_HOME/version` ([`install.sh:264-287`](../../scripts/install/install.sh), [`install.ps1:264-303`](../../scripts/install/install.ps1)).
  - Transient `bin/.oskac.XXXXXXXX/`. `install.sh` has no `trap`, so a run interrupted by a signal can leave this staging directory and a temp directory behind (no `trap` in the file).
  - On Windows, possibly `bin/oskac.exe.old`. A running `oskac.exe` is renamed aside, and its deletion is `SilentlyContinue` ([`install.ps1:294-301`](../../scripts/install/install.ps1)).
  - The installer does not write to the PATH or to shell profiles ([`install.md:44`](../guide/install.md)).
- **So deleting `~/.oska` is correct for a default install, but incomplete:**
  1. A custom `OSKA_HOME` is not mentioned.
  2. The `export PATH=…` line the installer told the user to add ([`install.sh:320-325`](../../scripts/install/install.sh)) is left behind. On Windows, the user-PATH entry it suggested is left too ([`install.ps1:339-343`](../../scripts/install/install.ps1)).
  3. Future state lives elsewhere: package credentials go in `~/.config/oska/credentials.toml` ([ADR 0010](../adr/0010-packages-are-git-urls-resolved-by-minimal-version-selection.md)), and a fetch cache is planned ([ADR 0012](../adr/0012-tls-git-and-crypto-are-written-in-oska.md)).
  4. **Agent-setup leftovers break.** The opt-in hook falls back to a bare `oskac` when `~/.oska/bin/oskac` is gone. A failed check exits 2, which Claude Code shows as an error ([`site/agent-setup/oska-check.sh:21-32`](../../site/agent-setup/oska-check.sh)). After an uninstall, every `.osk` edit then reports "oskac: not found" as a blocking error. *Inference from the code.*
  5. The `AGENTS.md` section, the CI step and `.claude/settings.json` edits remain, and nothing lists them.

### 6. PATH and Windows

- **PATH:**
  - Both installers only detect whether `bin_dir` is on PATH (`on_path`) and print the line to add ([`install.sh:297-300,320-325`](../../scripts/install/install.sh), [`install.ps1:310-313,339-343`](../../scripts/install/install.ps1)).
  - The agent prompt forbids editing shell profiles unless asked, and says to run `oskac` by its full `path` ([`prompt.md:34`](../../site/agent-setup/prompt.md)).
- **Windows:**
  - The installer needs the OpenSSH Client's `ssh-keygen` ([`install.ps1:193-197`](../../scripts/install/install.ps1)).
  - Windows on ARM gets the x86_64 build under emulation ([`install.ps1:186-191`](../../scripts/install/install.ps1)).
  - The staging file has a fixed name, `.oskac.new.exe`, unlike the unpredictable `mktemp` directory in `install.sh` ([`install.ps1:276`](../../scripts/install/install.ps1)).
  - Under `iex`, the installer throws instead of exiting so that it does not close the window ([`install.ps1:360-367`](../../scripts/install/install.ps1)).
  - The agent hook is POSIX-only and has no Windows version yet (issue #87, [`prompt.md:56`](../../site/agent-setup/prompt.md)).

### 7. What is machine-readable today

- **Installer success JSON:** `ok`, `action`, `version`, `tag`, `previous_version`, `platform`, `path`, `bin_dir`, `on_path`, `checksum_verified`, `signature_verified` ([`install.sh:302-311`](../../scripts/install/install.sh), [`install.ps1:315-330`](../../scripts/install/install.ps1)).
- **Installer failure JSON:** `{"ok": false, "error", "message"}`, with exit 2 for `usage` and 1 otherwise. There are 11 stable error codes ([`install.sh:54-69`](../../scripts/install/install.sh), [`install.md:71-85`](../guide/install.md)).
- **`oskac --version --format=json`** prints only `{"name":"oskac","version":"…"}` ([`main.rs:124-134`](../../crates/oskac/src/main.rs), [`docs/guide/cli-reference.md:147`](../guide/cli-reference.md)). It has no platform, commit, install location or receipt.
- **An unknown command or flag** prints usage on stderr and exits 2, even with `--format=json` ([`main.rs:155-158`](../../crates/oskac/src/main.rs), [`cli-reference.md:40`](../guide/cli-reference.md)). So `oskac self update` today is a usage error, not a JSON error.
- **`latest.json`** is machine-readable but unsigned (§2).

### 8. Signing-key rotation

Every downloaded installer pins one key. After a rotation, old copies refuse new releases and users must re-download the installer ([`docs/agents/release.md:60`](../agents/release.md)). Old releases must be re-signed or deleted, or the Pages build stops ([`release.md:70`](../agents/release.md)). Any updater that pins a key inherits this. There is no signed "key B replaces key A" statement.

### Gaps and inconsistencies, in brief

| # | Gap | Evidence |
| --- | --- | --- |
| G1 | No way to learn that a release exists, short of fetching `latest.json` | `main.rs:123-159`, `install.md:99` |
| G2 | Unsigned `latest.json` chooses the tag, so a freeze or rollback to an older genuine release passes every check | `install.sh:221-230`, `stage-site.sh:76-100` |
| G3 | A downgrade reports `action: "updated"`, and nothing guards against one | `install.sh:290-296`, `install.md:67` |
| G4 | The receipt is one line (version only): no platform, source, key fingerprint, digest or file list | `install.sh:267,287` |
| G5 | Uninstall is manual and omits a custom `OSKA_HOME`, the PATH line and agent-setup leftovers. The hook then fails on every edit | `install.md:101-103`, `oska-check.sh:21-32` |
| G6 | No project-level toolchain requirement | `manifest.rs:499-507` |
| G7 | `--version --format=json` is minimal, and an unknown command is a non-JSON usage error | `main.rs:124-158` |
| G8 | `install.sh` leaves staging and temp directories on a signal (no `trap`). `install.ps1` uses a fixed staging name and may leave `oskac.exe.old` | `install.sh:45-52,278`, `install.ps1:276,294-301` |
| G9 | Key rotation strands every installed copy that pins the key | `release.md:60` |
| G10 | Pre-releases are installable by `--version` but have no channel in `latest.json` | `stage-site.sh:42,70` |

## Part 2: prior art

### Self-updating toolchains

- **rustup:**
  - It has `rustup check` ("Check for updates to Rust toolchains and rustup"), `self update` ("Download and install updates to rustup") and `self uninstall` with `-y` ("Disable confirmation prompt") ([`rustup_mode.rs`](https://raw.githubusercontent.com/rust-lang/rustup/master/src/cli/rustup_mode.rs)).
  - An `auto-self-update` setting takes `enable`, `disable` or `check-only`, and builds made without the feature "can not update itself" ([rustup basics](https://rust-lang.github.io/rustup/basics.html)).
  - **Uninstall refuses to run unless `bin/rustup` exists in `CARGO_HOME`.** It removes proxies only if they are the same file as `rustup`, removes directories only when empty ("keeping non-empty cargo bin directory"), and touches PATH only if it removed the bin directory ([`self_update.rs`](https://raw.githubusercontent.com/rust-lang/rustup/master/src/cli/self_update.rs)).
  - On Windows, the running binary is removed after the process exits ([`self_update.rs`](https://raw.githubusercontent.com/rust-lang/rustup/master/src/cli/self_update.rs)).
  - The update is checked against a `.sha256` file when one exists, and falls back to an unverified download with a warning if it is missing ([`self_update.rs`](https://raw.githubusercontent.com/rust-lang/rustup/master/src/cli/self_update.rs)).
- **uv:**
  - "When uv is installed via the standalone installer, it can update itself… When another installation method is used, self-updates are disabled" ([uv installation](https://docs.astral.sh/uv/getting-started/installation/)).
  - The decision rests on a receipt file, `uv-receipt.json`, under `~/.config/uv` or `%LOCALAPPDATA%\uv`. It is checked against the running executable. With no receipt, uv says "Self-update is only available for uv binaries installed via the standalone installation scripts" ([`self_update.rs`](https://raw.githubusercontent.com/astral-sh/uv/main/crates/uv/src/commands/self_update.rs)).
  - It has `--dry-run` and an exact-version target. It updates by downloading and **re-running the installer script**, with no signature check in that code path ([`self_update.rs`](https://raw.githubusercontent.com/astral-sh/uv/main/crates/uv/src/commands/self_update.rs)).
  - Uninstall is a manual list of files and directories ([uv installation](https://docs.astral.sh/uv/getting-started/installation/)).
- **Deno:**
  - `deno upgrade [version|lts|rc|canary]` has `--dry-run` ("Perform all checks without replacing old exe"), `--force`, `--output` and `--checksum` (a SHA-256 the user supplies) ([deno upgrade](https://docs.deno.com/runtime/reference/cli/upgrade/)).
  - A background update notice exists, and `DENO_NO_UPDATE_CHECK` turns it off ([Deno env vars](https://docs.deno.com/runtime/reference/env_variables/)).
- **Bun:**
  - "The binary can upgrade itself" with `bun upgrade [--canary|--stable]`. Homebrew and Scoop users are told to use their package manager instead "to avoid conflicts" ([Bun installation](https://bun.com/docs/installation)).
  - A specific version is installed by re-running the installer. Uninstall is `rm -rf ~/.bun`, or `~\.bun\uninstall.ps1` on Windows ([Bun installation](https://bun.com/docs/installation)).
- **Go:**
  - "The `go` line declares the minimum required Go version"; "The Go toolchain refuses to load a module or workspace that declares a minimum required Go version greater than the toolchain's own version." `GOTOOLCHAIN=auto` downloads the toolchain; `local` never switches ([Go toolchains](https://go.dev/doc/toolchain)).
  - Downloaded toolchains are checked against the Go checksum database, and "toolchain downloads fail for lack of verification if `GOSUMDB=off`" ([Go toolchains](https://go.dev/doc/toolchain)).

### Package-manager manifests

- **Homebrew:** a formula pins `url` and `sha256` and has a `test do` block; `livecheck` finds new versions ([Formula Cookbook](https://docs.brew.sh/Formula-Cookbook)).
- **Scoop:** a manifest has `version`, `url`, `hash` (SHA-256 by default), `bin`, `checkver` and `autoupdate` ([Scoop App Manifests](https://github.com/ScoopInstaller/Scoop/wiki/App-Manifests)).
- **winget:**
  - A manifest is YAML with `InstallerUrl` and `InstallerSha256`, submitted as a pull request to `microsoft/winget-pkgs` ([winget manifest](https://learn.microsoft.com/en-us/windows/package-manager/package/manifest)).
  - Portable packages are supported, and `PortableCommandAlias` is "added to the PATH" ([installer schema](https://github.com/microsoft/winget-pkgs/blob/master/doc/manifest/schema/1.12.0/installer.md)).
- **What all three share:** they verify a hash that the manifest repository holds, not the publisher's signature.

### Update security

- **TUF:**
  - **Version numbers:** clients "MUST NOT replace a metadata file with a version number less than the one currently trusted" (§4.3), and on a lower timestamp version they "abort the update cycle, and report the potential rollback attack" (§5.4) ([TUF spec](https://theupdateframework.github.io/specification/latest/)).
  - **Expiry** bounds how long an attacker can freeze a client ([TUF spec](https://theupdateframework.github.io/specification/latest/)).
  - **Key rotation:** a new root must be signed by a threshold of both the old and the new root keys, and clients walk the chain version by version (§5.3, §6.1) ([TUF spec](https://theupdateframework.github.io/specification/latest/)).
- **minisign:** signatures carry a signed "trusted comment" for metadata "such as the intended file name, timestamps", and versions help prevent downgrade attacks ([minisign](https://jedisct1.github.io/minisign/)). Oska's signed `# oska-release vX.Y.Z` line is the same idea, carried in the signed file.
- **OpenSSH `ssh-keygen -Y`:**
  - The namespace (`-n`) is "used to prevent signature confusion across different domains of use" ([ssh-keygen(1)](https://man.openbsd.org/ssh-keygen.1)).
  - The allowed-signers format supports `valid-after`, `valid-before` and `cert-authority` ([ssh-keygen(1)](https://man.openbsd.org/ssh-keygen.1)). These give a native way to overlap two keys during a rotation.

## Part 2: candidate features

The Oska constraints that apply throughout:

- **No external crates.** Rust `std` has no TLS or ed25519 ([ADR 0009](../adr/0009-oska-sys-is-the-only-crate-with-unsafe.md)).
- **`unsafe` only in `oska-sys`** ([ADR 0009](../adr/0009-oska-sys-is-the-only-crate-with-unsafe.md)).
- **TLS, git and crypto are to be written in Oska** ([ADR 0012](../adr/0012-tls-git-and-crypto-are-written-in-oska.md)). `Net` is not implemented yet ([`docs/design.md:423`](../design.md)). Shelling out to the system's git was "Rejected by the maintainer" for packages (ADR 0012).
- **No ambient authority** (GLOSSARY: *Ambient authority*). `oskac` is a Rust binary outside the checker, but an updater needs network and file-write **authority**. In Oska terms that is `NetConnect` plus a **directory capability** confined to `$OSKA_HOME`.
- **The receipt is the right root of trust for "what to remove".**

### F1. Installer `--check` (and `--dry-run`)

- **What it does:** downloads `latest.json` (or the signed index of F4) and compares it with `$OSKA_HOME/version`. It prints `{"ok": true, "installed": "0.3.0", "latest": "0.4.0", "update_available": true, "tag": "v0.4.0"}` and installs nothing.
- **Fit:** pure shell and PowerShell, no `oskac` change. It mirrors `rustup check` and `deno upgrade --dry-run`.
- **Risk:** low. It reports the unsigned `latest.json` until F4 lands.
- **Effort:** S.

### F2. JSON install receipt plus a downgrade guard

- **What it does:** replaces `$OSKA_HOME/version` with `$OSKA_HOME/receipt.json`, keeping `version` for one release for compatibility. The receipt holds:
  - `version`, `tag`, `platform`
  - `installed_at`, `site`
  - `key_fingerprint`
  - the binary's `sha256`
  - `files` (every path the installer wrote)
  - `installer` (`sh` or `ps1`)
- **Downgrade guard:**
  - The installer refuses to go from a newer to an older version unless `--version` names it explicitly. A new error code, `downgrade_refused`, covers the case where `latest.json` names an older tag than the receipt.
  - An explicit downgrade reports a new `action: "downgraded"`.
  - It also records a `trap`/`finally` cleanup (G8).
- **Fit:** uv bases self-update on a receipt; rustup checks that it is "self-installed" before uninstalling. TUF §4.3 says never accept an older version than the one trusted.
- **Risk:** the receipt is user-writable, which is fine because it is advisory. Verification still rests on the signature.
- **Effort:** S–M (both installers and both test suites).

### F3. Installer `--uninstall`, and later `oskac self uninstall`

- **What it does:**
  - Removes only the files listed in the receipt, then `bin/` and `$OSKA_HOME` if they are empty, as rustup does.
  - Refuses (`not_installed`) if there is no receipt or the binary's digest does not match it.
  - Reports in JSON: `{"ok": true, "action": "uninstalled", "removed": […], "kept": […], "path_hint": "…"}`.
  - Never edits profiles, which matches the prompt rule at [`prompt.md:34`](../../site/agent-setup/prompt.md). It prints the PATH line to remove.
  - Leaves `~/.config/oska` alone unless `--purge` is given.
- **On Windows:** the PowerShell form can delete the binary, since `oskac.exe` is not the running process. An `oskac self uninstall` would need rustup's deferred delete.
- **Also fix the hook:** `oska-check.sh` should exit 0 with a note when no `oskac` is found, so an uninstall does not block every edit (G5).
- **Risk:** a deletion steered by `OSKA_HOME` or a planted receipt. Mitigations:
  - list files, never `rm -rf $OSKA_HOME`
  - refuse paths outside `$OSKA_HOME`
  - refuse symlinks, as the directory capabilities do (GLOSSARY: *Directory capability*)
- **Effort:** S–M.

### F4. A signed, expiring release index (TUF-lite)

- **What it does:** the release pipeline signs a small index with the release key in a new namespace, `oska-index`. The index has `serial` (monotonic), `expires` (for example 30 days), `latest` (tag) and optionally `channels`.
  - The installer and any `oskac self` command verify the index with `ssh-keygen -Y verify`, refuse an expired index (`stale_index`), and refuse a `serial` below the one in the receipt (`rollback_refused`).
  - The `pages.yml` deploy then re-signs it on a schedule.
- **Fit:** it closes G2 with the tools already required (OpenSSH 8.1). The `-n` namespace keeps index signatures from being confused with release signatures.
- **Risk:** expiry needs a scheduled re-sign. Today the key is only reachable from the tag-gated `release` environment ([`release.md:23-25`](../agents/release.md)), so a scheduled job either needs a separate online "timestamp" key or the expiry must be long. TUF separates exactly these roles: offline root, online timestamp. Pinning a second key widens what the installer trusts. *Inference.*
- **Effort:** M.

### F5. `oskac self` command family (thin)

| Command | What it does |
| --- | --- |
| `oskac self info --format=json` | Reports the version, platform, commit and executable path, the receipt (if any) and whether the binary matches it. |
| `oskac self check --format=json` | Same output as F1. |
| `oskac self uninstall [--purge] [--format=json]` | F3, done in-process with `std::fs`. |
| `oskac self update [--version X] [--check] [--format=json]` | Delegates to the installer. |

- **Fit for `self info`, `self check` and `self uninstall`:** these need no network beyond `self check`.
- **How `self update` delegates:** the pinned installer is not a release artifact, so `oskac` cannot verify a downloaded `install.sh`. The safer delegation is either:
  - `oskac` prints the exact installer command for the agent to run (zero new authority), or
  - `oskac` ships the installer text embedded in its own binary and runs it with `sh`/`pwsh`. That shells out to `curl` and `ssh-keygen`, which ADR 0012 rejected for git. *Needs a maintainer decision.*
- **Self-disable:** `self update` refuses when there is no receipt, as uv does, so Homebrew, Scoop and source builds are not overwritten.
- **Exit codes:** keep 0, 1 and 2. Report `update_available` in JSON rather than as an exit code, so `check` is not mistaken for a failure.
- **Also:** extend `--version --format=json` with `platform` and `commit`, and make an unknown command under `--format=json` print a JSON error (G7).
- **Risk:** low for `info` and `uninstall`. For `check`, it is the first network use by `oskac`, so it should only run when invoked.
- **Effort:** M.

### F6. A native self-updater written in Oska (later)

- **What it does:** once the Oska TLS client exists, `oskac self update` runs a toolchain-shipped Oska program. Its signature declares `NetConnect` (attenuated to `oskalang.dev`) and a `DirWrite` confined to `$OSKA_HOME/bin`. It verifies SSHSIG and ed25519 in Oska, and its authority report is published, as the site generator's is ([`pages.yml:31-33`](../../.github/workflows/pages.yml)).
- **Fit:** it is the ADR 0012 direction, and it dogfoods capabilities on security-critical code.
- **Risk:** hand-written, unaudited crypto (ADR 0012, Consequences). A bug here installs arbitrary code. Mitigation: keep the installer path as a second, independent verifier (CI cross-check), and run the published Wycheproof vectors on ed25519.
- **Effort:** L, and blocked on milestone 4 work ([`docs/design.md:475`](../design.md)).

### F7. An update-available notice

- **What it does:** a one-line stderr notice from `check`, `run` or `test` when a newer release exists.
- **Fit:** poor as a default. It is network use the user did not ask for, from a compiler whose `explain` is advertised as offline ([`main.rs:51`](../../crates/oskac/src/main.rs)). It also adds noise to agent stderr.
- **Recommendation:** make it opt-in (`OSKA_UPDATE_CHECK=1`, or a setting in `~/.config/oska/`). Or never have `oskac` make the network request: only compare against a cached index that `self check` writes. Prior art is opt-out: `DENO_NO_UPDATE_CHECK`, and rustup's `check-only`.
- **Risk:** privacy, CI flakiness, agent confusion.
- **Effort:** S, once F5 exists.

### F8. Per-project toolchain requirement in `oska.toml`

- **What it does:** a key such as `[package] oska = "0.3"` (a minimum, Go-style).
  - `oskac` refuses a package needing a newer toolchain, with a new stable code that says what to install. Today it would say `E0402` "unknown key", which hides the reason.
  - The agent prompt and CI read the key to pin `--version`.
  - An auto-download (`GOTOOLCHAIN=auto`) is out of scope until F6. Go makes it safe only by checking a checksum database.
- **Fit:** strong for agents. The manifest is the agreed place for a package's identity (GLOSSARY: *Manifest*), and minimal version selection ([ADR 0010](../adr/0010-packages-are-git-urls-resolved-by-minimal-version-selection.md)) already thinks in minimums.
- **Risk:** the manifest-format change needs an ADR amendment.
- **Effort:** S–M.

### F9. Rollback to the previous binary

- **What it does:** keep `bin/oskac.prev` plus a receipt entry, and offer `--rollback` or `oskac self rollback`. Or simply document `--version <previous_version>`, which works today.
- **Fit:** the explicit `--version` already gives a verified rollback. A local copy only saves a download.
- **Risk:** a stale `prev` binary bypasses the downgrade guard by design, so it should be reported in JSON.
- **Effort:** S. Low value while pre-alpha.

### F10. Package-manager manifests (Homebrew tap, Scoop bucket, winget)

- **What it does:** the release pipeline generates manifests from `SHA256SUMS`.
- **Fit:** familiar to humans, and winget and Scoop handle PATH, which the installers deliberately do not.
- **Risk:** they verify only the hash in a third-party repository, not the Oska signature, and they need their own update path (Bun and uv defer to them). A pre-alpha with breaking releases ([`install.md:114-116`](../guide/install.md)) gains little.
- **Effort:** M plus ongoing maintenance. Defer.

### F11. Agent-setup changes

- **The prompt:**
  - Add "update" and "uninstall" steps that use `--check` and `--uninstall` and stop on `ok != true`.
  - Have it report `action` and `previous_version` (including the new `downgraded`).
  - Have it record the installed version in `oska.toml` (F8), or in the CI pin it already writes ([`prompt.md:63`](../../site/agent-setup/prompt.md)).
- **The AGENTS template:** add `oskac self check --format=json` once F5 exists ([`AGENTS.template.md:32`](../../site/agent-setup/AGENTS.template.md)).
- **The hook:** tolerate a missing `oskac` (F3).
- **Effort:** S.

### F12. Key rotation without stranding installers

- **What it does:** publish a rotation statement, signed by the old key, that names the new key. Installers and `oskac` accept a new key only through it, which is TUF's chained root.
  - A simpler alternative: the allowed-signers file pins both keys with `valid-before` and `valid-after`.
- **Fit:** fixes G9 with tools already in use.
- **Effort:** M. Do it before the first real rotation.

## Recommendation (ranked)

1. **F2 + F3 + F11: receipt, downgrade guard, `--uninstall`, hook fix, prompt update.** These are the highest value per effort and change only the installers and the site's agent-setup files. They close G3, G4, G5 and G8, and make uninstall safe for agents (JSON, no prompts, deletes only what the receipt lists).
2. **F4 + F1: signed, expiring index plus `--check`.** This closes the freeze and rollback gap (G2) and gives agents a verified "is there an update?" answer (G1), using only OpenSSH. The scheduling and key-role question below must be decided first.
3. **F5 + F8: thin `oskac self info|check|uninstall` and a project toolchain requirement.** Both are agent-facing, with JSON and stable codes, and need no in-process TLS. `self update` stays a delegation to the installer until **F6**, the Oska-written updater, can be built on the ADR 0012 TLS stack.

Defer F7 (opt-in only, if ever), F9 (already possible with `--version`), F10 (until beta) and F12 (before the first rotation).

## Open questions

1. Should `oskac` ever make a network request on its own (F5 `self check`, F7 notice), or should every network use stay in the installer until the Oska `NetConnect` runtime exists?
2. For F4 freshness, is an online signing key acceptable (a timestamp role, held by a scheduled job), or should the index be signed only at release time with a long expiry, accepting a weaker freeze bound?
3. Should `oskac self update` shell out to `curl` and `ssh-keygen` through an embedded installer? ADR 0012 rejected shelling out for git; does that rule cover the updater?
4. Should a downgrade through `latest.json` be a hard error (`downgrade_refused`), or a warning with `action: "downgraded"`?
5. Where should the receipt live: `$OSKA_HOME/receipt.json`, next to the binary, or `~/.config/oska/` as uv does? This also decides whether uninstall touches `~/.config/oska`.
6. What should the `oska.toml` toolchain key be called (`oska`, `toolchain`, or `oska-version`), and is it a minimum or an exact pin?
7. Are package-manager channels (Homebrew, Scoop, winget) wanted at all before 1.0?

## All sources fetched

| Source | Quality | Topic |
| --- | --- | --- |
| [rustup basics](https://rust-lang.github.io/rustup/basics.html) | primary | self update, auto-self-update |
| [rustup installation](https://rust-lang.github.io/rustup/installation/index.html) | primary | self uninstall |
| [rustup `rustup_mode.rs`](https://raw.githubusercontent.com/rust-lang/rustup/master/src/cli/rustup_mode.rs) | primary (source) | check, self subcommands |
| [rustup `self_update.rs`](https://raw.githubusercontent.com/rust-lang/rustup/master/src/cli/self_update.rs) | primary (source) | uninstall safety, update replacement |
| [uv installation](https://docs.astral.sh/uv/getting-started/installation/) | primary | standalone installer, self update, uninstall |
| [uv `self_update.rs`](https://raw.githubusercontent.com/astral-sh/uv/main/crates/uv/src/commands/self_update.rs) | primary (source) | receipt, dry-run, no-receipt message |
| [deno upgrade](https://docs.deno.com/runtime/reference/cli/upgrade/) | primary | upgrade flags |
| [Deno environment variables](https://docs.deno.com/runtime/reference/env_variables/) | primary | `DENO_NO_UPDATE_CHECK` |
| [Bun installation](https://bun.com/docs/installation) | primary | upgrade, uninstall |
| [Go toolchains](https://go.dev/doc/toolchain) | primary | go/toolchain lines, `GOTOOLCHAIN`, checksum DB |
| [Homebrew Formula Cookbook](https://docs.brew.sh/Formula-Cookbook) | primary | formula fields |
| [Scoop App Manifests](https://github.com/ScoopInstaller/Scoop/wiki/App-Manifests) | primary | manifest fields |
| [winget manifest](https://learn.microsoft.com/en-us/windows/package-manager/package/manifest) | primary | manifest, hash |
| [winget installer schema 1.12.0](https://github.com/microsoft/winget-pkgs/blob/master/doc/manifest/schema/1.12.0/installer.md) | primary | portable, PATH alias |
| [The Update Framework specification](https://theupdateframework.github.io/specification/latest/) | primary | rollback, freeze, roles, rotation |
| [minisign](https://jedisct1.github.io/minisign/) | primary | trusted comments, downgrade |
| [ssh-keygen(1)](https://man.openbsd.org/ssh-keygen.1) | primary | `-Y sign/verify`, namespaces, validity |
