// SPDX-License-Identifier: Apache-2.0
//! `std.tls`'s engine against RFC 8448, section 3 (#695 of #659): the
//! "Simple 1-RTT Handshake" trace, replayed record by record. Every record
//! the engine sends must be the RFC's, byte for byte (the ClientHello, the
//! client's Finished, its application data and its close_notify), and every
//! record the RFC's server sends must be accepted: the key schedule, the
//! record protection and the transcript hash have to agree with the RFC's.
//!
//! The engine's own ClientHello differs from the RFC's (its random, session
//! ID, key share, extensions and the compatibility change_cipher_spec come
//! from the engine's choices), so the test builds a copy of the module with a
//! test-only hook: a few replaced lines that fix the client random, the
//! session ID and the x25519 key to the RFC's, and the cipher suites and the
//! extensions around the server name and the key share to the RFC's list. The
//! engine's own `client_hello` still builds the message, the server name and
//! the key share included. The copy is the package's own `tls` module (as
//! `native_tls.rs` does to change the rekey limits). `std.tls` has no hook,
//! and a program cannot reach one; `crates/oska-check/tests/std_tls_rfc8448_hook.rs`
//! holds that.
//!
//! Proof: `cargo test -p oskac --test tls_rfc8448` and
//! `cargo test -p oska-check --test std_tls_rfc8448_hook`.
//!
//! The RFC's server is a 1024-bit RSA key for the name `rsa`, which `std.rsa`
//! (2048 bits and up) and the host name check refuse. So the package also
//! holds copies of `rsa` and `x509` that take a 1024-bit key, and the copy of
//! `tls` skips the certificate path and host name check (`verify_server`).
//! `every_crafted_chain_natively` in `native_x509.rs` holds that check. The CertificateVerify signature (RSA-PSS) is
//! still checked, so is every Finished and every record's tag.

#![cfg(any(
    all(
        any(target_os = "linux", target_os = "macos"),
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(windows, target_arch = "x86_64")
))]

mod common;
mod differential;

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use differential::{assert_same, build_native, in_parallel, Case};

const REPLAY: &str = include_str!("tls/rfc8448.osk");
const TRACE: &str = include_str!("tls/rfc8448.txt");
const TLS_MODULE: &str = include_str!("../../oska-check/src/std/tls.osk");
const X509_MODULE: &str = include_str!("../../oska-check/src/std/x509.osk");
const RSA_MODULE: &str = include_str!("../../oska-check/src/std/rsa.osk");

/// The client's random and x25519 private key of the trace.
const CLIENT_RANDOM: &str = "cb34ecb1e78163ba1c38c6dacb196a6dffa21a8d9912ec18a2ef6283024dece7";
const CLIENT_PRIVATE_KEY: &str = "49af42ba7f7994852d713ef2784bcbcaa7911de26adc5642cb634540e7ea5005";

/// The cipher suites of the trace's ClientHello, in its order (the engine
/// offers two of the three; the server of the trace picks the first).
const HELLO_SUITES_LINE: &str = "body.append(u16_list([4865, 4867, 4866]))";

/// The extensions of the trace's ClientHello that come before the key share:
/// renegotiation_info, the nine groups and the session ticket. The server
/// name is the engine's own, and so is the key share that follows.
const HELLO_BEFORE_SHARE: &str = r#"    extensions.append(extension(65281, Bytes.from_hex("00").unwrap_or(Bytes.new())))
    extensions.append(extension(10, u16_list([29, 23, 24, 25, 256, 257, 258, 259, 260])))
    extensions.append(extension(35, Bytes.new()))
"#;

/// The extensions of the trace's ClientHello after the key share.
const HELLO_AFTER_SHARE: &str = r#"    extensions.append(extension(51, shares))
    extensions.append(extension(43, Bytes.from_hex("020304").unwrap_or(Bytes.new())))
    extensions.append(extension(13, u16_list([1027, 1283, 1539, 515, 2052, 2053, 2054, 1025, 1281, 1537, 513, 1026, 1282, 1538, 514])))
    extensions.append(extension(45, Bytes.from_hex("0101").unwrap_or(Bytes.new())))
    extensions.append(extension(28, Bytes.from_hex("4001").unwrap_or(Bytes.new())))
