//! `std.sshsig` (#234) as programs run: Oska source linked into the
//! program, with `std.sha2` and `std.ed25519`, which it imports, and run
//! as Oska by both backends. The signatures are fixtures made by
//! `ssh-keygen -Y sign` (see `fixtures/sshsig/generate.sh`; no test runs
//! ssh-keygen) and the `SHA256SUMS.sig` of every published release. Each
//! verifies, and is then refused with the error that names what changed.
//! One program (`fixtures/sshsig_cases.osk`) reads every case from stdin,
//! so each backend runs it once, and the differential harness requires
//! the same output from both.

#![cfg(any(
    all(
        any(target_os = "linux", target_os = "macos"),
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(windows, target_arch = "x86_64")
))]

mod common;
mod differential;

use differential::{assert_same, Case};

const PROGRAM: &str = include_str!("fixtures/sshsig_cases.osk");

const TEST_KEY: &str = include_str!("fixtures/sshsig/test_key.pub");
const OTHER_KEY: &str = include_str!("fixtures/sshsig/other_key.pub");
const MESSAGE: &[u8] = include_bytes!("fixtures/sshsig/message.txt");
const SIG_SHA512: &str = include_str!("fixtures/sshsig/message.sha512.sig");
const SIG_SHA256: &str = include_str!("fixtures/sshsig/message.sha256.sig");
const RELEASE_KEY: &str = include_str!("../../../site/oska-release.pub");
const RUNBOOK: &str = include_str!("../../../docs/agents/release.md");

/// `ssh-keygen -l` on the test key, the other key and the release key.
const TEST_FINGERPRINT: &str = "SHA256:tMdvoGUi4kC3M8Dqo5IUHM85ExaZ7J1kAQhArgI0T9I";
const OTHER_FINGERPRINT: &str = "SHA256:Fz5ibo7tcVW+O0Z1qjMjZA4zwhzSm7t+JbU7UV2N3K8";
const RELEASE_FINGERPRINT: &str = "SHA256:PjwTDycs560g1ktWC/gdznamSkzm4xH+OAdfntNaW4o";

/// Every published release, with its `SHA256SUMS` and `SHA256SUMS.sig`.
const RELEASES: [(&str, &[u8], &str); 6] = [
    (
        "v0.1.0",
        include_bytes!("fixtures/sshsig/releases/v0.1.0/SHA256SUMS"),
        include_str!("fixtures/sshsig/releases/v0.1.0/SHA256SUMS.sig"),
    ),
    (
        "v0.2.0",
        include_bytes!("fixtures/sshsig/releases/v0.2.0/SHA256SUMS"),
        include_str!("fixtures/sshsig/releases/v0.2.0/SHA256SUMS.sig"),
    ),
    (
        "v0.3.0",
        include_bytes!("fixtures/sshsig/releases/v0.3.0/SHA256SUMS"),
        include_str!("fixtures/sshsig/releases/v0.3.0/SHA256SUMS.sig"),
    ),
    (
        "v0.4.0",
        include_bytes!("fixtures/sshsig/releases/v0.4.0/SHA256SUMS"),
        include_str!("fixtures/sshsig/releases/v0.4.0/SHA256SUMS.sig"),
    ),
    (
        "v0.5.0",
        include_bytes!("fixtures/sshsig/releases/v0.5.0/SHA256SUMS"),
        include_str!("fixtures/sshsig/releases/v0.5.0/SHA256SUMS.sig"),
    ),
    (
        "v0.6.0",
        include_bytes!("fixtures/sshsig/releases/v0.6.0/SHA256SUMS"),
        include_str!("fixtures/sshsig/releases/v0.6.0/SHA256SUMS.sig"),
    ),
];

// ---- Building cases ---------------------------------------------------------------------

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(data: &[u8]) -> String {
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..=chunk.len() {
            out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
        for _ in chunk.len()..3 {
            out.push('=');
        }
    }
    out
}

