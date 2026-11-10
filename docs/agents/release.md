# Releasing Oska

The maintainer runbook for cutting a release, checking it, and keeping the signing setup intact. Users install from https://oskalang.dev. How the installers behave is documented in `docs/guide/install.md`.

## How a release flows

```text
git tag vX.Y.Z on main ─▶ release.yml
  build      5 native targets (Linux on ubuntu-22.04 / -arm: glibc ≤ 2.35 enforced)
  assemble   SHA256SUMS: line 1 "# oska-release vX.Y.Z", then "<sha256>  <file>" per binary
  sign       env "release": ssh-keygen -Y sign -n oska-release with RELEASE_SIGNING_KEY
  publish    env "release": GitHub Release vX.Y.Z (storage only), then dispatch pages.yml on main
pages.yml ─▶ downloads every release, verify.sh each against site/oska-release.pub,
             stages /releases/vX.Y.Z/ and latest.json, pins install.sh/.ps1 to the key
```

On a pull request, `release.yml` does a dry run: the same builds, signed with a throwaway key, and nothing published.

## Repository configuration it depends on

Nothing in the repo can check these settings, so verify them before a release.

- **Secret `RELEASE_SIGNING_KEY`:** an ed25519 OpenSSH private key with no passphrase. Its public half is `site/oska-release.pub`, fingerprint `SHA256:PjwTDycs560g1ktWC/gdznamSkzm4xH+OAdfntNaW4o`. It should live in the `release` environment, not at repository level.
- **Environment `release`:** deployment limited to tags matching `v*`. No required reviewers (decided 2026-11-10).
- **Ruleset "Release tags":** on `refs/tags/v*`, it restricts creation, update and deletion, and only repository admins may bypass it.
- **Environment `github-pages`:** deployment from `main` only. Pages uses `build_type=workflow`, the custom domain `oskalang.dev`, and enforced HTTPS.
- **Runners:** `ubuntu-22.04-arm` and `macos-15-intel` must stay available to this private repo. The PR dry run shows they are.

## Cutting a release

1. **Bump `[workspace.package] version` in `Cargo.toml`** to `X.Y.Z` in a normal PR, refreshing `Cargo.lock`. Merge the PR once `scripts/check.sh` passes and the PR checks are green, including the Release dry run.
2. **Tag the merge commit:**
   ```
   git checkout main && git pull --ff-only
   git tag -a vX.Y.Z -m "oska vX.Y.Z" && git push origin vX.Y.Z
   ```
   The push reports "Bypassed rule violations". That is the admin bypass of the tag ruleset working as intended.
3. **Watch the tag's Release run** until build, assemble, sign and publish are all green. Nothing is published before `publish`. If an earlier job fails, delete the tag, fix the problem, and tag again.
4. **Watch the Pages run that `publish` dispatches.** Its smoke job must end with `ok: https://oskalang.dev installs oskac X.Y.Z, and it runs`.
   - The warning `no release is published` means the deploy ran before GitHub listed the new release ([#89](https://github.com/aparsons/oska/issues/89)).
   - Run `gh workflow run pages.yml --ref main` and check again.

## Checking a published release

- `curl -fsS https://oskalang.dev/latest.json` reports `"version": "X.Y.Z"` and `"tag": "vX.Y.Z"`.
- `/releases/vX.Y.Z/` serves `SHA256SUMS`, `SHA256SUMS.sig` and the 5 binaries.
- Verify the signature against the committed key:
  ```
  printf 'oska-release %s\n' "$(cat site/oska-release.pub)" > allowed
  ssh-keygen -Y verify -f allowed -I oska-release -n oska-release -s SHA256SUMS.sig < SHA256SUMS
  ```
- Run a real install into a scratch home:
  ```
  curl -fsSL https://oskalang.dev/install.sh | OSKA_HOME=$(mktemp -d) sh -s -- --json
  ```
  It must print `"ok": true`, `"checksum_verified": true` and `"signature_verified": true`. The installed `oskac --version` must print `oskac X.Y.Z`.

## The signing key

- **Don't rotate it casually.** Every downloaded installer has the public key built in, so after a rotation those copies refuse new releases. Users have to re-download the installer.
- **Backups:** a passphrase-encrypted copy of the private key is kept offline by the maintainer. Nothing in the repo or on the site holds it.
- **To rotate:**
  1. Generate a new ed25519 key with no passphrase, in memory.
  2. Set it as the `release` environment's `RELEASE_SIGNING_KEY`.
  3. Commit its public half as `site/oska-release.pub`. The Pages build then publishes it and re-pins the installers.
  4. Shred the plaintext.
  5. Make a new encrypted backup.
  6. Announce the new fingerprint, and update it in this file.

  Releases already published stay signed by the old key. `stage-site.sh` checks every GitHub Release against the current key and stops the whole Pages build on the first one that fails. So before committing the new public key, re-sign each old release's `SHA256SUMS` with the new key, or delete those GitHub Releases.
