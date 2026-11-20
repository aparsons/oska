# Releasing Oska

The maintainer runbook for cutting a release, checking it, and keeping the signing setup intact. Users install from https://oskalang.dev. How the installers behave is documented in `docs/guide/install.md`.

## How a release flows

```text
git tag vX.Y.Z on main ─▶ release.yml
  build      7 native builds: Linux gnu and musl on ubuntu-22.04 / -arm, macOS, Windows
             (check-linux-build.sh: gnu needs glibc ≤ 2.35, musl is static)
  assemble   image.sh: the image from the two musl builds, its index digest in
             oskac-image-digest; SHA256SUMS: line 1 "# oska-release vX.Y.Z",
             then "<sha256>  <file>" per binary and for oskac-image-digest
  smoke-image  on ubuntu-22.04 / -arm: each musl build in alpine and in the image
             (smoke-image.sh: --version, oskac run and oskac build of hello.osk)
  sign       env "release": ssh-keygen -Y sign -n oska-release with RELEASE_SIGNING_KEY,
             then sign-index.sh: the next release index, -n oska-index, same key
  publish    env "release": push-image.sh pushes the signed image to
             ghcr.io/aparsons/oskac:vX.Y.Z (and :latest), then GitHub Release
             vX.Y.Z with the index beside the files (the source pages.yml
             fetches), then dispatch pages.yml on main
pages.yml ─▶ downloads every release, verify.sh each against site/oska-release.pub,
             stages /releases/vX.Y.Z/ and latest.json, publishes the highest-serial
             index as /release-index.json, pins install.sh/.ps1 to the key and,
             once an index is published, to require it

monthly (1st, 05:17 UTC) or by hand on main ─▶ resign-index.yml
  sign       env "release": waits out any publishing Release or Pages run, then
             sign-index.sh with no tag: the live index re-signed, serial + 1,
             same latest, 90 more days
  publish    replaces the index on the GitHub Release it names, then
             dispatches pages.yml on main
```

On a pull request, `release.yml` does a dry run: the same builds and image smoke test, signed with a throwaway key (the index too, as if none were published), and nothing published or pushed.

## The release assets

`scripts/release/assemble.sh` lists them, and a release ships exactly these, each under its own key in `latest.json`'s `binaries` (the name without `oskac-` and `.exe`):

| Asset | Build | Checked in the build job |
|---|---|---|
| `oskac-linux-x86_64-gnu`, `oskac-linux-aarch64-gnu` | the glibc build, on `ubuntu-22.04` / `-arm` | no glibc symbol newer than `GLIBC_2.35` |
| `oskac-linux-x86_64-musl`, `oskac-linux-aarch64-musl` | the musl build, `--target <arch>-unknown-linux-musl` on the same runners | no `PT_INTERP` and no `DT_NEEDED`: static, so it runs on any Linux |
| `oskac-macos-x86_64`, `oskac-macos-aarch64` | on `macos-15-intel` / `macos-latest` | |
| `oskac-windows-x86_64.exe` | on `windows-latest` | |