fn base64_decode(text: &str) -> Vec<u8> {
    let mut bits = 0u32;
    let mut have = 0;
    let mut out = Vec::new();
    for c in text.bytes().filter(|c| *c != b'=') {
        let v = ALPHABET.iter().position(|a| *a == c).expect("base64") as u32;
        bits = (bits << 6) | v;
        have += 6;
        if have >= 8 {
            have -= 8;
            out.push((bits >> have) as u8);
            bits &= (1 << have) - 1;
        }
    }
    out
}

/// The wire bytes of an armored signature.
fn wire_of(armored: &str) -> Vec<u8> {
    let body: String = armored
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .collect();
    base64_decode(&body)
}

/// Wire bytes in armor, wrapped at `width` characters, lines ended by `eol`.
fn armor(wire: &[u8], width: usize, eol: &str) -> String {
    let text = base64_encode(wire);
    let mut out = format!("-----BEGIN SSH SIGNATURE-----{eol}");
    for line in text.as_bytes().chunks(width) {
        out.push_str(std::str::from_utf8(line).expect("a well-formed fixture"));
        out.push_str(eol);
    }
    out.push_str(&format!("-----END SSH SIGNATURE-----{eol}"));
    out
}

fn armored(wire: &[u8]) -> String {
    armor(wire, 70, "\n")
}

/// The fields of SSHSIG wire bytes, as (offset of the length, length): the
/// public key, namespace, reserved, hash algorithm and signature.
fn fields(wire: &[u8]) -> Vec<(usize, usize)> {
    let mut at = 10;
    let mut out = Vec::new();
    for _ in 0..5 {
        let len = u32::from_be_bytes(wire[at..at + 4].try_into().expect("a well-formed fixture"))
            as usize;
        out.push((at, len));
        at += 4 + len;
    }
    assert_eq!(at, wire.len(), "the fixture has no trailing bytes");
    out
}

const KEY: usize = 0;
const NAMESPACE: usize = 1;
const RESERVED: usize = 2;
const HASH: usize = 3;
const SIGNATURE: usize = 4;

/// `wire` with field `which` replaced by `value`, its length fixed.
fn with_field(wire: &[u8], which: usize, value: &[u8]) -> Vec<u8> {
    let (at, len) = fields(wire)[which];
    let mut out = wire[..at].to_vec();
    out.extend((value.len() as u32).to_be_bytes());
    out.extend(value);
    out.extend(&wire[at + 4 + len..]);
    out
}

/// `wire` with the length of field `which` set to `len`, nothing else
/// changed.
fn with_length(wire: &[u8], which: usize, len: u32) -> Vec<u8> {
    let (at, _) = fields(wire)[which];
    let mut out = wire.to_vec();
    out[at..at + 4].copy_from_slice(&len.to_be_bytes());
    out
}

/// The key's 32 bytes, from an OpenSSH key line.
fn key_bytes(line: &str) -> Vec<u8> {
    let wire = base64_decode(line.split(' ').nth(1).expect("a well-formed fixture"));
    wire[wire.len() - 32..].to_vec()
}

