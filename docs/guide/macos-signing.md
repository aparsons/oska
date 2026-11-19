# Signing and notarizing for macOS

`oskac build --target macos-aarch64` and `--target macos-x86_64` write a macOS executable on any host, with no Apple tools, and it runs on a Mac. To give it to other people as a download, sign it with your own Developer ID and have Apple notarize it. This page says what `oskac` signs, what that signature does and does not do, and how to do the rest with Apple's tools. Signing with a Developer ID and notarizing are your steps: `oskac` does neither.

## What `oskac` signs

Every macOS executable `oskac` writes, for both processors, carries an **ad-hoc signature**. That is a code signature with page hashes and no certificate, which `oskac` writes itself, so the same program gives the same bytes on every host, the signature included.

An ad-hoc signature does these things:

- **It lets the executable run on Apple silicon.** macOS on Apple silicon refuses to run arm64 code that has no signature at all. An ad-hoc signature is enough.
- **It lets macOS notice a damaged or changed file.** The signature hashes every page of the file, so a file changed after `oskac` wrote it no longer matches. On Apple silicon, macOS refuses to run it. An Intel Mac enforces them only in some cases, such as under the hardened runtime, so there a changed file may still run.

It does not do these things:

- **It does not say who built the executable.** It has no certificate, so anyone can write one, for any file. It is no proof of origin.
- **It does not pass Gatekeeper after a download.** A file downloaded with a browser, or received another way that marks it as quarantined, is checked by Gatekeeper the first time it is opened. Gatekeeper refuses an ad-hoc signed file and says that Apple cannot check it for malicious software. A file you build on the same Mac, or copy there with `scp` or a CI download that sets no quarantine mark, runs.
- **It cannot be notarized.** Apple notarizes only code signed with a Developer ID and the hardened runtime.

## Sign with your Developer ID

You need a membership of the Apple Developer Program and a **Developer ID Application** certificate in your login keychain. Apple's tools run on a Mac, so build on any host, then copy the executable to a Mac to sign it.

Signing replaces `oskac`'s ad-hoc signature:

```sh
oskac build app.osk --target macos-aarch64 -o app
codesign --force --sign "Developer ID Application: Your Name (TEAMID)" --options runtime --timestamp app
codesign --verify --strict --verbose=2 app
```

- `--options runtime` turns on the hardened runtime, which notarization requires. An Oska executable needs no entitlements under it: its code is never writable, and it loads nothing but `libSystem`.
- `--timestamp` adds Apple's secure timestamp, which notarization also requires.
- After signing, the file's bytes differ from what `oskac` wrote. To check that a build is reproducible, compare `oskac`'s output before you sign it.

To ship one file for both processors, build both targets, join them with `lipo`, and sign the result:

```sh
oskac build app.osk --target macos-aarch64 -o app-arm64
oskac build app.osk --target macos-x86_64 -o app-x86_64
lipo -create -output app app-arm64 app-x86_64
codesign --force --sign "Developer ID Application: Your Name (TEAMID)" --options runtime --timestamp app
```

## Notarize

Notarization sends the signed file to Apple, which scans it and records a ticket that Gatekeeper looks up. Store your credentials once in a keychain profile. The password is an app-specific password for your Apple ID, not your Apple ID password:

```sh
xcrun notarytool store-credentials "notary" --apple-id you@example.com --team-id TEAMID
```

Then submit the executable in a zip archive and wait for the result:

```sh
ditto -c -k --keepParent app app.zip
xcrun notarytool submit app.zip --keychain-profile "notary" --wait
```

If the result is `Invalid`, `xcrun notarytool log <submission-id> --keychain-profile "notary"` says why.

## Staple, or not

`xcrun stapler staple` attaches the ticket to a file, so that Gatekeeper can check it offline. A ticket cannot be stapled to a bare executable. For one, Gatekeeper looks the ticket up online the first time it runs. To staple, put the signed executable in a disk image, then sign the disk image with `codesign` as above, notarize it, and staple it. An installer package works too, but it is signed with a separate Developer ID Installer certificate, by `productbuild` or `productsign`, not by `codesign`:

```sh
xcrun stapler staple app.dmg
```

## Without a Developer ID

For a tool you use yourself, or share with people who build it themselves, the ad-hoc signature is enough. Someone who downloads an ad-hoc signed file and trusts where it came from can remove the quarantine mark. That is their decision, since it skips Gatekeeper's check:

```sh
xattr -d com.apple.quarantine app
```
