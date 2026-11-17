# Security policy

Oska is a secure-by-default language: a program can reach only the authority its `main` is handed, and untrusted values cannot reach a structural position of a sink. A way around either is a vulnerability, and so is a way to make the installers or the site do something they should not. Please report it privately.

## Reporting a vulnerability

Report it through GitHub private vulnerability reporting: [open a private report](https://github.com/aparsons/oska/security/advisories/new). It is the only channel. Do not open a public issue, pull request or discussion for a vulnerability.

A good report has:

- what you found, and which version or commit of `main` you found it in;
- the smallest program, file or command that shows it, with what you expected and what happened;
- what an attacker gains from it.

Send a report you have reproduced and read yourself. Please do not send unreviewed output from a scanner or an LLM: a finding nobody has checked costs the maintainers time that real reports need.

## What to expect

- We acknowledge your report within 7 days.
- While Oska is pre-alpha, a fix's timeline depends on its severity and is agreed with the reporter.
- We tell you when the fix is released, and credit you if you want to be credited.
- Fixed vulnerabilities are published as [GitHub security advisories](https://github.com/aparsons/oska/security/advisories).

## Supported versions

Oska is pre-alpha. Security fixes go into `main` and into the next release, and only the latest release is supported. If you are on an older release, update with the installer and check that the problem is still there before you report it.

## Scope

In scope:

- **Authority:** the checker accepts a program that reaches authority it was not given, for example a capability it never received, a file outside the directory capability it holds, or ambient authority of any kind. See [capabilities](docs/guide/capabilities.md).
- **Untrusted data:** an untrusted value reaches a structural position of a sink without going through a validator, or the checker accepts a program that lets it. See [untrusted data](docs/guide/untrusted-data.md).
- **The runtime:** an Oska program that does something the checker's authority report says it cannot.
- **Installing and releases:** a way to make the installer install a binary that the release key did not sign, an older release in place of a newer one, or anything over plain `http://`, or to get a release past the verification in the release workflows.
- **The site:** injection into a page of https://oskalang.dev, such as markup or script from a document's text.

Out of scope:

- Features the docs say are not built yet, such as the standard library modules that check but do not run.
- Vulnerabilities in GitHub, GitHub Pages or the font CDN themselves.
- Missing response headers that GitHub Pages cannot set.

## Verifying a release

Every release is signed with the release key, published at https://oskalang.dev/oska-release.pub and committed as `site/oska-release.pub`. Its fingerprint is:

```text
SHA256:PjwTDycs560g1ktWC/gdznamSkzm4xH+OAdfntNaW4o
```

The installers have this key built in. To check a release without them, see [verify a release by hand](docs/guide/install.md#verify-a-release-by-hand) in the install guide.
