# Releasing Oska

The maintainer runbook for cutting a release, checking it, and keeping the signing setup intact. Users install from https://oskalang.dev. How the installers behave is documented in `docs/guide/install.md`.

## How a release flows

```text
git tag vX.Y.Z on main ─▶ release.yml
  build      5 native targets (Linux on ubuntu-22.04 / -arm: glibc ≤ 2.35 enforced)
  assemble   SHA256SUMS: line 1 "# oska-release vX.Y.Z", then "<sha256>  <file>" per binary
  sign       env "release": ssh-keygen -Y sign -n oska-release with RELEASE_SIGNING_KEY,
             then sign-index.sh: the next release index, -n oska-index, same key
  publish    env "release": GitHub Release vX.Y.Z with the index beside the files
             (the source pages.yml fetches), then dispatch pages.yml on main
pages.yml ─▶ downloads every release, verify.sh each against site/oska-release.pub,
             stages /releases/vX.Y.Z/ and latest.json, publishes the highest-serial
             index as /release-index.json, pins install.sh/.ps1 to the key and,
             once an index is published, to require it
```

On a pull request, `release.yml` does a dry run: the same builds, signed with a throwaway key (the index too, as if none were published), and nothing published.

## The release index

`/release-index.json` tells the installers which release is the latest. They trust it because it is signed, unlike `latest.json`, which stays unsigned, unchanged and informational. Without it, whoever could change `latest.json` could hold users on an old, genuine release.

```json
{
  "schema": 1,
  "serial": 4,
  "issued": "2026-11-13T10:00:00Z",
  "expires": "2027-02-11T10:00:00Z",
  "latest": {"tag": "v0.4.0", "version": "0.4.0"}
}
```

- **Signed with the release key, in its own namespace.** `/release-index.json.sig` is an `ssh-keygen -Y sign -n oska-index` signature, so an index signature never passes for a release signature (`oska-release`), or the reverse.
- **`serial` only goes up.** `sign-index.sh` reads the live index, verifies it, and signs its serial + 1 (1 if none is published). `latest` is the live index's, or the release's tag if that is a newer stable release, so it never moves back. The signing step refuses a key whose public half is not `site/oska-release.pub`, as `sign.sh` does.
- **It expires 90 days after it is signed.** Installers warn when fewer than 30 days remain, and refuse an expired index (`stale_index`), so a freeze lasts at most 90 days. `--version X` never reads the index, so a named release still installs.
- **Installers refuse a lower serial than the one they last accepted** (`rollback_refused`), which the install receipt records as `index_serial`.
- **Where it lives.** The release run attaches the index to the tag's GitHub Release. `stage-site.sh` takes every index out of its release directory before verifying the release, so a release directory still holds only what `SHA256SUMS` lists. It verifies every index, and stops the build if one does not verify. It publishes the one with the highest serial, if it has not expired and names a published release, and it warns (a `::warning::` annotation) when that index expires within 30 days.
- **Pinning.** When `stage-site.sh` publishes an index, `pin.sh --require-index` pins the installers to require it, as they are pinned to the key. From then on, installers take the latest release only from the verified index and report `index_verified: true`. Before the first index exists, they read `latest.json` and report `index_verified: false`, so the change needed no rollout window.

**Until the monthly re-sign workflow exists ([#202](https://github.com/aparsons/oska/issues/202)), only a release re-signs the index.** It then expires 90 days after the last release run, stable or pre-release. If no release is due before then, re-sign it by hand from the offline backup of the key, then dispatch `pages.yml`. Otherwise "install latest" fails with `stale_index` for everyone:
```
scripts/release/sign-index.sh release-key site/oska-release.pub https://oskalang.dev out
gh release upload vX.Y.Z out/release-index.json out/release-index.json.sig --clobber
```
`vX.Y.Z` is the newest stable release. Shred the plaintext key afterwards.

Check the live index by hand:

```
curl -fsSO https://oskalang.dev/release-index.json -O https://oskalang.dev/release-index.json.sig
printf 'oska-index namespaces="oska-index" %s\n' "$(cat site/oska-release.pub)" > allowed_index
ssh-keygen -Y verify -f allowed_index -I oska-index -n oska-index -s release-index.json.sig < release-index.json
jq . release-index.json
```

`scripts/release/verify-index.sh <dir> site/oska-release.pub` does the same checks, and the format check too, on a directory holding both files.

## Repository configuration it depends on

Nothing in the repo can check these settings, so verify them before a release.

- **Secret `RELEASE_SIGNING_KEY`:** an ed25519 OpenSSH private key with no passphrase. Its public half is `site/oska-release.pub`, fingerprint `SHA256:PjwTDycs560g1ktWC/gdznamSkzm4xH+OAdfntNaW4o`. It should live in the `release` environment, not at repository level.
- **Environment `release`:** deployment limited to tags matching `v*`. No required reviewers (decided 2026-11-10).
- **Ruleset "Release tags":** on `refs/tags/v*`, it restricts creation, update and deletion, and only repository admins may bypass it.
- **Environment `github-pages`:** deployment from `main` only. Pages uses `build_type=workflow`, the custom domain `oskalang.dev`, and enforced HTTPS.
- **Runners:** `ubuntu-22.04-arm` and `macos-15-intel` must stay available to this repo. It is public, so GitHub-hosted runners, macOS included, cost nothing. The PR dry run shows they are.

## Cutting a release

1. **Bump `[workspace.package] version` in `Cargo.toml`** to `X.Y.Z` in a normal PR, refreshing `Cargo.lock`. Merge the PR once `scripts/check.sh` passes and the PR checks are green, including the Release dry run.
2. **Tag the merge commit:**
   ```
   git checkout main && git pull --ff-only
   git tag -a vX.Y.Z -m "oska vX.Y.Z" && git push origin vX.Y.Z
   ```
   The push reports "Bypassed rule violations". That is the admin bypass of the tag ruleset working as intended.
3. **Watch the tag's Release run** until build, assemble, sign and publish are all green. Nothing is published before `publish`. If an earlier job fails, delete the tag, fix the problem, and tag again.
4. **Watch the Pages run that `publish` dispatches.** Its smoke job must end with `ok: https://oskalang.dev installs oskac X.Y.Z, and it runs`. Once an index is published, the line before it must read `the install verified release index serial N, which names X.Y.Z`. Its build job logs `published release index serial N`. A `::warning::` that the index expires within 30 days means it must be re-signed now.
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
  It must print `"ok": true`, `"checksum_verified": true` and `"signature_verified": true`, and, once an index is published, `"index_verified": true`. The installed `oskac --version` must print `oskac X.Y.Z`.
- Check the index by hand (see [The release index](#the-release-index)): it verifies, its `latest.version` is `X.Y.Z`, its serial is one above the previous one, and it expires in about 90 days. `sh -s -- --check --json` reports the same with `"index_verified": true`.

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

  The same goes for the release index. Delete `release-index.json` and `release-index.json.sig` from every GitHub Release, then sign the next index with the new key, continuing the serial of the live index, which the old key signed:
  ```
  scripts/release/sign-index.sh --previous-key old-release.pub new-key new-release.pub https://oskalang.dev out
  gh release upload vX.Y.Z out/release-index.json out/release-index.json.sig
  ```
  `vX.Y.Z` is the newest stable release. Installers already pinned to the old key refuse the new index with `bad_index`; like their copies of the old key, they have to be downloaded again.