/// What a case should give: the verdict `sshsig.verify` prints first, or
/// the line `key <error>` of a key line refused.
enum Expect {
    Verdict(&'static str),
    Key(&'static str),
}

struct Cases {
    labels: Vec<String>,
    expects: Vec<Expect>,
    stdin: String,
}

impl Cases {
    fn new() -> Cases {
        Cases {
            labels: Vec::new(),
            expects: Vec::new(),
            stdin: String::new(),
        }
    }

    /// A verification: key line, namespace, armored signature, message.
    fn verify(
        &mut self,
        label: &str,
        key: &str,
        namespace: &str,
        signature: &str,
        message: &[u8],
        expect: &'static str,
    ) {
        let message_hex: String = message.iter().map(|b| format!("{b:02x}")).collect();
        let signature = signature.replace('\n', "|").replace('\r', "^");
        self.stdin.push_str(&format!(
            "{}\n{namespace}\n{signature}\n{message_hex}\n---\n",
            key.trim_end()
        ));
        self.labels.push(label.to_string());
        self.expects.push(Expect::Verdict(expect));
    }

    /// A key line that should be refused.
    fn key(&mut self, label: &str, line: &str, error: &'static str) {
        self.stdin
            .push_str(&format!("{line}\noska-test\n{}\n00\n---\n", "x"));
        self.labels.push(label.to_string());
        self.expects.push(Expect::Key(error));
    }

    /// Runs them on both backends and checks each line.
    fn run(&self, name: &str) -> Vec<String> {
        let outcome = assert_same(&Case::file(name, PROGRAM).with_stdin(&self.stdin));
        assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
        let lines: Vec<String> = outcome.stdout.lines().map(str::to_string).collect();
        assert_eq!(
            lines.len(),
            self.labels.len(),
            "one line a case: {lines:#?}"
        );
        for ((line, label), expect) in lines.iter().zip(&self.labels).zip(&self.expects) {
            match expect {
                Expect::Verdict(verdict) => {
                    assert_eq!(line.split(' ').next(), Some(*verdict), "{label}: {line}")
                }
                Expect::Key(error) => assert_eq!(line, &format!("key {error}"), "{label}"),
            }
        }
        lines
    }
}

// ---- The tests --------------------------------------------------------------------------

#[test]
fn signatures_made_by_ssh_keygen_verify_and_tampering_is_refused() {
    let mut cases = Cases::new();
    for (label, sig) in [("sha512", SIG_SHA512), ("sha256", SIG_SHA256)] {
        let wire = wire_of(sig);
        let v = |cases: &mut Cases, what: &str, wire: &[u8], expect| {
            cases.verify(
                &format!("{label} {what}"),
                TEST_KEY,
                "oska-test",
                &armored(wire),
                MESSAGE,
                expect,
            )
        };
        cases.verify(
            &format!("{label} as written"),
            TEST_KEY,
            "oska-test",
            sig,
            MESSAGE,
            "ok",
        );
        // Every change the format allows, but the signature does not
        // cover.
        for (what, text) in [
            ("CRLF", armor(&wire, 70, "\r\n")),
            ("one long line", armor(&wire, 1000, "\n")),
            ("short lines", armor(&wire, 16, "\n")),
            (
                "blank lines around",
                format!("\n\r\n{}\n\n", armored(&wire)),
            ),
            ("no final newline", armored(&wire).trim_end().to_string()),
        ] {
            cases.verify(
                &format!("{label} {what}"),
                TEST_KEY,
                "oska-test",
                &text,
                MESSAGE,
                "ok",
            );
        }
        // The message, the namespace and the key.
        let mut changed = MESSAGE.to_vec();
        changed[0] ^= 1;
        cases.verify(
            &format!("{label} message"),
            TEST_KEY,
            "oska-test",
            sig,
            &changed,
            "BadSignature",
        );
        cases.verify(
            &format!("{label} empty message"),
            TEST_KEY,
            "oska-test",
            sig,
            b"",
            "BadSignature",
        );
        cases.verify(
            &format!("{label} namespace"),
            TEST_KEY,
            "oska-index",
            sig,
            MESSAGE,
            "WrongNamespace",
        );
        cases.verify(
            &format!("{label} namespace prefix"),
            TEST_KEY,
            "oska-tes",
            sig,
            MESSAGE,
            "WrongNamespace",
        );
        cases.verify(
            &format!("{label} key"),
            OTHER_KEY,
            "oska-test",
            sig,
            MESSAGE,
            "WrongKey",
        );
        // The key comes first: a wrong key and a wrong namespace is a
        // wrong key.
        cases.verify(
            &format!("{label} key and namespace"),
            OTHER_KEY,
            "oska-index",
            sig,
            MESSAGE,
            "WrongKey",
        );
        // The comment on the key line is not part of the key.
        cases.verify(
            &format!("{label} no comment"),
            TEST_KEY
                .split(' ')
                .take(2)
                .collect::<Vec<_>>()
                .join(" ")
                .as_str(),
            "oska-test",
            sig,
            MESSAGE,
            "ok",
        );
        cases.verify(
            &format!("{label} another comment"),
            &format!(
                "{} some other label\n",
                TEST_KEY
                    .trim_end()
                    .rsplit_once(' ')
                    .expect("a well-formed fixture")
                    .0
            ),
            "oska-test",
            sig,
            MESSAGE,
            "ok",
        );
        // The embedded key replaced by another valid one.
        let mut other = wire[fields(&wire)[KEY].0 + 4..][..fields(&wire)[KEY].1].to_vec();
        let n = other.len();
        other[n - 32..].copy_from_slice(&key_bytes(OTHER_KEY));
        let swapped = with_field(&wire, KEY, &other);
        v(
            &mut cases,
            "embedded other key, test key asked",
            &swapped,
            "WrongKey",
        );
        cases.verify(
            &format!("{label} embedded other key, other key asked"),
            OTHER_KEY,
            "oska-test",
            &armored(&swapped),
            MESSAGE,
            "BadSignature",
        );
        // A signature by another key whose key is no point of the curve is
        // refused as another key's, before any curve arithmetic: the key
        // is compared as bytes first.
        let mut not_a_point = other.clone();
        let n = not_a_point.len();
        not_a_point[n - 32..].fill(0);
        not_a_point[n - 32] = 2; // y = 2 has no x on the curve
        v(
            &mut cases,
            "embedded key not a point",
            &with_field(&wire, KEY, &not_a_point),
            "WrongKey",
        );
        // An empty namespace is refused, as OpenSSH refuses it, whether it
        // is asked for or signed.
        cases.verify(
            &format!("{label} empty namespace asked"),
            TEST_KEY,
            "",
            sig,
            MESSAGE,
            "WrongNamespace",
        );
        let empty_signed = armored(&with_field(&wire, NAMESPACE, b""));
        for asked in ["", "oska-test"] {
            cases.verify(
                &format!("{label} empty namespace signed, {asked:?} asked"),
                TEST_KEY,
                asked,
                &empty_signed,
                MESSAGE,
                "WrongNamespace",
            );
        }
        // The embedded key's type, and its length.
        let mut kind = other.clone();
        kind[10] = b'8';
        v(
            &mut cases,
            "embedded key type",
            &with_field(&wire, KEY, &kind),
            "UnsupportedAlgorithm",
        );
        v(
            &mut cases,
            "embedded key short",
            &with_field(&wire, KEY, &other[..n - 1]),
            "Malformed",
        );
        let mut longer = other.clone();
        longer.push(0);
        v(
            &mut cases,
            "embedded key long",
            &with_field(&wire, KEY, &longer),
            "Malformed",
        );
        // The namespace in the signature.
        v(
            &mut cases,
            "wire namespace",
            &with_field(&wire, NAMESPACE, b"oska-tesu"),
            "WrongNamespace",
        );
        cases.verify(
            &format!("{label} wire namespace asked for"),
            TEST_KEY,
            "oska-tesu",
            &armored(&with_field(&wire, NAMESPACE, b"oska-tesu")),
            MESSAGE,
            "BadSignature",
        );
        // The reserved field is signed, so it cannot be changed.
        v(
            &mut cases,
            "reserved",
            &with_field(&wire, RESERVED, b"x"),
            "BadSignature",
        );
        // The hash algorithm's name.
        v(
            &mut cases,
            "hash name same length",
            &with_field(&wire, HASH, b"sha513"),
            "UnsupportedAlgorithm",
        );
        v(
            &mut cases,
            "hash name md5",
            &with_field(&wire, HASH, b"md5"),
            "UnsupportedAlgorithm",
        );
        v(
            &mut cases,
            "hash name empty",
            &with_field(&wire, HASH, b""),
            "UnsupportedAlgorithm",
        );
        let other_hash: &[u8] = if label == "sha512" {
            b"sha256"
        } else {
            b"sha512"
        };
        v(
            &mut cases,
            "hash name swapped",
            &with_field(&wire, HASH, other_hash),
            "BadSignature",
        );
        // The signature.
        let (at, len) = fields(&wire)[SIGNATURE];
        let inner = wire[at + 4..at + 4 + len].to_vec();
        let mut kind = inner.clone();
        kind[10] = b'8';
        v(
            &mut cases,
            "signature type",
            &with_field(&wire, SIGNATURE, &kind),
            "UnsupportedAlgorithm",
        );
        v(
            &mut cases,
            "signature short",
            &with_field(&wire, SIGNATURE, &inner[..len - 1]),
            "Malformed",
        );
        let mut longer = inner.clone();
        longer.push(0);
        v(
            &mut cases,
            "signature trailing",
            &with_field(&wire, SIGNATURE, &longer),
            "Malformed",
        );
        for byte in [len - 64, len - 33, len - 32, len - 1] {
            let mut flipped = wire.clone();
            flipped[at + 4 + byte] ^= 0x10;
            v(
                &mut cases,
                &format!("signature byte {byte}"),
                &flipped,
                "BadSignature",
            );
        }
        // The version.
        for version in [0u32, 2, 0x0100_0000, u32::MAX] {
            let mut wire = wire.clone();
            wire[6..10].copy_from_slice(&version.to_be_bytes());
            v(
                &mut cases,
                &format!("version {version}"),
                &wire,
                "UnsupportedVersion",
            );
        }
        // The magic.
        let mut magic = wire.clone();
        magic[5] = b'X';
        v(&mut cases, "magic", &magic, "Malformed");
        // Trailing bytes.
        let mut trailing = wire.clone();
        trailing.push(0);
        v(&mut cases, "trailing byte", &trailing, "Malformed");
        // Every length field, made longer than what remains, one more or
        // one less.
        for which in 0..5 {
            let (_, len) = fields(&wire)[which];
            v(
                &mut cases,
                &format!("length {which} huge"),
                &with_length(&wire, which, u32::MAX),
                "Malformed",
            );
            v(
                &mut cases,
                &format!("length {which} 2^31"),
                &with_length(&wire, which, 1 << 31),
                "Malformed",
            );
            v(
                &mut cases,
                &format!("length {which} plus one"),
                &with_length(&wire, which, len as u32 + 1),
                "Malformed",
            );
        }
        // Cut short anywhere.
        for cut in 0..wire.len() {
            v(
                &mut cases,
                &format!("cut at {cut}"),
                &wire[..cut],
                "Malformed",
            );
        }
        // The armor.
        let text = armored(&wire);
        let body: Vec<&str> = text.lines().collect();
        let end = body.len() - 1;
        let join = |lines: &[&str]| format!("{}\n", lines.join("\n"));
        let mut broken_char = body.iter().map(|l| l.to_string()).collect::<Vec<_>>();
        broken_char[1].replace_range(3..4, "*");
        let mut blank_inside = body.clone();
        blank_inside.insert(2, "");
        let mut space_after = body.iter().map(|l| l.to_string()).collect::<Vec<_>>();
        space_after[1].push(' ');
        let mut unpadded = body.iter().map(|l| l.to_string()).collect::<Vec<_>>();
        let last = unpadded[end - 1].clone();
        unpadded[end - 1] = last.trim_end_matches('=').to_string();
        let mut lone_cr = text.replace('\n', "\r");
        lone_cr.push('\n');
        for (what, text) in [
            ("no end", join(&body[..end])),
            ("no begin", join(&body[1..])),
            ("empty", String::new()),
            ("begin only", join(&body[..1])),
            (
                "wrong begin",
                text.replace("SSH SIGNATURE-----\n", "SSH SIGNATURF-----\n"),
            ),
            ("wrong end", text.replace("-----END SSH", "-----END SSX")),
            ("text before", format!("hello\n{text}")),
            ("text after", format!("{text}hello\n")),
            ("two signatures", format!("{text}{text}")),
            (
                "char outside the alphabet",
                join(&broken_char.iter().map(String::as_str).collect::<Vec<_>>()),
            ),
            ("blank line inside", join(&blank_inside)),
            (
                "space after a line",
                join(&space_after.iter().map(String::as_str).collect::<Vec<_>>()),
            ),
            (
                "padding removed",
                join(&unpadded.iter().map(String::as_str).collect::<Vec<_>>()),
            ),
            ("lone CR line ends", lone_cr),
            ("PEM label", text.replace("SSH SIGNATURE", "SIGNATURE")),
        ] {
            cases.verify(
                &format!("{label} armor: {what}"),
                TEST_KEY,
                "oska-test",
                &text,
                MESSAGE,
                "Armor",
            );
        }
    }
    cases.run("sshsig_ssh_keygen");
}

#[test]
fn every_published_release_verifies_against_the_release_key() {
    let mut cases = Cases::new();
    for (tag, sums, sig) in RELEASES {
        cases.verify(
            &format!("{tag} verifies"),
            RELEASE_KEY,
            "oska-release",
            sig,
            sums,
            "ok",
        );
        cases.verify(
            &format!("{tag} in the index namespace"),
            RELEASE_KEY,
            "oska-index",
            sig,
            sums,
            "WrongNamespace",
        );
        cases.verify(
            &format!("{tag} under another key"),
            TEST_KEY,
            "oska-release",
            sig,
            sums,
            "WrongKey",
        );
        let mut changed = sums.to_vec();
        let last = changed.len() - 2;
        changed[last] ^= 1;
        cases.verify(
            &format!("{tag} with a changed digest"),
            RELEASE_KEY,
            "oska-release",
            sig,
            &changed,
            "BadSignature",
        );
        // Another release's sums, which have another tag line.
        let other = RELEASES[(RELEASES
            .iter()
            .position(|r| r.0 == tag)
            .expect("a well-formed fixture")
            + 1)
            % 6]
        .1;
        cases.verify(
            &format!("{tag} with another release's sums"),
            RELEASE_KEY,
            "oska-release",
            sig,
            other,
            "BadSignature",
        );
    }
    let lines = cases.run("sshsig_releases");
    for line in lines.iter().step_by(5) {
        assert!(
            line.starts_with(&format!(
                "ok {RELEASE_FINGERPRINT} oska-release oska-release sha512 "
            )),
            "the release key, its comment and the signature's claims: {line}"
        );
    }
}

#[test]
fn fingerprints_match_ssh_keygen_and_the_runbook() {
    let mut cases = Cases::new();
    cases.verify("test key", TEST_KEY, "oska-test", SIG_SHA512, MESSAGE, "ok");
    cases.verify(
        "other key",
        OTHER_KEY,
        "oska-test",
        SIG_SHA512,
        MESSAGE,
        "WrongKey",
    );
    cases.verify(
        "release key",
        RELEASE_KEY,
        "oska-test",
        SIG_SHA512,
        MESSAGE,
        "WrongKey",
    );
    let lines = cases.run("sshsig_fingerprints");
    // The verdict, the fingerprint and the comment of the key, then what
    // the signature claims.
    assert_eq!(
        lines[0],
        format!("ok {TEST_FINGERPRINT} oska-test-key oska-test sha512 {TEST_FINGERPRINT}")
    );
    assert!(
        lines[1].starts_with(&format!(
            "WrongKey {OTHER_FINGERPRINT} other-key oska-test sha512 {TEST_FINGERPRINT}"
        )),
        "{}",
        lines[1]
    );
    assert!(
        lines[2].starts_with(&format!(
            "WrongKey {RELEASE_FINGERPRINT} oska-release oska-test"
        )),
        "{}",
        lines[2]
    );
    assert!(
        RUNBOOK.contains(RELEASE_FINGERPRINT),
        "the release runbook records the release key's fingerprint"
    );
}

#[test]
fn key_lines_are_read_strictly() {
    let good = TEST_KEY.trim_end();
    let wire = base64_decode(good.split(' ').nth(1).expect("a well-formed fixture"));
    let line = |wire: &[u8]| format!("ssh-ed25519 {} x", base64_encode(wire));
    let mut cases = Cases::new();
    cases.verify("as written", good, "oska-test", SIG_SHA512, MESSAGE, "ok");
    cases.verify(
        "a comment of several words",
        &format!("{good} and more words"),
        "oska-test",
        SIG_SHA512,
        MESSAGE,
        "ok",
    );
    cases.key(
        "rsa",
        "ssh-rsa AAAAB3NzaC1yc2E= comment",
        "UnsupportedAlgorithm",
    );
    cases.key(
        "ecdsa",
        "ecdsa-sha2-nistp256 AAAA comment",
        "UnsupportedAlgorithm",
    );
    cases.key(
        "sk key",
        "sk-ssh-ed25519@openssh.com AAAA comment",
        "UnsupportedAlgorithm",
    );
    cases.key("empty", "", "Malformed");
    cases.key("the type only", "ssh-ed25519", "Malformed");
    cases.key("not base64", "ssh-ed25519 !!!! x", "Malformed");
    cases.key(
        "unpadded",
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJkiHKcB7olfSbFctH77PhH1wdg/QVxvI544xAB2AS+g= x",
        "Malformed",
    );
    cases.key(
        "a tab",
        "ssh-ed25519\tAAAAC3NzaC1lZDI1NTE5AAAAIJkiHKcB7olfSbFctH77PhH1wdg/QVxvI544xAB2AS+g x",
        "UnsupportedAlgorithm",
    );
    cases.key(
        "a run of spaces",
        "ssh-ed25519  AAAAC3NzaC1lZDI1NTE5AAAAIJkiHKcB7olfSbFctH77PhH1wdg/QVxvI544xAB2AS+g x",
        "Malformed",
    );
    cases.key("empty wire", "ssh-ed25519  x", "Malformed");
    let mut other_type = wire.clone();
    other_type[14] = b'8';
    cases.key("wire of another type", &line(&other_type), "Malformed");
    cases.key("short key", &line(&wire[..wire.len() - 1]), "Malformed");
    let mut trailing = wire.clone();
    trailing.push(0);
    cases.key("trailing byte", &line(&trailing), "Malformed");
    cases.key(
        "truncated at the length",
        &line(&wire[..wire.len() - 34]),
        "Malformed",
    );
    let mut huge = wire.clone();
    huge[0..4].copy_from_slice(&u32::MAX.to_be_bytes());
    cases.key("huge length", &line(&huge), "Malformed");
    let mut not_a_point = wire.clone();
    let n = not_a_point.len();
    not_a_point[n - 32..].fill(0);
    not_a_point[n - 32] = 2; // y = 2 has no x on the curve
    cases.key("not a point", &line(&not_a_point), "Malformed");
    for cut in 0..wire.len() {
        cases.key(&format!("cut at {cut}"), &line(&wire[..cut]), "Malformed");
    }
    cases.run("sshsig_key_lines");
}

/// `SshsigError` is `Display`, one stable line a case, the same on both
/// backends.
#[test]
fn every_error_has_one_stable_line() {
    let src = r#"use std.sshsig

fn main(io: Stdout) -> Result<(), IoError> {
    io.println(sshsig.SshsigError.Armor.to_string())?
    io.println(sshsig.SshsigError.Malformed.to_string())?
    io.println(sshsig.SshsigError.UnsupportedVersion.to_string())?
    io.println(sshsig.SshsigError.UnsupportedAlgorithm.to_string())?
    io.println(sshsig.SshsigError.WrongKey.to_string())?
    io.println(sshsig.SshsigError.WrongNamespace.to_string())?
    io.println(sshsig.SshsigError.BadSignature.to_string())
}
"#;
    let outcome = assert_same(&Case::file("sshsig_display", src));
    assert_eq!(
        outcome.stdout,
        "the signature armor is malformed\n\
         the key or signature data is malformed\n\
         the signature version is not supported\n\
         the key, signature or hash algorithm is not supported\n\
         the signature was made by a different key\n\
         the signature was made for a different namespace\n\
         the signature does not verify\n",
        "{outcome:#?}"
    );
}
