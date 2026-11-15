//! The security policy and the release key's fingerprint (spec #183,
//! ticket #187).
//!
//! `SECURITY.md` at the repository root is the policy: the site generator
//! publishes it as `/security.html`, with a Markdown copy, and lists it in
//! the navigation as Security. The policy and the install guide print the
//! SHA-256 fingerprint of the committed release key,
//! `site/oska-release.pub`, so a reader can compare it with the key the
//! installers pin. These tests compute that fingerprint from the key
//! itself, so a rotation that leaves a stale fingerprint anywhere fails.
//!
//! What the deploy adds after the generator, `/.well-known/security.txt`
//! and `signing_key_fingerprint` in `latest.json`, is tested by
//! `scripts/release/test.sh`; here only the workflow's wiring is checked.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use oska_run::FakeHost;

/// The generator's source.
const BUILD: &str = include_str!("../../../site/build.osk");

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &str) -> String {
    let file = repo_root().join(path);
    std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("cannot read {}: {e}", file.display()))
}

/// The files every repository the generator reads must have.
const MINIMAL_REPO: &[(&str, &str)] = &[
    ("README.md", "# Oska\n"),
    ("GLOSSARY.md", "# Glossary\n"),
    ("docs/design.md", "# Design\n"),
    ("docs/guide/install.md", "# Install\n"),
    ("site/style.css", "body {}\n"),
    ("site/search.js", "// search\n"),
    (
        "Cargo.toml",
        "[workspace.package]\nversion = \"1.2.3\"\nlicense = \"Apache-2.0\"\n",
    ),
];

/// The generator, run on [`MINIMAL_REPO`] with `inputs` added: the files it
/// wrote under `_site/`, once it succeeded.
fn build(inputs: &[(&str, &str)]) -> BTreeMap<String, String> {
    let mut host = MINIMAL_REPO
        .iter()
        .chain(inputs)
        .fold(FakeHost::new(), |host, (path, text)| {
            host.with_file(path, text)
        });
    let outcome = common::run_differential(&mut host, BUILD, &[]);
    assert!(
        outcome.exit_code == 0 && outcome.diagnostics.is_empty() && outcome.panic.is_none(),
        "the generator should succeed: {outcome:#?}"
    );
    host.files_under("_site")
}

const POLICY: &str = "# Security policy\n\n\
    Report a vulnerability [privately](https://github.com/aparsons/oska/security/advisories/new).\n\n\
    See the [install guide](docs/guide/install.md).\n";

// ---- The policy page --------------------------------------------------------------------

#[test]
fn security_md_is_published_as_the_security_page_with_a_markdown_copy() {
    let site = build(&[("SECURITY.md", POLICY)]);
    let page = &site["security.html"];
    assert!(
        page.contains("<h1 id=\"security-policy\">Security policy</h1>"),
        "{page}"
    );
    assert!(
        page.contains(
            "<a href=\"https://github.com/aparsons/oska/security/advisories/new\">privately</a>"
        ),
        "{page}"
    );
    // Its relative links resolve from the repository root, where it lives.
    assert!(
        page.contains("<a href=\"/guide/install.html\">install guide</a>"),
        "{page}"
    );
    let copy = &site["security.md"];
    assert!(copy.starts_with("# Security policy\n"), "{copy}");
    assert!(
        copy.contains("[install guide](/guide/install.md)"),
        "{copy}"
    );
    assert!(site["sitemap.xml"].contains("<loc>https://oskalang.dev/security.html</loc>"));
    assert!(
        site["llms.txt"].contains("- [Security policy](https://oskalang.dev/security.md)"),
        "{}",
        site["llms.txt"]
    );
    assert!(site["llms-full.txt"].contains("Source: https://oskalang.dev/security.html\n"));
}

#[test]
fn every_page_lists_the_security_page_in_its_navigation() {
    let site = build(&[("SECURITY.md", POLICY)]);
    for name in ["index.html", "design.html", "glossary.html"] {
        assert!(
            site[name].contains("<li><a href=\"/security.html\">Security</a></li>"),
            "{name} has no Security link: {}",
            site[name]
        );
    }
    assert!(site["security.html"]
        .contains("<li><a href=\"/security.html\" aria-current=\"page\">Security</a></li>"));
}

