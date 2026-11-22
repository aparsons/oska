# Compliance FAQ

The Oska project answers only what the repository can prove, and each answer links its evidence. The reports in the [Compliance section](index.md) are self-assessments, not the verdict of a third party, and [how they are written](template.md) explains the ratings. A determination of Conforms can sit beside a Low-rated unmet requirement, so Conforms does not mean every clause is met.

## What is Oska's license?

Oska is licensed under the Apache License, Version 2.0: see [LICENSE](https://github.com/aparsons/oska/blob/main/LICENSE) and the `license` field in [Cargo.toml](https://github.com/aparsons/oska/blob/main/Cargo.toml). The [NOTICE](https://github.com/aparsons/oska/blob/main/NOTICE) file carries the attribution that section 4 of the license asks redistributions to keep. A contribution is licensed on the same terms under section 5, as [CONTRIBUTING.md](https://github.com/aparsons/oska/blob/main/CONTRIBUTING.md) says.

## Does Oska publish an SBOM?

Not yet. A release does not list an SBOM among its assets: the files it ships are the ones in [`SHA256SUMS`](https://github.com/aparsons/oska/releases/latest), and the [release runbook](https://github.com/aparsons/oska/blob/main/docs/agents/release.md) lists them. The Rust dependencies the binaries are built from are pinned in [Cargo.lock](https://github.com/aparsons/oska/blob/main/Cargo.lock). Generating an SPDX SBOM for each release is planned, and the SPDX report will judge it.

## Are releases signed?

Yes. Every release is signed with the release key, which is published at [oskalang.dev/oska-release.pub](https://oskalang.dev/oska-release.pub) and committed as [site/oska-release.pub](https://github.com/aparsons/oska/blob/main/site/oska-release.pub). The installers have the key built in. To check a release by hand, follow [verify a release by hand](../guide/install.md#verify-a-release-by-hand), and see [Verifying a release](../../SECURITY.md#verifying-a-release) in the security policy for the key's fingerprint.

## Does Oska publish build provenance?

Not yet. The release workflow, [release.yml](https://github.com/aparsons/oska/blob/main/.github/workflows/release.yml), builds the binaries on GitHub-hosted runners and signs `SHA256SUMS`, but it does not publish a signed provenance statement, so the project claims no SLSA Build level for releases today. Release provenance is tracked in [issue 295](https://github.com/aparsons/oska/issues/295), and the SLSA report will judge it.

## How do I report a vulnerability?

Use GitHub private vulnerability reporting: [open a private report](https://github.com/aparsons/oska/security/advisories/new). It is the only channel; do not open a public issue for a vulnerability. The [security policy](../../SECURITY.md) says what to include and what the project does in reply, and fixed vulnerabilities are published as [security advisories](https://github.com/aparsons/oska/security/advisories).

## Can the project answer a legal or export-control question?

No. The Oska project gives no legal advice, and it gives no export-control classification of Oska. The facts the repository can show are the [license](https://github.com/aparsons/oska/blob/main/LICENSE) and that the TLS, git and crypto code is written in Oska itself, as [ADR 0012](../adr/0012-tls-git-and-crypto-are-written-in-oska.md) records. If your question turns on what those facts mean for you, consult your own counsel.
