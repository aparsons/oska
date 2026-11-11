# Packages are git URLs, resolved by minimal version selection

## Decision

A **package** is a set of modules published from one git repository, and its name is the repository URL. There is no registry.

- **Modules.** One file is one module. `use pkg.a.b` names this package's file `src/a/b.osk`, `use <alias>.x` names a dependency under the alias its manifest gives it, and `use std.x` names the standard library. `pkg` and `std` are reserved, so the first segment of every path says where a name comes from.
- **Visibility.** Items are private by default. `pub(pkg)` makes an item visible to every module of its package. `pub` makes it part of the package's API, which is the only part a package's authority is counted over.
- **Manifest.** `oska.toml`, a strict TOML subset: tables, strings, arrays and comments, with no dotted keys, dates or multiline strings. Oska parses it with its own parser. It names each direct dependency by URL and minimum version tag, and records the authority approved for it (#12).
- **Versions.** Version tags are semver. Resolution is **minimal version selection**: one copy per major version, and the build takes the highest of the minimum versions that anything in the graph requires.
- **Lockfile.** It records, for every package in the graph, the version, the commit, a SHA-256 over a canonical listing of the package's files (path, mode, contents), and the computed authority. The SHA-256 is the integrity check. The commit ID, a SHA-1, is for people and for fetching again.
- **No package code runs while fetching or building.** There are no build scripts, install hooks or code generators. Only the toolchain acts.
- **Private repositories.** A token per host comes from `~/.config/oska/credentials.toml`, which must be mode 0600, or from an `OSKA_TOKEN_<HOST>` environment variable. A token is a `Secret<String>`, is sent only to its own host, and is never written to the manifest or lockfile.

## Why

A package named by its URL can't have its name squatted or typosquatted on a registry, and nobody has to run or secure a registry. Minimal version selection is deterministic even without a lockfile and needs no SAT solver. It never picks up a release that nobody asked for, so a **widening** of a dependency's authority appears only when someone edits a requirement, which is exactly when it should be reviewed. Build scripts and install hooks are the most common way a malicious package runs code on a developer's machine, and they contradict the design principle that no code runs when a module loads.

## Considered options

- **Local modules first, packages later.** Rejected: the whole package design is settled now, so modules and packages are designed together. Modules still ship first (see the milestones in `docs/design.md`).
- **A central registry** (crates.io, npm). Rejected: short names and discovery don't outweigh running a registry, name squatting, and typosquatting.
- **Newest compatible version** (Cargo). Rejected: an update can silently pull in new code and widenings.
- **Several copies of one package** (npm). Rejected: duplicate types, and authority counted many times over.
- **Manifest as Oska code, JSON, or a go.mod-style format.** Rejected: Oska code tempts agents to put logic in it, JSON has no comments for an approval to explain itself, and a bespoke format is one agents have never seen.
- **`pub` alone, or an export list in the manifest.** Rejected: internal helpers would inflate a package's authority, or the API would be defined far from the code.

## Consequences

- Fetching needs TLS and git, and Oska implements both itself ([ADR 0012](0012-tls-git-and-crypto-are-written-in-oska.md)), so the package manager ships after the native backend.
- Version tags must be semver, and a new major version is a new package path.