#[test]
fn main_narrows_reading_the_policy_to_security_md_alone() {
    assert!(BUILD.contains("security: read.restrict_to_file(\"SECURITY.md\"),"));
    let report = oska_check::check_module(BUILD).authority_to_human("site/build.osk");
    assert!(
        report.contains("  FsRead -> FileRead: restrict_to_file \"SECURITY.md\" (bytes "),
        "{report}"
    );
}

#[test]
fn without_a_policy_there_is_no_security_page_or_link() {
    let site = build(&[]);
    assert!(!site.contains_key("security.html"));
    assert!(!site["index.html"].contains("/security.html"));
}

// ---- The real policy ----------------------------------------------------------------------

/// Where a reporter opens a private report: GitHub private vulnerability
/// reporting, the only channel.
const REPORT_URL: &str = "https://github.com/aparsons/oska/security/advisories/new";

#[test]
fn the_policy_says_how_to_report_what_is_in_scope_and_what_to_expect() {
    let policy = read("SECURITY.md");
    for heading in [
        "# Security policy\n",
        "\n## Reporting a vulnerability\n",
        "\n## What to expect\n",
        "\n## Supported versions\n",
        "\n## Scope\n",
        "\n## Verifying a release\n",
    ] {
        assert!(policy.contains(heading), "SECURITY.md has no {heading:?}");
    }
    for claim in [
        REPORT_URL,
        // Private reporting is the only channel.
        "Do not open a public issue",
        "within 7 days",
        "agreed with the reporter",
        "https://github.com/aparsons/oska/security/advisories",
        // Pre-alpha: only the latest release is supported.
        "only the latest release",
        // The in-scope classes.
        "authority it was not given",
        "untrusted value",
        "installer",
        "injection",
        // Curated reports, not unreviewed tool output.
        "unreviewed",
    ] {
        assert!(policy.contains(claim), "SECURITY.md does not say {claim:?}");
    }
    assert!(
        !policy.contains('@'),
        "SECURITY.md names an email address; private reporting is the only channel"
    );
}

// ---- The release key's fingerprint --------------------------------------------------------

/// The SHA-256 of `data` (FIPS 180-4).
fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut message = data.to_vec();
    let bits = (data.len() as u64) * 8;
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bits.to_be_bytes());
    for block in message.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [
                t1.wrapping_add(t2),
                v[0],
                v[1],
                v[2],
                v[3].wrapping_add(t1),
                v[4],
                v[5],
                v[6],
            ];
        }
        for (state, add) in h.iter_mut().zip(v) {
            *state = state.wrapping_add(add);
        }
    }
    let mut out = [0u8; 32];
    for (bytes, word) in out.chunks_mut(4).zip(h) {
        bytes.copy_from_slice(&word.to_be_bytes());
    }
    out
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_decode(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buffer = 0u32;
    let mut held = 0;
    for c in text.bytes().take_while(|&c| c != b'=') {
        let value = BASE64
            .iter()
            .position(|&b| b == c)
            .unwrap_or_else(|| panic!("{c:?} is not base64 in {text}"));
        buffer = (buffer << 6) | value as u32;
        held += 6;
        if held >= 8 {
            held -= 8;
            out.push((buffer >> held) as u8);
        }
    }
    out
}

