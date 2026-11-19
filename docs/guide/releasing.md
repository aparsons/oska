# Releasing an application

`oskac release` builds your program for all five platforms and signs the result with your SSH key, in the format `oskac`'s own releases use. This page shows how to make a release, how your users check one, and how to add the platform signatures that Windows and macOS ask for, which are your own steps. The command's flags and output are in the [CLI reference](cli-reference.md#oskac-release).

## Make a signing key

Any SSH key that `ssh-keygen` can sign with will do. Keep a key for releases only, and keep its private half off shared machines:

```sh
ssh-keygen -t ed25519 -C greeter-release -f ~/.ssh/greeter-release
```

Publish the public half, `~/.ssh/greeter-release.pub`, where your users get it from somewhere other than the downloads, such as your website or your repository. A key served next to the files it signs proves nothing: whoever can change one can change the other.

## Make a release

```sh
oskac release greeter --version 1.2.0 --key ~/.ssh/greeter-release
```

This checks the package in `greeter/`, builds it for every platform, and writes `greeter-v1.2.0/`:

```text
greeter-v1.2.0/
  SHA256SUMS
  SHA256SUMS.sig
  greeter-linux-aarch64
  greeter-linux-x86_64
  greeter-macos-aarch64
  greeter-macos-x86_64
  greeter-windows-x86_64.exe
```

`SHA256SUMS` starts with a line that names the release, then lists the SHA-256 of each executable:

```text
# greeter-release v1.2.0
6ec8bee5…  greeter-linux-aarch64
…
```

`SHA256SUMS.sig` is its signature, made by `ssh-keygen -Y sign` in the namespace `greeter-release`. Because the first line is signed with the rest, a signed release cannot be passed off as another version. The same program built by the same `oskac` gives the same executables on every host, so anyone can rebuild a release and compare the checksums.

Upload the whole directory. In CI, add `--format=json` to get the files, their checksums and the signature as one JSON object.

## How your users check a release

Users need OpenSSH 8.1 or newer, and the public key you published. In the directory they downloaded into:

```sh
printf 'greeter-release %s\n' "$(cat greeter-release.pub)" > allowed
ssh-keygen -Y verify -f allowed -I greeter-release -n greeter-release -s SHA256SUMS.sig < SHA256SUMS
head -n 1 SHA256SUMS
sha256sum --ignore-missing -c SHA256SUMS
```

1. `ssh-keygen -Y verify` must print `Good "greeter-release" signature`.
2. The first line must be `# greeter-release v1.2.0`: the release they meant to download.
3. `sha256sum` must print `OK` for each file they downloaded. On macOS, use `shasum -a 256 --ignore-missing -c SHA256SUMS`. On Windows, compare `(Get-FileHash greeter-windows-x86_64.exe).Hash` in PowerShell with the file's line, ignoring case.

If any step fails, the file is not the one you released.

## Authenticode and Apple notarization

The SSH signature tells your users that a file is the one you released. It does not satisfy the operating systems' own checks for downloaded programs:

- **Windows.** SmartScreen warns about an executable without an Authenticode signature from a code-signing certificate. Signing with one is your step, with `signtool` on Windows or `osslsigncode` elsewhere.
- **macOS.** Gatekeeper refuses a downloaded executable that is only ad-hoc signed, as every macOS executable `oskac` writes is. Signing with a Developer ID and notarizing are your steps: see [Signing and notarizing for macOS](macos-signing.md).

Both signatures change the executable's bytes, so the checksums in `SHA256SUMS` no longer match. Sign the executables first, then write `SHA256SUMS` again and sign it again, keeping its first line:

```sh
cd greeter-v1.2.0
# ... sign greeter-windows-x86_64.exe and the macOS executables here ...
{ head -n 1 SHA256SUMS; LC_ALL=C sha256sum -- greeter-* | LC_ALL=C sort -k2; } > SHA256SUMS.new
mv SHA256SUMS.new SHA256SUMS
rm SHA256SUMS.sig
ssh-keygen -Y sign -f ~/.ssh/greeter-release -n greeter-release SHA256SUMS
```

`oskac` does not sign with Authenticode or a Developer ID itself. After these signatures, a rebuild no longer gives the same bytes as the release, so keep the first `SHA256SUMS` if you want others to check a rebuild against it.