"#;

/// What the hook adds to the module: the engine the trace needs, and the
/// record of the trace's ClientHello (record version 1.0, as the RFC's).
const HOOK: &str = r#"
fn rfc8448_hello_record(hello: Bytes) -> Bytes {
    var out = Bytes.new()
    out.push(22)
    out.push(3)
    out.push(1)
    out.push_u16_be(hello.len().to_u16())
    out.append(hello)
    out
}

pub fn rfc8448_engine(config: TlsConfig) -> Engine {
    Engine.new("server", config, Secret.new(Bytes.zeroed(64)), 1500000000)
}
"#;

/// `text` with the one occurrence of `old` replaced by `new`.
fn replaced(text: &str, old: &str, new: &str) -> String {
    assert_eq!(
        text.matches(old).count(),
        1,
        "std.tls should hold `{old}` once"
    );
    text.replace(old, new)
}

/// `text` with what lies from the one `start` up to the one `end` replaced by
/// `new` (`end` stays).
fn replaced_between(text: &str, start: &str, end: &str, new: &str) -> String {
    assert_eq!(
        text.matches(start).count(),
        1,
        "std.tls should hold `{start}` once"
    );
    let from = text.find(start).expect("start");
    let to = from
        + text[from..]
            .find(end)
            .unwrap_or_else(|| panic!("std.tls should hold `{end}` after it"));
    format!("{}{}{}", &text[..from], new, &text[to..])
}

/// The module with the hook, over the package's copies of `rsa` and `x509`.
/// The hook fixes the client random, the x25519 key, an empty legacy session
/// ID, the cipher suites and the extensions around the server name and key
/// share (the engine's own `client_hello` builds the rest), the record version
/// of the ClientHello, and the extension it asks for (record_size_limit, which
/// the server's EncryptedExtensions answers). It sends no compatibility
/// change_cipher_spec and skips the certificate path check.
fn hooked_module() -> String {
    // The engine's own `client_hello` runs: only its suite list and the
    // extensions around the server name and the key share are the trace's.
    let mut module = replaced(
        TLS_MODULE,
        "body.append(u16_list([SUITE_CHACHA, SUITE_AES]))",
        HELLO_SUITES_LINE,
    );
    module = replaced_between(
        &module,
        "    extensions.append(extension(10, u16_list([GROUP_X25519, GROUP_P256])))\n",
        "    var entry = Bytes.new()\n    entry.push_u16_be(group.to_u16())",
        HELLO_BEFORE_SHARE,
    );
    module = replaced(
        &module,
        "    extensions.append(extension(51, shares))\n",
        HELLO_AFTER_SHARE,
    );
    module = replaced(
        &module,
        "Some(bytes) => { client_random = bytes.expose() }",
        &format!("Some(_) => {{ client_random = Bytes.from_hex(\"{CLIENT_RANDOM}\").unwrap_or(Bytes.new()) }}"),
    );
    module = replaced(
        &module,
        "Some(bytes) => { session_id = bytes.expose() }",
        "Some(_) => { session_id = Bytes.new() }",
    );
    module = replaced(
        &module,
        "Some(bytes) => { x_scalar = bytes }",
        &format!("Some(_) => {{ x_scalar = Secret.new(Bytes.from_hex(\"{CLIENT_PRIVATE_KEY}\").unwrap_or(Bytes.new())) }}"),
    );
    module = replaced(
        &module,
        "self.transcript.update(hello)\n        self.out.append(plain_record(22, hello))\n        self.state = STATE_SERVER_HELLO",
        "self.transcript.update(hello)\n        self.out.append(rfc8448_hello_record(hello))\n        self.state = STATE_SERVER_HELLO",
    );
    module = replaced(
        &module,
        "parse_extensions(message, 6, [0, 10, 16])?",
        "parse_extensions(message, 6, [0, 10, 16, 28])?",
    );
    module = replaced(&module, "ccs_sent: false,", "ccs_sent: true,");
    module = replaced(
        &module,
        "match x509.verify_server(self.config.trust, leaf, others, self.server_name, self.now) {\n            Ok(_) => {}",
        "match Ok(others) {\n            Ok(_) => {}",
    );
    module = replaced(&module, "use std.rsa\n", "use pkg.rsa\n");
    module = replaced(&module, "use std.x509\n", "use pkg.x509\n");
    module.push_str(HOOK);
    module
}