Every row also checks that `uname -m` is the processor its name says, and that the binary prints its version. `scripts/release/check-linux-build.sh gnu|musl <binary>` does the two Linux checks; run it by hand on a downloaded Linux asset to check it again. `SHA256SUMS` lists all seven, and `oskac-image-digest` (see [The container image](#the-container-image)), so the signature covers each. ADR 0022 records why Linux has two builds. `scripts/release/check-linux-build-test.sh` tests both checks.

**A breaking rename.** Releases before the two builds shipped `oskac-linux-x86_64` and `oskac-linux-aarch64`. From the first release with them, those files are gone, and `latest.json`'s `binaries` has no `linux-x86_64` or `linux-aarch64` key: it has `linux-x86_64-gnu`, `linux-x86_64-musl`, `linux-aarch64-gnu` and `linux-aarch64-musl` in their place (the macOS and Windows keys are unchanged). Anything that reads the old keys or downloads the old names breaks, an installer saved from before the rename included (`download_failed`); say so in that release's notes.

## The container image

Each release publishes `ghcr.io/aparsons/oskac:vX.Y.Z`, and for a stable release `:latest`: an OCI image index with one image per Linux processor, each holding only the musl build at `/oskac` and an empty `/tmp`, run as user 65532 (ADR 0022).

- **Built from the release's own musl builds.** `assemble.sh` runs `scripts/release/image.sh` on `oskac-linux-x86_64-musl` and `oskac-linux-aarch64-musl`, with `SOURCE_DATE_EPOCH` set to the commit time of the tag, and writes the digest of the image index, one line `sha256:<hex>`, to `oskac-image-digest`. `SHA256SUMS` lists that file, so the `oska-release` signature covers the image. `verify.sh` refuses a digest file that is not exactly that one line. It is not a binary: `latest.json`'s `binaries` does not list it, and the installers never download it.
- **Smoke-tested before signing.** The `smoke-image` job, on `ubuntu-22.04` and `ubuntu-22.04-arm`, runs `scripts/release/smoke-image.sh`: each musl build in `alpine` (pinned by digest) and in the image as Docker picks it for that processor, `oskac --version`, `oskac run` and `oskac build` of `examples/hello.osk`, and the executable it built. `sign` waits for it.
- **Pushed only after signing, and only the signed bytes.** In `publish`, before the GitHub Release is created, `scripts/release/push-image.sh` verifies the signed release against `site/oska-release.pub`, and refuses the OCI layout `assemble` wrote unless its index is the signed digest and every blob matches its digest. It pushes those exact bytes with `skopeo copy --all --preserve-digests` and the workflow's own `GITHUB_TOKEN`, first by digest only, and reads that back: if the registry does not serve the signed digest, the release fails before any tag names it. Then it tags `vX.Y.Z`, and `latest` only when the signed release index beside the release names this release, so `latest` never moves back, as the index never does (see [The release index](#the-release-index)); each tag is read back too. Only `publish` has `packages: write`; `scripts/release/test.sh` fails if another job gets it. It uses the runner's own `skopeo` and installs nothing. If the push fails, nothing is published: fix it and run the failed jobs again (a re-run pushes the same bytes).
- **The package's visibility.** The first push creates the package private. Once, in the package's settings on GitHub, make it public, and check that it is linked to this repository with "Actions" access for it. Nothing in the repo can check this.

### Verify an image by its digest

Users pull by digest, never by tag, and a tag can move. To check that a digest is the one a release signed:

```
v=vX.Y.Z
curl -fsSO "https://oskalang.dev/releases/$v/SHA256SUMS" -O "https://oskalang.dev/releases/$v/SHA256SUMS.sig" \
  -O "https://oskalang.dev/releases/$v/oskac-image-digest"
printf 'oska-release %s\n' "$(curl -fsS https://oskalang.dev/oska-release.pub)" > allowed
ssh-keygen -Y verify -f allowed -I oska-release -n oska-release -s SHA256SUMS.sig < SHA256SUMS
head -n 1 SHA256SUMS                          # must be: # oska-release vX.Y.Z
sha256sum --ignore-missing -c SHA256SUMS      # oskac-image-digest: OK
digest=$(cat oskac-image-digest)
docker run --rm -v "$PWD:/w" -w /w "ghcr.io/aparsons/oskac@$digest" --version
```

Pulling `ghcr.io/aparsons/oskac@<digest>` makes Docker check every byte against that digest. To see that a tag points at it: `skopeo inspect --raw docker://ghcr.io/aparsons/oskac:$v | sha256sum` prints the same hex.

To check that the image holds exactly the signed binaries, rebuild it from them: download `oskac-linux-x86_64-musl` and `oskac-linux-aarch64-musl` into the same directory, then, in a clone of the repo,

```
SOURCE_DATE_EPOCH=$(git log -1 --format=%ct "$v") \
  scripts/release/image.sh oskac-linux-x86_64-musl oskac-linux-aarch64-musl "$(mktemp -d)/image"
```

It must print the digest in `oskac-image-digest`.

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
- **`serial` only goes up.** `sign-index.sh` reads the live index, verifies it, and signs its serial + 1 (1 if none is published). The largest serial is 999999999999999 (15 digits), in `verify-index.sh`, `sign-index.sh` and both installers alike. `latest` is the live index's, or the release's tag if that is a newer stable release, so it never moves back. The signing step refuses a key whose public half is not `site/oska-release.pub`, as `sign.sh` does.
- **It expires 90 days after it is signed.** Installers warn when fewer than 30 days remain, and refuse an expired index (`stale_index`), so a freeze lasts at most 90 days. `--version X` never reads the index, so a named release still installs.
- **Installers refuse a lower serial than the one they last accepted** (`rollback_refused`), which the install receipt records as `index_serial`.
- **Where it lives.** The release run attaches the index to the tag's GitHub Release. `stage-site.sh` takes every index out of its release directory before verifying the release, so a release directory still holds only what `SHA256SUMS` lists. It verifies every index, and stops the build if one does not verify, if two different ones have the same serial, or if the one with the highest serial names a release that is not published. It publishes the one with the highest serial and warns (a `::warning::` annotation) when it expires within 30 days.
- **An expired index does not stop the Pages build.** `stage-site.sh` still publishes it, with a `::warning::` that it has expired, so the rest of the site deploys, and the installers stay pinned to require it. They refuse it with `stale_index`, so only installing the latest release fails, and `--check` with it: `--version X` and `--uninstall` work as before. The Pages run's smoke job then fails at the install step until the index is re-signed (below).
- **Pinning.** When `stage-site.sh` publishes an index, `pin.sh --require-index` pins the installers to require it, as they are pinned to the key. From then on, installers take the latest release only from the verified index and report `index_verified: true`. Before the first index exists, they read `latest.json` and report `index_verified: false`, so the change needed no rollout window.

### The monthly re-sign

Releases can be months apart, so `resign-index.yml` re-signs the index on the 1st of every month, unattended, with the release key. A live index is then never more than about a month old, and the 30-day warning never fires while the workflow works.

- **The key now signs without a person present.** Besides a `v*` tag push, a scheduled or dispatched run on `main` reads `RELEASE_SIGNING_KEY`. It can sign only what `sign-index.sh` signs: the live index, verified first, with its serial + 1 and the same `latest`. It names no new release, and it refuses to sign over an index that does not verify. Only its `sign` job reads the key; the `publish` job, which uploads, has no access to it. `scripts/release/test.sh` fails if any job outside the `release` environment reads the key, or if a third job starts reading it.
- **What it does.** It waits (up to 6 minutes) while a Release or Pages run is under way (a pull request's dry run does not count, since it publishes nothing), since either could publish an index with the serial it is about to sign. Then it signs, replaces `release-index.json` and its signature on the GitHub Release that the index names (`gh release upload --clobber`), and dispatches `pages.yml`. With no index published yet it signs nothing and says so.
- **Run it by hand** when an index expires within 30 days, or has expired, and the workflow has not run since: `gh workflow run resign-index.yml --ref main`. Any other ref is refused.

**If the workflow fails,** the index keeps its current expiry, so there are about 60 days before installs fail. Read the failed run:

- *A Release or Pages run is under way:* run it again once that run has finished.
- *The `sign` job is refused by the environment* ("not allowed to deploy to release"): the `release` environment no longer admits `main` (see [Repository configuration](#repository-configuration-it-depends-on)).
- *`the RELEASE_SIGNING_KEY secret is not set`, `does not match the published key`, or `does not verify`:* the key or the live index is wrong. Do not re-run it until you know why; a live index that does not verify may mean the site was altered.
- *The Pages run it dispatched fails* with `the release indexes in ... differ but both have serial N`: a release run signed at the same time from the same live index. Delete `release-index.json` and its `.sig` from the older of the two GitHub Releases, dispatch `pages.yml`, then run `resign-index.yml` again.

If the workflow cannot run at all before the index expires, re-sign it by hand from the offline backup of the key, then dispatch `pages.yml`. Otherwise "install latest" fails with `stale_index` for everyone:
```
scripts/release/sign-index.sh release-key site/oska-release.pub https://oskalang.dev out
gh release upload vX.Y.Z out/release-index.json out/release-index.json.sig --clobber
```
`vX.Y.Z` is the release that `out/release-index.json` names (`jq -r .latest.tag`). Shred the plaintext key afterwards.

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
- **Environment `release`:** deployment limited to tags matching `v*` and the branch `main`. No required reviewers (decided 2026-11-10). `main` is admitted only so that `resign-index.yml` can re-sign the index unattended ([#202](https://github.com/aparsons/oska/issues/202)); GitHub cannot admit one workflow, only refs, so every workflow on `main` could now name the environment. That is bounded by `main` changing only through reviewed pull requests, and by `scripts/release/test.sh`, which allows the key in exactly two jobs: `release.yml`'s `sign` and `resign-index.yml`'s `sign`.
- **Ruleset "Release tags":** on `refs/tags/v*`, it restricts creation, update and deletion, and only repository admins may bypass it.
- **Package `ghcr.io/aparsons/oskac`:** public, linked to this repository, with "Actions" access for it, so the release's `GITHUB_TOKEN` can push to it (see [The container image](#the-container-image)). It does not exist until the first release that pushes the image.
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
3. **Watch the tag's Release run** until build, assemble, smoke-image, sign and publish are all green. Nothing is published before `publish`. If an earlier job fails, delete the tag, fix the problem, and tag again. Every release job is bounded by `timeout-minutes` (build 20, the others 10), so a stuck one turns red by itself. A cancelled run is not a failed release: a superseded dry run on a pull request is cancelled on purpose, and a tag's run is never cancelled by a later push.
4. **Watch the Pages run that `publish` dispatches.** Its smoke job must end with `ok: https://oskalang.dev installs oskac X.Y.Z, and it runs`. Once an index is published, the line before it must read `the install verified release index serial N, which names X.Y.Z`. Its build job logs `published release index serial N`. A `::warning::` that the index expires within 30 days means the monthly re-sign has not run: see [The monthly re-sign](#the-monthly-re-sign).
   - The warning `no release is published` means the deploy ran before GitHub listed the new release ([#89](https://github.com/aparsons/oska/issues/89)).
   - Run `gh workflow run pages.yml --ref main` and check again.
   - A `::warning::` that the index has expired, with the smoke job failing on `stale_index`, means the monthly re-sign has not run: run it now (see [The monthly re-sign](#the-monthly-re-sign)).
5. **Name the new release in the docs** in a normal PR: "The latest release is **vX.Y.Z**" in `docs/guide/install.md` and `docs/guide/index.md`, which `crates/oskac/tests/front_door.rs` holds to the same version, with an "Upgrading to vX.Y.Z" section in the install guide.

## Checking a published release

- `curl -fsS https://oskalang.dev/latest.json` reports `"version": "X.Y.Z"`, `"tag": "vX.Y.Z"` and the key's `"signing_key_fingerprint"`.
- `/releases/vX.Y.Z/` serves `SHA256SUMS`, `SHA256SUMS.sig`, the 7 binaries (see [The release assets](#the-release-assets)) and `oskac-image-digest`.
- `ghcr.io/aparsons/oskac:vX.Y.Z` is the signed digest (see [Verify an image by its digest](#verify-an-image-by-its-digest)), and so is `:latest` for a stable release.
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
  6. Announce the new fingerprint, and update it in this file, `SECURITY.md` and `docs/guide/install.md`. `crates/oska-run/tests/security_policy.rs` fails until every copy matches the committed key. `latest.json`'s `signing_key_fingerprint` is computed from the key by `stage-site.sh`.

  Releases already published stay signed by the old key. `stage-site.sh` checks every GitHub Release against the current key and stops the whole Pages build on the first one that fails. So before committing the new public key, re-sign each old release's `SHA256SUMS` with the new key, or delete those GitHub Releases.

  The same goes for the release index. Delete `release-index.json` and `release-index.json.sig` from every GitHub Release, then sign the next index with the new key, continuing the serial of the live index, which the old key signed:
  ```
  scripts/release/sign-index.sh --previous-key old-release.pub new-key new-release.pub https://oskalang.dev out
  gh release upload vX.Y.Z out/release-index.json out/release-index.json.sig
  ```
  `vX.Y.Z` is the newest stable release. Installers already pinned to the old key refuse the new index with `bad_index`; like their copies of the old key, they have to be downloaded again.