/// `data` in base64, without the `=` padding, as `ssh-keygen -l` prints a
/// fingerprint.
fn base64_unpadded(data: &[u8]) -> String {
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let mut buffer = [0u8; 3];
        buffer[..chunk.len()].copy_from_slice(chunk);
        let n = u32::from_be_bytes([0, buffer[0], buffer[1], buffer[2]]);
        for i in 0..=chunk.len() {
            out.push(BASE64[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

/// The fingerprint `ssh-keygen -l -E sha256` prints for an OpenSSH public
/// key line: `SHA256:` and the unpadded base64 of the SHA-256 of the key
/// blob.
fn fingerprint(public_key: &str) -> String {
    let blob = public_key
        .split_whitespace()
        .nth(1)
        .unwrap_or_else(|| panic!("not an OpenSSH public key: {public_key}"));
    format!("SHA256:{}", base64_unpadded(&sha256(&base64_decode(blob))))
}

#[test]
fn the_fingerprint_is_computed_as_ssh_keygen_prints_it() {
    // FIPS 180-4's example for "abc".
    let abc: String = sha256(b"abc").iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(
        abc,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    // A throwaway key, and what `ssh-keygen -lf key.pub -E sha256` printed
    // for it.
    let key =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMdIkzizKGZyY0q5lMZqCH4JYPJCjSxOiLJZkdG+I2X5 test";
    assert_eq!(
        fingerprint(key),
        "SHA256:1VJY7NaBp8iRvUesl8O6MTs0VefYM0XNFkuK5clkTcQ"
    );
}

/// The fingerprints, `SHA256:` and 43 base64 characters, written in `text`.
fn fingerprints_in(text: &str) -> Vec<&str> {
    text.match_indices("SHA256:")
        .map(|(at, _)| {
            let rest = &text[at + 7..];
            let len = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '+' || c == '/'))
                .unwrap_or(rest.len());
            &text[at..at + 7 + len]
        })
        .filter(|f| f.len() == 7 + 43)
        .collect()
}

/// Every Markdown file under `dir`, relative to the repository root.
fn markdown_under(dir: &str) -> Vec<String> {
    let mut out = Vec::new();
    let entries = std::fs::read_dir(repo_root().join(dir))
        .unwrap_or_else(|e| panic!("cannot list {dir}: {e}"));
    for entry in entries {
        let name = entry
            .expect("a directory entry")
            .file_name()
            .to_string_lossy()
            .into_owned();
        let path = format!("{dir}/{name}");
        if repo_root().join(&path).is_dir() {
            out.extend(markdown_under(&path));
        } else if name.ends_with(".md") {
            out.push(path);
        }
    }
    out
}

#[test]
fn every_written_fingerprint_is_the_committed_release_key_s() {
    let key = fingerprint(&read("site/oska-release.pub"));
    // Where a reader verifies a release, the fingerprint is shown.
    for page in ["SECURITY.md", "docs/guide/install.md"] {
        assert!(
            fingerprints_in(&read(page)).contains(&key.as_str()),
            "{page} does not show the release key's fingerprint, {key}"
        );
    }
    // And no page shows any other: after a rotation, every copy is updated.
    let mut pages = vec!["SECURITY.md".to_string(), "README.md".to_string()];
    pages.extend(markdown_under("docs"));
    pages.extend(markdown_under("site/agent-setup"));
    let mut seen = 0;
    for page in &pages {
        for written in fingerprints_in(&read(page)) {
            seen += 1;
            assert_eq!(
                written, key,
                "{page} shows {written}, but site/oska-release.pub is {key}"
            );
        }
    }
    assert!(seen >= 3, "only {seen} fingerprints were found");
}

// ---- What the deploy adds -----------------------------------------------------------------

const PAGES_WORKFLOW: &str = include_str!("../../../.github/workflows/pages.yml");

/// The deploy writes security.txt into the built site before it uploads
/// it, and the post-deploy check fails on a missing or expired one.
#[test]
fn the_pages_workflow_writes_security_txt_and_checks_it_after_the_deploy() {
    let position = |text: &str| {
        PAGES_WORKFLOW
            .find(text)
            .unwrap_or_else(|| panic!("pages.yml does not run `{text}`"))
    };
    let generate = position("./target/release/oskac run site/build.osk");
    let write = position("scripts/release/security-txt.sh _site");
    let upload = position("actions/upload-pages-artifact");
    let smoke = position("\n  smoke:\n");
    let check = position("scripts/release/check-security-txt.sh https://oskalang.dev");
    assert!(
        generate < write && write < upload,
        "security.txt is not written into the upload"
    );
    assert!(
        smoke < check,
        "security.txt is not checked after the deploy"
    );
    // The policy page it names is published, and checked for too.
    assert!(PAGES_WORKFLOW.contains(" security.md"));
}