/// `std.rsa` with the modulus sizes the RFC's key has: 1024 bits, which the
/// module refuses (it takes 2048 to 8192).
fn small_key_rsa() -> String {
    let module = replaced(RSA_MODULE, "size < 256 ||", "size < 128 ||");
    replaced(&module, "bits < 2048 ||", "bits < 1024 ||")
}

/// `std.x509` over that `rsa`, so that the key of the RFC's certificate is
/// a key.
fn small_key_x509() -> String {
    replaced(X509_MODULE, "use std.rsa\n", "use pkg.rsa\n")
}

fn replay_case(name: &str) -> Case {
    let module = hooked_module();
    let rsa = small_key_rsa();
    let x509 = small_key_x509();
    Case::package(
        name,
        &[
            (
                "oska.toml",
                "[package]\nname = \"rfc8448\"\nversion = \"0.1.0\"\n",
            ),
            ("src/main.osk", REPLAY),
            ("src/tls.osk", &module),
            ("src/rsa.osk", &rsa),
            ("src/x509.osk", &x509),
        ],
    )
    .with_stdin(TRACE)
}

#[test]
fn the_trace_replays_record_for_record_on_both_backends() {
    let outcome = assert_same(&replay_case("tls_rfc8448"));
    assert_eq!(outcome.stdout, "done\n", "stderr: {}", outcome.stderr);
    assert_eq!(outcome.exit_code, Some(0));
}

/// The trace with the last byte of its `index`th record (of `prefix`
/// records, `C` or `S`) changed.
fn with_changed_record(prefix: &str, index: usize) -> String {
    let mut seen = 0;
    let mut out = String::new();
    for line in TRACE.lines() {
        let mut line = line.to_string();
        if line.starts_with(&format!("{prefix} ")) {
            if seen == index {
                let last = line.pop().expect("hex");
                line.push(if last == '0' { '1' } else { '0' });
            }
            seen += 1;
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}

fn run_with(exe: &(PathBuf, PathBuf), stdin: &str) -> String {
    let mut child = Command::new(&exe.0)
        .current_dir(&exe.1)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the replay program should run");
    child
        .stdin
        .take()
        .expect("a piped stdin")
        .write_all(stdin.as_bytes())
        .expect("the trace goes to stdin");
    let mut out = String::new();
    child
        .stdout
        .take()
        .expect("a piped stdout")
        .read_to_string(&mut out)
        .expect("output");
    child.wait().expect("the replay program ends");
    out.trim().to_string()
}

#[test]
fn a_changed_byte_in_any_record_of_the_trace_fails_the_replay() {
    static EXE: OnceLock<(PathBuf, PathBuf)> = OnceLock::new();
    let exe = EXE.get_or_init(|| build_native(&replay_case("tls_rfc8448_changed")));
    assert_eq!(run_with(exe, TRACE), "done", "the unchanged trace passes");
    let records: Vec<(&str, usize)> = [("C", 4), ("S", 5)]
        .iter()
        .flat_map(|&(prefix, count)| (0..count).map(move |i| (prefix, i)))
        .collect();
    let outcomes = in_parallel(&records, |&(prefix, index)| {
        (
            prefix,
            index,
            run_with(exe, &with_changed_record(prefix, index)),
        )
    });
    for (prefix, index, outcome) in outcomes {
        assert_ne!(
            outcome, "done",
            "{prefix} record {index} changed, replay passed"
        );
        assert!(
            outcome.starts_with("mismatch"),
            "{prefix} record {index} changed: {outcome}"
        );
    }
}
