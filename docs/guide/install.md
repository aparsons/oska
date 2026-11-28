# Install

Oska is pre-alpha. The latest release is **v0.6.0**. You can install `oskac`, the compiler and runner, from a signed binary with a one-line installer, or [build it from source](#build-from-source).

## One-line installers

On Linux and macOS:

```sh
curl -fsSL https://oskalang.dev/install.sh | sh
```

On Windows, in PowerShell:

```powershell
irm https://oskalang.dev/install.ps1 | iex
```

What the installers do:

- **They never prompt.** They are safe to run from a script, a CI job, or an AI agent.
- **They install to `~/.oska/bin`.** Set `OSKA_HOME` to install into `$OSKA_HOME/bin` instead. They don't change your `PATH`; if `~/.oska/bin` is not on it, they print the line to add.
- **They verify before they install.** The installer checks the SSH signature on `SHA256SUMS` (`ssh-keygen -Y verify`, so it needs OpenSSH 8.1 or newer) against the release key built into the installer. The signed file's first line, `# oska-release v0.6.0`, names the release, and it must be the one the installer asked for, so an old release cannot be served as a newer one. Then the installer checks the download's SHA-256 checksum against `SHA256SUMS`, and checks that the binary reports the expected version. If any check fails, nothing is installed and the installer exits with a non-zero status.
- **They take the latest release from a signed index.** Without `--version`, the installer reads `release-index.json`, which names the latest release and is signed by the release key in its own namespace, `oska-index`. It refuses an index that is not signed by that key (`bad_index`), one that has expired (`stale_index`), and one older than the last index this install accepted (`rollback_refused`). It warns when the index expires within 30 days. `--version` does not read the index. Until the site publishes its first index, the installer reads the unsigned `latest.json` instead and reports `index_verified: false`.
- **They download over HTTPS only.** They refuse a redirect to a plain `http://` URL.
- **They never downgrade by accident.** If the release they would install is older than the installed `oskac`, they stop with `downgrade_refused` and install nothing. To go back to an older release, name it with `--version` and add `--allow-downgrade`.
- **They record what they installed.** See [the install receipt](#the-install-receipt).
- **They clean up after themselves.** If an install is interrupted, for example with Ctrl-C or because a CI job is cancelled, the installer removes its temporary and staging files before it exits.
- **They take `--version`, `--allow-downgrade`, `--check`, `--uninstall`, `--add-to-path`, `--hook` and `--json`.** `--version` installs that release instead of the latest. `--allow-downgrade`, which needs `--version`, lets that release be older than the installed one. `--check` reports the installed and the latest version and installs nothing (see [Check for an update](#check-for-an-update)). `--uninstall` removes what the installer installed (see [Uninstall](#uninstall)). `--add-to-path` and `--hook` are opt-in (see [Put oskac on your PATH, and install the agent hook](#put-oskac-on-your-path-and-install-the-agent-hook)). `--json` prints one line of JSON on stdout and nothing else.

To pass options through the pipe:

```sh
curl -fsSL https://oskalang.dev/install.sh | sh -s -- --version 0.6.0 --json
```

`iex` cannot pass options, so in PowerShell run the script as a script block:

```powershell
& ([scriptblock]::Create((irm https://oskalang.dev/install.ps1))) --version 0.6.0 --json
```

A successful `--json` run prints:

```json
{"ok": true, "action": "installed", "version": "0.6.0", "tag": "v0.6.0", "previous_version": null, "platform": "linux-x86_64", "path": "/home/you/.oska/bin/oskac", "bin_dir": "/home/you/.oska/bin", "on_path": false, "checksum_verified": true, "signature_verified": true, "receipt": "/home/you/.oska/receipt.json", "index_verified": true, "index_serial": 2}
```

- `action` is one of:
  - `installed`: there was no `oskac` installed;
  - `updated`: the old version, in `previous_version`, was lower;
  - `reinstalled`: the same version was installed;
  - `downgraded`: the old version, in `previous_version`, was higher, and the run had `--version` and `--allow-downgrade`.

  Versions are compared by [semver](https://semver.org) precedence, so `0.4.0-pre.1` is lower than `0.4.0` and higher than `0.3.0`.
- `receipt` is the path of the [install receipt](#the-install-receipt).
- `index_verified` is `true` when the release came from the verified, signed index, and `index_serial` is that index's serial. With `--version`, or before the site publishes an index, they are `false` and `null`.
- With `--add-to-path`, `profile` (sh) is the profile file the installer added a line to, or `user_path_entry` (PowerShell) is the entry it added to your user `PATH`. Neither is there when nothing was changed.
- With `--hook`, `hook_settings` is the settings file the installer created or `null`, and `settings_merge_needed` is `true` when the project already had settings that do not run the post-edit hook, so it needs merging in.
- `platform` is one of `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`, or `windows-x86_64`.
- On Windows, `path` ends in `oskac.exe`.

A failed run prints `{"ok": false, "error": "<code>", "message": "..."}` and exits non-zero: 2 for a usage error, 1 for anything else. The codes are:

| `error` | Meaning |
| --- | --- |
| `usage` | An unknown option, an `OSKA_LIBC` other than `glibc` or `musl`, a `--version` that is not a version, `--allow-downgrade` without `--version`, or options that cannot be used together, such as `--uninstall` with `--version`. |
| `no_signing_key` | The installer was not pinned when the site was built: it has no release key built in, or no valid setting for whether the release index is required. Download it again from oskalang.dev. |
| `insecure_url` | `OSKA_SITE_URL` is not an `https://` URL, or a download was redirected to one that is not. |
| `unsupported_platform` | There is no build for this OS or CPU, or `--hook` was used with `install.ps1`, which does not have it (the hook works on Windows: merge its settings as the agent-setup prompt does). |
| `unsupported_libc` | Linux only: `OSKA_LIBC=glibc` asks for the glibc build, and the installer did not find glibc 2.35 or newer; or the release has no musl build (v0.6.0 and earlier), and the system needs it or `OSKA_LIBC=musl` asks for it. |
| `missing_tool` | `curl`, `ssh-keygen`, or a SHA-256 tool is missing. |
| `download_failed` | A file could not be downloaded, for example because that version does not exist. |
| `bad_signature` | `SHA256SUMS` is not signed by the release key. |
| `checksum_mismatch` | The binary does not match `SHA256SUMS` (the installers and `oskac self update`). |
| `version_mismatch` | The signed `SHA256SUMS` is for another release than the one asked for, or the binary reports another version (the installers and `oskac self update`). |
| `downgrade_refused` | The release to install is older than the installed `oskac`, and the run did not ask for a downgrade with both `--version` and `--allow-downgrade` (the installers and `oskac self update`). Nothing was installed. |
| `bad_index` | The release index is not signed by the release key in the `oska-index` namespace, has no signature, or is not in the index format. |
| `stale_index` | The release index has expired. A named release still installs with `--version`. |
| `rollback_refused` | The release index's serial is lower than the one this install already accepted, recorded in the receipt. |
| `network_error` | `oskac self check` and `self update`: no route, DNS failure, timeout, refused connection, a redirect, a status other than 200, or a response past its size cap. |
| `tls_error` | `oskac self check` and `self update`: the TLS handshake or the site's certificate was refused (an untrusted or expired certificate, a wrong host, a bad handshake). A wrong system clock can cause it. |
| `bad_checksums` | `oskac self check` and `self update`: the release's `SHA256SUMS` is not signed by the release key in the `oska-release` namespace, is for another release, or is not in the checksum format. |
| `unmanaged_install` | `oskac self update`: this `oskac` is not a managed install (no receipt, a receipt for another path or version), so it is not replaced, and no connection was made. Re-run the installer or rebuild from source. |
| `no_previous_version` | `oskac self update --rollback`: the receipt records no previous version to go back to, or it is the version installed. Nothing was changed. |
| `replace_failed` | `oskac self update`: the verified binary could not be moved into place; the installed `oskac` is as it was. |
| `internal` | `oskac self check` and `self update`: the update checker could not be run or its answer was not understood. |
| `install_failed` | The binary or the receipt could not be written into place, or neither `OSKA_HOME` nor a home directory is set. |
| `not_installed` | `--uninstall` found no install receipt in the Oska home, or there is no Oska home because neither `OSKA_HOME` nor a home directory is set. Nothing was removed. |
| `unsafe_path` | `--uninstall` found a receipt entry outside the places the installer writes, in a `.config/oska` directory, reached through a symlink anywhere in its path, or containing `..`, or a receipt it cannot read. Nothing was removed. |

The binaries are published on oskalang.dev, together with `SHA256SUMS` and its signature, for Linux x86_64 and aarch64, macOS x86_64 and aarch64, and Windows x86_64. Linux has two builds of each release, the glibc build and the musl build: see [the two Linux builds](#the-two-linux-builds).

### The two Linux builds

From v0.7.0, each release has two `oskac` builds for each Linux processor ([ADR 0022](../adr/0022-linux-oskac-ships-as-glibc-and-musl-builds.md)). They replace the one `oskac-linux-<arch>` file of earlier releases (see the v0.7.0 notes in the [changelog](../../CHANGELOG.md), under Unreleased until v0.7.0 ships).

| Build | Release files | Runs on |
| --- | --- | --- |
| The glibc build | `oskac-linux-x86_64-gnu`, `oskac-linux-aarch64-gnu` | Linux with glibc 2.35 or newer, such as Ubuntu 22.04 or Debian 12. It is built on Ubuntu 22.04 and links glibc dynamically. |
| The musl build | `oskac-linux-x86_64-musl`, `oskac-linux-aarch64-musl` | Any Linux: a musl system such as Alpine, an older glibc such as Debian 11 or RHEL 8, or an empty container. It is a static executable, with no dynamic loader and no shared library. |

The two builds are the same `oskac` for the same Linux platform: the same commands, diagnostics, filesystem confinement and `--target` names, and an executable that `oskac build` writes is the same bytes from either. They differ in speed only. The musl build compiles about 1.6 times slower (0.32 s against 0.20 s for the site generator) and runs a program on the interpreter about 1.5 times slower, because musl's memory allocator is slower. An executable it builds runs at the same speed as one from the glibc build.

**Which one the installer picks.** `install.sh` installs the glibc build when it finds glibc 2.35 or newer, because it is the faster one where it runs. It installs the musl build otherwise, because that one runs everywhere: on musl systems such as Alpine, on an older glibc, and when it cannot tell which C library the system has. It never stops because of the C library unless you force the glibc build. Releases up to v0.6.0 have only the glibc build, as `oskac-linux-<arch>`: the installer installs it where it would choose the glibc build, and stops with `unsupported_libc` where it would choose the musl build.

**`OSKA_LIBC`.** Set `OSKA_LIBC=glibc` or `OSKA_LIBC=musl` to choose the build yourself. Set it for `sh`, not for `curl`:

```sh
curl -fsSL https://oskalang.dev/install.sh | OSKA_LIBC=musl sh
```

- `OSKA_LIBC=musl` installs the musl build on any Linux. Use it in CI for one `oskac` that behaves the same in every container.
- `OSKA_LIBC=glibc` installs the glibc build, and stops with `unsupported_libc` when the installer does not find glibc 2.35 or newer, so a forced choice never fails silently.
- Any other value is a `usage` error. That error, and `unsupported_libc` for `OSKA_LIBC=glibc`, come before anything is downloaded.
- A release up to v0.6.0 has no musl build. Where the installer would choose the musl build, or with `OSKA_LIBC=musl`, it stops with `unsupported_libc` once it has downloaded and verified that release's signed `SHA256SUMS`, which lists its builds, and before it downloads `oskac`.
- The installer does not remember `OSKA_LIBC`. Each run chooses the build again, an update included: from `OSKA_LIBC` when it is set, and from the system when it is not. Set `OSKA_LIBC` on each run to keep a build.

The [install receipt](#the-install-receipt) records the build installed in `linux_build` and whether `OSKA_LIBC` chose it in `libc_forced`; no run of the installer reads them back. `oskac self info` names the build of the running `oskac` (see [`oskac self`](cli-reference.md#oskac-self)).

### The install receipt

Each install writes `receipt.json` into the Oska home (`~/.oska`, or `OSKA_HOME`). It records what the installer put there:

```json
{
  "schema": 1,
  "version": "0.6.0",
  "tag": "v0.6.0",
  "platform": "linux-x86_64",
  "linux_build": "gnu",
  "libc_forced": false,
  "installed_at": "2026-11-12T09:30:00Z",
  "site": "https://oskalang.dev",
  "key_fingerprint": "SHA256:PjwTDycs560g1ktWC/gdznamSkzm4xH+OAdfntNaW4o",
  "sha256": "<SHA-256 of the installed oskac>",
  "installer": "sh",
  "index_serial": 0,
  "files": [
    {"path": "/home/you/.oska/bin/oskac", "kind": "binary", "sha256": "<SHA-256>"},
    {"path": "/home/you/.oska/version", "kind": "record", "sha256": "<SHA-256>"},
    {"path": "/home/you/.oska/receipt.json", "kind": "record"}
  ]
}
```

With `--add-to-path` or `--hook`, `files` also lists what they wrote outside the Oska home: a `profile_line` (the profile and the exact line), a `user_path_entry` (the exact `PATH` entry), a `hook_settings` (with its SHA-256, if the installer created it, and the `project` directory). A receipt from an earlier installer may also list a `hook_script`, the old shell hook, which `--uninstall` still removes if unchanged. A re-run without those options keeps the entries an earlier run recorded. [The install receipt](install-receipt.md) defines every kind of entry, where each may be, and what each uninstaller does with it.

- `key_fingerprint` is the fingerprint of the release key that vouched for the binary, and `sha256` is the binary's SHA-256, so you can tell whether it has changed since.
- `linux_build` (Linux only) is the build installed, `gnu` or `musl`, and `libc_forced` is `true` when `OSKA_LIBC` chose it. A receipt from an earlier installer has neither.
- `installer` is `sh` or `ps1`.
- `files` lists every file the installer wrote, with its SHA-256 where the content is the installer's own.
- The installer also still writes the one-line `version` file, for one more release, so that tools that read it keep working.

The receipt is advisory. The installer never trusts it to verify anything: every install checks the signature, the signed tag line, the checksum and the binary's version, whatever the receipt says. It only decides which version counts as installed, for the downgrade guard and `previous_version`, what `--uninstall` may remove, and records in `index_serial` the highest release index serial accepted, so that an older index is refused. When a receipt and a `version` file disagree, the receipt wins, but only while it describes the installed `oskac`: its `sha256` must be the binary's. A receipt that does not, because `oskac` was replaced by hand or an install was killed as it moved the files into place, is not believed, and the `version` file says what is installed. The installer writes the receipt last, after the binary and the `version` file, each by renaming a finished file into place, and the sh installer holds off Ctrl-C and other catchable signals until all three are in place. A re-run repairs a receipt left behind.

### Put oskac on your PATH, and install the agent hook

By default the installer touches nothing outside the Oska home. Two options change that, and only when you pass them. Uninstall undoes both.

- **`--add-to-path`** puts `oskac` on your `PATH`. `install.sh` appends one line to one profile file in your home directory, chosen from your login shell (`$SHELL`): `~/.zshrc` for zsh, `~/.bashrc` for bash (`~/.bash_profile` on macOS), and `~/.profile` for anything else. The line is `export PATH="/home/you/.oska/bin:$PATH" # added by the Oska installer`. It writes nothing if the `bin` directory is on your `PATH` already or the profile already holds that exact line, and it refuses a profile that is a symlink. `install.ps1` adds the `bin` directory to your user `PATH`, never the machine `PATH`. Open a new shell to use it.
- **`--hook <project dir>`** (`install.sh` only) installs the Claude Code check-after-edit hook into the project: it creates `.claude/settings.json` from the [post-edit settings](https://oskalang.dev/agent-setup/claude-settings-post-edit.json), a `PostToolUse` entry that runs `oskac hook claude --event=post-edit` (see [`oskac hook`](cli-reference.md)), only if the project has none. It never turns on the Stop gate, which is a separate choice: merge the `Stop` entry of the [published settings](https://oskalang.dev/agent-setup/claude-settings.json) for that. It writes no script, and it never edits existing settings: if they do not run the post-edit hook already, it reports `settings_merge_needed: true`, and you or your agent merge the `PostToolUse` entry, in place of any entry that runs the old `.claude/hooks/oska-check.sh`. It never touches `~/.claude`. With `install.ps1` it fails with `unsupported_platform`: on Windows, merge the settings by hand or with the agent-setup prompt.

Both belong to install alone, so they cannot be combined with `--check` or `--uninstall`.

### Verify a release by hand

The installers check every download for you. To check a release without them, compare the release key with its fingerprint, then check the signature, then the checksums. The release key is at https://oskalang.dev/oska-release.pub, and its fingerprint is:

```text
SHA256:PjwTDycs560g1ktWC/gdznamSkzm4xH+OAdfntNaW4o
```

The same fingerprint is on the [security page](../../SECURITY.md#verifying-a-release). On Linux, for v0.6.0:

```sh
base=https://oskalang.dev/releases/v0.6.0
asset=oskac-linux-x86_64
curl -fsSL --remote-name-all "$base/SHA256SUMS" "$base/SHA256SUMS.sig" "$base/$asset"
curl -fsSL https://oskalang.dev/oska-release.pub -o oska-release.pub
ssh-keygen -l -E sha256 -f oska-release.pub
printf 'oska-release %s\n' "$(cat oska-release.pub)" > allowed
ssh-keygen -Y verify -f allowed -I oska-release -n oska-release -s SHA256SUMS.sig < SHA256SUMS
head -n 1 SHA256SUMS
sha256sum --ignore-missing -c SHA256SUMS
```

1. `ssh-keygen -l` must print the fingerprint above. If it prints another, stop: the key is not the release key.
2. `ssh-keygen -Y verify` must print `Good "oska-release" signature`.
3. The first line of `SHA256SUMS` must be `# oska-release v0.6.0`, the release you asked for.
4. `sha256sum` must print `OK` for the binary you downloaded. On macOS, use `shasum -a 256 --ignore-missing -c SHA256SUMS`.

From v0.7.0, you can also check where a binary was built. Each release's binaries have a GitHub build-provenance attestation: a statement signed by GitHub that the release workflow of `aparsons/oska` built the file from a given commit. With the [GitHub CLI](https://cli.github.com/), download the file and run:

```sh
gh attestation verify "$asset" -R aparsons/oska
```

It must print `Verification succeeded!` and name the workflow `aparsons/oska/.github/workflows/release.yml` and the tag. The attestations are in the release's `oskac.intoto.jsonl`, which `SHA256SUMS` lists, and the one for `SHA256SUMS` itself is in `SHA256SUMS.intoto.jsonl`. This check needs `gh` and a network; the installers do not make it, and checking the signature above stays the check that needs no outside service. v0.6.0 and earlier have no attestation. See [what provenance proves](threat-model.md#the-supply-chain).

v0.6.0 and earlier have one Linux file for each processor, `oskac-linux-x86_64` or `oskac-linux-aarch64`. From v0.7.0, set `asset` to the build you want instead: `oskac-linux-x86_64-gnu` or `oskac-linux-x86_64-musl` (see [the two Linux builds](#the-two-linux-builds)).

### Run oskac in a container

From v0.7.0, each release also publishes the container image `ghcr.io/aparsons/oskac`, tagged `vX.Y.Z`. `latest` names the newest stable release: it moves only to a release that the signed release index names as the latest, so it never moves back to an older one. One name serves both Linux processors: Docker picks the x86_64 or the aarch64 image for the host. Each image holds only the musl build at `/oskac` and an empty `/tmp`, with no shell and no package manager. It runs as user 65532, not root, and `/oskac` is its entrypoint, so the arguments are `oskac`'s:

```sh
docker run --rm -e OSKA_CACHE=off -v "$PWD:/w" -w /w ghcr.io/aparsons/oskac@sha256:<digest> check main.osk
```

- **Pull by digest, not by tag.** A tag can move; a digest names exact bytes, and Docker checks every byte it pulls against it. The digest of each release's image is in its `oskac-image-digest` file, one line `sha256:<hex>`.
- **Verify the digest** with the release key, as you verify a binary: see [Verify an image](#verify-an-image).
- **Rebuild it to check its contents.** The image is built from the release's two musl builds by `scripts/release/image.sh` in the repository, as reproducible bytes: rebuilt from the same files, it has the same digest. The script needs bash 4.4 or newer, coreutils (`sha256sum` among them) and `jq`, and no Docker; it writes the tar itself. The bash that macOS ships is 3.2, too old: use Linux, or a newer bash. Download `oskac-linux-x86_64-musl` and `oskac-linux-aarch64-musl` of the release into one directory. Check out the release's tag in a clone of the repository at `<clone>`, `git -C <clone> checkout "$v"` with the tag in `v`, so you run the `image.sh` that built that release (a later one may write other bytes). Then, in the download directory, run `SOURCE_DATE_EPOCH=$(git -C <clone> log -1 --format=%ct "$v") <clone>/scripts/release/image.sh oskac-linux-x86_64-musl oskac-linux-aarch64-musl "$(mktemp -d)/image"`. It must print the digest in `oskac-image-digest`.
- **`oskac run` and `oskac test` work in the image.** They build the native executable in `/tmp`, the image's one writable directory.
- **Writing into the mounted folder.** User 65532 must be able to write there for `oskac build` to write its executable. Run as yourself with `--user "$(id -u):$(id -g)"`, or make the folder writable.
- **The build cache.** A `--rm` container keeps nothing between runs, so turn the build cache off with `-e OSKA_CACHE=off` (see [`oskac cache`](cli-reference.md#oskac-cache)).

#### Verify an image

`SHA256SUMS` lists the release's `oskac-image-digest` file, so the release signature covers it. For the release in `v`, such as `v0.7.0`:

```sh
v=v0.6.0
base=https://oskalang.dev/releases/$v
curl -fsSL --remote-name-all "$base/SHA256SUMS" "$base/SHA256SUMS.sig" "$base/oskac-image-digest"
curl -fsSL https://oskalang.dev/oska-release.pub -o oska-release.pub
ssh-keygen -l -E sha256 -f oska-release.pub
printf 'oska-release %s\n' "$(cat oska-release.pub)" > allowed
ssh-keygen -Y verify -f allowed -I oska-release -n oska-release -s SHA256SUMS.sig < SHA256SUMS
head -n 1 SHA256SUMS
sha256sum --ignore-missing -c SHA256SUMS
docker run --rm "ghcr.io/aparsons/oskac@$(cat oskac-image-digest)" --version
```

`ssh-keygen -l` must print the fingerprint in [Verify a release by hand](#verify-a-release-by-hand), and `ssh-keygen -Y verify` `Good "oska-release" signature`. The first line of `SHA256SUMS` must be `# oska-release v0.6.0`, the release you asked for, and `sha256sum` must print `oskac-image-digest: OK`. `oskac-image-digest` is one line, `sha256:<hex>`; the last command pulls the image by it and prints `oskac 0.6.0`.

### Verify the installation

```sh
oskac --version
```

If `oskac` is not found, add `~/.oska/bin` to your `PATH`. The [quickstart](quickstart.md) runs a first program.

### Update

Re-run the installer. It installs the latest release over the old one, and with `--version` it installs a specific one. It refuses to install a release older than the installed one with `downgrade_refused`. So if you installed a pre-release with `--version`, a plain re-run keeps it until a newer release comes out. To go back to an older release:

```sh
curl -fsSL https://oskalang.dev/install.sh | sh -s -- --version 0.2.0 --allow-downgrade
```

On a managed install, `oskac self update` does the same without the installer: see [Update with `oskac self update`](#update-with-oskac-self-update). To see whether an update exists first, use [`oskac self check`](#check-with-oskac-self-check) or the installer's [`--check`](#check-for-an-update).

### Check for an update

`--check` reports the installed and the latest version, and installs and writes nothing:

```sh
curl -fsSL https://oskalang.dev/install.sh | sh -s -- --check --json
```

```json
{"ok": true, "action": "checked", "installed": "0.5.0", "latest": "0.6.0", "tag": "v0.6.0", "update_available": true, "index_verified": true, "index_serial": 2, "index_expires": "2027-02-18T14:38:45Z"}
```

- `installed` is `null` when no `oskac` is installed.
- `update_available` is `true` when the latest release is newer than the installed one, or nothing is installed. It exits 0 either way.
- It takes the latest release from the signed index, with the same checks as an install, so it fails with `bad_index`, `stale_index` or `rollback_refused` as an install would. Before the site publishes an index, it reads `latest.json`, and `index_verified`, `index_serial` and `index_expires` are `false`, `null` and `null`.

### Check with `oskac self check`

`oskac self check` answers "is there a newer Oska, and is this binary a genuine one?" with no curl, no `ssh-keygen` and no system TLS library: `oskac` runs its built-in update checker, a program written in Oska, which uses Oska's own TLS client, SHA-256 and SSH signature verification. It installs, replaces and deletes nothing: the only thing it writes is the receipt's `index_serial`, raised after a verified check (never lowered). It does that for any receipt in the Oska home (`~/.oska`, or `OSKA_HOME`), even when the running `oskac` is not the managed binary, because the serial memory is shared. It uses the network only when you run it.

```sh
oskac self check --format=json
```

```json
{"ok":true,"action":"checked","installed":"0.6.0","latest":"0.7.0","tag":"v0.7.0","comparison":"behind","update_available":true,"index_serial":5,"index_expires":"2027-02-18T14:38:45Z","serial_remembered":true,"site":"https://oskalang.dev","warnings":[],"identity":{"state":"genuine","sha256":"<SHA-256 of this oskac>","expected_sha256":"<the same, from the signed SHA256SUMS>","source":"the signed SHA256SUMS of v0.6.0","matches_receipt":true}}
```

- It always uses `https://oskalang.dev`, the release key and the root certificates built into `oskac`. No flag, variable or setting changes them in a release build. The request carries only the user agent `oskac <version>`.
- It verifies the signed release index (the signature in the `oska-index` namespace before the index is read, then the expiry) and refuses one whose serial is below the highest this install has accepted. It exits 0 whether or not an update exists; an answer it could not verify is an error, never "no update".
- `comparison` is `behind`, `current` or `ahead` by semver precedence, and `update_available` is true only when behind. The human output ends with the command to run when an update exists: `oskac self update` on a managed install (one the installer put in place), the installer command otherwise.
- `serial_remembered` is `true` when an install receipt exists: after a verified check, `index_serial` in the receipt is raised to the index's serial (never lowered, written atomically). With no receipt (a source build, a copied binary) it is `false`, the rollback guard is not active for this install, and only the expiry bounds a freeze.
- `identity.state` is `genuine` (this binary's SHA-256 equals its entry in the signed `SHA256SUMS` of the release it reports), `modified` (that release is published, but the digest differs), `unknown_release` (no published release has this version, as for a source build) or `skipped` (`--no-identify`, which also saves two downloads). None is an error. The comparison uses the signed `SHA256SUMS`, not the receipt; `matches_receipt` shows whether the file still has the digest the receipt recorded (`null` when either is missing).
- `warnings` lists, for example, that the index expires within 30 days.
- A failure is `{"ok": false, "error": "<code>", "message": "..."}` with exit code 1, and a usage error exits 2. The codes are `bad_index`, `stale_index`, `rollback_refused`, `network_error`, `tls_error`, `bad_checksums` and `internal` (see the table above; the same codes, with the others the table marks for `self update`, come from `self update`). If the system clock is wrong, `stale_index` and `tls_error` messages say so.

### Update with `oskac self update`

`oskac self update` replaces the running `oskac` with the latest release, or with release `X` for `--version X`, with no installer script. It uses the same network and trust as `self check`, and checks what the installers check: the signed release index (its signature, its expiry and a serial never below the one in the receipt), the signature on the release's `SHA256SUMS`, the SHA-256 of the downloaded binary, and the version the new binary reports when `oskac` runs it. The update checker writes the download only to a staging file; `oskac` replaces itself from it in one step (on Windows by renaming the running `oskac.exe` aside first) and then writes the new version, digest and serial into the receipt.

```sh
oskac self update --format=json
```

```json
{"ok":true,"action":"updated","from":"0.6.0","to":"0.7.0","index_serial":5,"site":"https://oskalang.dev","warnings":[]}
```

- `action` is `updated`, `current` (the latest release is the one installed; nothing changes) or `downgraded`. Never report a `downgraded` as an update.
- **The managed-install rule.** It acts only on a managed install: one the installer put in place, whose receipt lists the path of the running `oskac` as a binary of this version. A source build, a copied binary, a package-manager install, or an `oskac` that is not the one the receipt names is refused with `unmanaged_install`, before any network use and with nothing changed. Re-run the installer, or rebuild from source. `oskac self check` works on any install.
- **A downgrade needs a name.** `--version X` installs release `X`, newer or not. If `X` is older than the installed `oskac` it is refused with `downgrade_refused` unless the run also has `--allow-downgrade`, and then the action is `downgraded`. `--allow-downgrade` without `--version` is a usage error (exit code 2). A serial below the receipt's is still refused with `rollback_refused`, and an expired index with `stale_index` (a named release still installs with `--version`).
- **A failure changes nothing.** A bad signature, a tampered `SHA256SUMS`, a digest that differs, a binary that reports another version, a connection cut mid-download or a failed replacement leaves the installed `oskac` and the receipt as they were, and removes the staging file. The codes are those of [the table above](#one-line-installers): `bad_index`, `stale_index`, `rollback_refused`, `network_error`, `tls_error`, `bad_checksums`, `checksum_mismatch`, `version_mismatch`, `downgrade_refused`, `unmanaged_install`, `replace_failed`, `no_previous_version` and `internal`. A failure is `{"ok": false, "error": "<code>", "message": "..."}` with exit code 1, and a usage error exits 2.

#### Roll back

`oskac self update --rollback` undoes an update by going back exactly one version, to the receipt's `previous_version`. It downloads that release and verifies it as it would for an install, so no old binary has to be on your machine, and reports `downgraded` with `"source":"downloaded"`. It does not combine with `--version` or `--allow-downgrade`. With no previous version recorded, or one equal to the installed version, it fails with `no_previous_version` and changes nothing.

By default nothing old is kept. Run `oskac self update --keep-previous` to keep the binary it replaces, beside the live one, for that update only. The receipt records the copy in `kept_previous` (its version, path and the SHA-256 from the signed `SHA256SUMS`), and the JSON reports it as `kept_previous` (`null` when nothing was kept, for example because the replaced binary was not the published release). A rollback then has the update checker hash the copy, and uses it (`"source":"kept"`) only if that equals the recorded digest; otherwise it ignores the copy, warns, and downloads. The next update or rollback discards a kept copy unless it passes `--keep-previous` again, and `oskac self uninstall` and the installer's `--uninstall` remove it.

#### What `oskac` sends, and what can go wrong

- **The network is used only by `oskac self check` and `oskac self update`**, and only to `https://oskalang.dev` on port 443, when you run them. `self info`, `self uninstall`, `check`, `run`, `test`, `build` and every other command make no connection. Nothing runs in the background, at startup or on a schedule. A request carries the user agent `oskac <version>` and no identifier of you or your machine. (Later, `oskac` will also fetch packages you ask for.)
- **The TLS and signature code is new and unaudited.** `oskac` verifies with TLS, SHA-256 and Ed25519/SSHSIG code written in Oska, not with a reviewed library ([ADR 0012](../adr/0012-tls-git-and-crypto-are-written-in-oska.md)). The signature checks mean that a flaw in the TLS code costs privacy and availability, not authenticity; a flaw in the Ed25519 or SSHSIG code would cost authenticity.
- **A wrong clock.** A clock that is wrong or set back can make a valid index look expired (`stale_index`) or a certificate look invalid (`tls_error`), and the messages say so. It cannot make an expired index look valid, except that on an install with no receipt a clock set back can extend a freeze, because there is no remembered serial to hold the index to.
- **The update checker and the replacement.** The update checker holds a connection narrowed to oskalang.dev and write access to one staging path, so a flaw in its file handling is bounded by that path. A flaw in the replacement code, which is Rust, is covered by the tests that a failure leaves the live binary and the receipt unchanged.
- **After a key rotation,** an `oskac` built before it reports `bad_index`, as the installers do: download the installer again and re-run it (see [Check with `oskac self check`](#check-with-oskac-self-check)); `oskac self update` cannot trust the new key.

### Uninstall

Run the installer with `--uninstall`. It removes what the [install receipt](#the-install-receipt) lists, and nothing else:

```sh
curl -fsSL https://oskalang.dev/install.sh | sh -s -- --uninstall
```

```powershell
& ([scriptblock]::Create((irm https://oskalang.dev/install.ps1))) --uninstall
```

It downloads nothing, so a saved copy of the installer works offline. Set `OSKA_HOME` as you did to install, and it uninstalls from there.

- **It removes only what the receipt lists**, and only files directly in the Oska home or its `bin` directory. Then it removes `bin` and the Oska home if they are left empty. It never removes a directory with anything in it.
- **It checks everything before it removes anything.** An entry outside those places, a path with `..`, or a path through a symlink stops it with `unsafe_path`, and nothing is removed. That covers a symlink or junction anywhere in the path, above the Oska home too. On Linux and macOS a symlink owned by root, such as macOS's `/var`, is allowed, as only an administrator can make one. With no receipt it stops with `not_installed`.
- **It acts on each file once,** however many times the receipt lists it.
- **It keeps a file that changed** since it was installed: one whose SHA-256 is no longer the one the receipt recorded.
- **It never touches `~/.config/oska`,** which holds settings and credentials, nor anything in any other `.config/oska` directory, so it is safe even when no home directory is set.
- **It undoes `--add-to-path` only if nothing changed.** It removes the marked line from the profile only if that exact line is still there, and the entry from your user `PATH` (Windows) only if it is still there. Otherwise it edits nothing, lists the profile (or the `bin` directory) in `kept`, and says in `path_hint` what to remove. Without `--add-to-path` it does not edit your `PATH`: if the `bin` directory is on it, the installer says so, and you remove the line that adds it from your shell profile (on Windows, the entry from your user `PATH`).
- **It undoes `--hook` the same way.** It removes the settings file the installer created, and the hook script an earlier installer wrote, only if they are unchanged. Settings the installer did not create, or that you changed since, stay, and are listed in `kept`: remove the `oskac hook` entries from them by hand. It leaves the `.claude` directories.

With `--json` it prints:

```json
{"ok": true, "action": "uninstalled", "removed": ["/home/you/.oska/bin/oskac", "/home/you/.oska/version", "/home/you/.oska/receipt.json", "/home/you/.oska/bin", "/home/you/.oska"], "kept": [], "path_hint": null}
```

- `removed` lists every file and directory it removed. A profile line or a user `PATH` entry it removed is listed with a note, such as `/home/you/.zshrc (the PATH line the installer added)`.
- `kept` lists what it left in place: a file that changed, the `bin` directory or the Oska home if something else is in it, and `~/.config/oska` if it exists.
- `path_hint` says what to remove from your `PATH`, or is `null` when the `bin` directory is not on it.

`oskac self uninstall` does the same from the installed `oskac`, with no installer, with the same refusals and error codes: see [`oskac self`](cli-reference.md#oskac-self). [The install receipt](install-receipt.md) sets out, kind by kind, where it and the two installers differ. It leaves a PATH line or user `PATH` entry that `--add-to-path` added to the installer's `--uninstall`, and says so in `path_hint`. On Windows it lists the running `oskac.exe` under `pending`. `oskac self info` reports the install and whether the receipt matches it.

An install made before the installers wrote a receipt has none, so `--uninstall` stops with `not_installed`. Delete `~/.oska` by hand: it holds `bin/oskac` (`bin\oskac.exe` on Windows) and a `version` file.

### Require a minimum oskac version

A package can name the oldest `oskac` it works with, with `oska` in the `[package]` table of `oska.toml`:

```text
[package]
name = "shopping"
version = "0.1.0"
oska = "0.4.0"
```

An older `oskac` refuses the package with `E0407`, from `check`, `run`, `test` and `build`. The message names both versions and says to install a newer one: run the installer again (see [Update](#update)). The key is a minimum, never an exact pin, so a newer `oskac` always accepts it. See [packages and modules](packages.md#starting-a-package) for the key and [`E0407`](../checker.md#e0407).

## Upgrading

Each release's notes are in the [changelog](../../CHANGELOG.md), newest first. They say what changed and what to do about it. Read the notes of every release after yours before you update. Changes on `main` that are not released yet are under "Unreleased".

## Set up an AI agent

The site publishes a copy-and-paste prompt that has an AI coding agent install Oska, verify the toolchain, and configure your project. It is at the top of this page and at `https://oskalang.dev/agent-setup.md` as plain Markdown. The [agent guide](agent-guide.md) describes the loop the prompt sets up.

## Build from source

You need a stable Rust toolchain, version 1.100.0 or newer, and a checkout of the repository:

```sh
cargo build --release -p oskac
```

The binary is `target/release/oskac`. Copy it somewhere on your `PATH`, or let Cargo install it into `~/.cargo/bin`:

```sh
cargo install --path crates/oskac
```

Confirm that it works:

```sh
oskac --version
```

It prints `oskac` and the version on one line. A build from this checkout prints `oskac 0.6.0`. Then continue with the [quickstart](quickstart.md).
