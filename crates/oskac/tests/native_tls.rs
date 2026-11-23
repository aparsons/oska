// SPDX-License-Identifier: Apache-2.0
//! `std.tls` (#244 of spec #228) as programs run: the TLS 1.3 client's pure
//! engine and `tls.connect`, on both backends, on every supported platform.
//!
//! Three seams, as the spec's testing decisions name them:
//!
//! - **The loopback test server** (`tls/server.osk`, an Oska program of the
//!   test corpus with the test CA of `tls/pki/`): it is built natively once,
//!   started on port 0 for each scenario, and a client program
//!   (`tls/client.osk`) runs against it on the interpreter and natively
//!   through the differential harness (natively only for the slow sets, see
//!   below), which requires the same output, exit code and error kinds. The server says after each connection which alert
//!   the client sent, so the alert RFC 8446 names for each failure is held
//!   too. The scenarios are each suite and group, `HelloRetryRequest`, ALPN,
//!   `KeyUpdate`, padding, empty records, coalesced and fragmented
//!   handshake records, a `CertificateRequest`, a body in several records,
//!   and every way the server can misbehave.
//! - **Recorded handshakes** (`tls/recorded/*.txt`, made by
//!   `scripts/tls-fixtures/record.py` against openssl's server, an
//!   independent implementation): replayed offline through the engine
//!   (`tls/replay.osk`) with the stored randomness and time, which must
//!   reproduce every byte the engine sent, on both backends; then mutated
//!   (a bit flipped in each field of the ServerHello and in the records after
//!   it, a record dropped, repeated or swapped, the stream cut) so that each
//!   fails with the expected error kind and alert. Recordings from real
//!   servers need the internet and are not part of the checked tree.
//! - **The signing** the server does (ECDSA P-256 with the nonce of
//!   RFC 6979) is checked against RFC 6979's published signature.
//!
//! These handshakes run the engine's cryptography many times, so the
//! slow sets run natively only, as the slow vector sets of the crypto
//! modules do: the mutation sweep, and all but a core set of the loopback
//! scenarios (nine, which cover each suite and group, `KeyUpdate` and the
//! main failures). The replays of the unchanged recordings and a few
//! mutations run on both backends.

#![cfg(any(
    all(
        any(target_os = "linux", target_os = "macos"),
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(windows, target_arch = "x86_64")
))]

mod common;
mod differential;

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;

use differential::{assert_same, build_native, in_parallel, Case};

const SERVER: &str = include_str!("tls/server.osk");
const CLIENT: &str = include_str!("tls/client.osk");
const REPLAY: &str = include_str!("tls/replay.osk");
const ROOT: &str = include_str!("tls/pki/root.hex");

fn pki_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/tls/pki")
}

fn recording(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/tls/recorded")
        .join(format!("{name}.txt"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// The test server, built natively once for every test of this file.
fn server_exe() -> &'static Path {
    static EXE: OnceLock<PathBuf> = OnceLock::new();
    EXE.get_or_init(|| build_native(&Case::file("tls_server", SERVER)).0)
}

/// A running test server, killed when it is dropped.
struct Server {
    child: Child,
    port: u16,
    lines: BufReader<std::process::ChildStdout>,
}

impl Server {
    /// Start the server for `scenario` to serve `connections` connections,
    /// and read the port it prints.
    fn start(scenario: &str, connections: usize) -> Server {
        let mut child = Command::new(server_exe())
            .arg(pki_dir())
            .arg(scenario)
            .arg(connections.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the test server should start");
        let mut lines = BufReader::new(child.stdout.take().expect("a piped stdout"));
        let mut line = String::new();
        lines
            .read_line(&mut line)
            .expect("the server prints its port");
        let port = line
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("the server printed {line:?}, not a port"));
        Server { child, port, lines }
    }

    /// Wait for the server to serve every connection and give what it said
    /// on stderr after each: `alert N` or `no alert`.
    fn alerts(mut self) -> Vec<String> {
        let mut text = String::new();
        self.child
            .stderr
            .take()
            .expect("a piped stderr")
            .read_to_string(&mut text)
            .expect("the server's stderr");
        let mut rest = String::new();
        let _ = self.lines.read_to_string(&mut rest);
        text.lines()
            .filter(|l| *l != "listening")
            .map(str::to_string)
            .collect()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// One scenario of the loopback server and what a client sees.
struct Scenario {
    /// The server's scenario and the client's mode.
    name: &'static str,
    server: &'static str,
    host: &'static str,
    mode: &'static str,
    /// What the client prints.
    output: &'static str,
    /// The description of the alert the client sends the server, if any.
    alert: Option<u8>,
    /// Whether the interpreter runs it too (the rest run natively only).
    both: bool,
}

const OPEN: &str = "connected\nclosed after: echo:hello\n";

const fn ok(
    name: &'static str,
    server: &'static str,
    host: &'static str,
    mode: &'static str,
    output: &'static str,
) -> Scenario {
    Scenario {
        name,
        server,
        host,
        mode,
        output,
        alert: None,
        both: false,
    }
}

const fn fails(
    name: &'static str,
    server: &'static str,
    output: &'static str,
    alert: Option<u8>,
) -> Scenario {
    Scenario {
        name,
        server,
        host: "127.0.0.1",
        mode: "echo",
        output,
        alert,
        both: false,
    }
}

/// The scenarios the interpreter runs too.
const BOTH: [&str; 9] = [
    "chacha_x25519",
    "aes_x25519",
    "aes_hello_retry_p256",
    "alpn",
    "key_update_both_ways",
    "a_wrong_host_name",
    "a_bad_finished",
    "a_stream_cut_without_close_notify",
    "the_downgrade_marker",
];

fn scenarios() -> Vec<Scenario> {
    let mut all = scenario_list();
    for s in &mut all {
        s.both = BOTH.contains(&s.name);
    }
    all
}

fn scenario_list() -> Vec<Scenario> {
    vec![
        ok("chacha_x25519", "good", "127.0.0.1", "echo", OPEN),
        ok("a_dns_name_host", "good", "localhost", "echo", OPEN),
        ok("aes_x25519", "aes", "127.0.0.1", "echo", OPEN),
        ok("chacha_hello_retry_p256", "hrr", "127.0.0.1", "echo", OPEN),
        ok("aes_hello_retry_p256", "hrr_aes", "127.0.0.1", "echo", OPEN),
        ok("alpn", "alpn", "127.0.0.1", "alpn", "connected\nprotocol: h2\nclosed after: echo:hello\n"),
        Scenario {
            name: "key_update_both_ways",
            server: "keyupdate",
            host: "127.0.0.1",
            mode: "twice",
            output: "connected\nfirst: echo:one\nsecond: echo:two\ndone\n",
            alert: Some(0),
            both: true,
        },
        ok("a_body_in_three_records", "large", "127.0.0.1", "large", "connected\nclosed after: 40000 bytes, checksum 549862\n"),
        ok("padded_records", "padded", "127.0.0.1", "echo", OPEN),
        ok("an_empty_application_record", "empty_records", "127.0.0.1", "echo", OPEN),
        ok("a_new_session_ticket", "new_session_ticket", "127.0.0.1", "echo", OPEN),
        ok("the_flight_in_one_record", "coalesced", "127.0.0.1", "echo", OPEN),
        ok("messages_split_across_records", "fragmented", "127.0.0.1", "echo", OPEN),
        ok("a_certificate_request", "cert_request", "127.0.0.1", "echo", OPEN),
        fails(
            "a_wrong_host_name",
            "wrong_host",
            "error certificate: certificate: the certificate is not for this host name\n",
            Some(42),
        ),
        fails(
            "an_expired_certificate",
            "expired",
            "error certificate: certificate: a certificate has expired\n",
            Some(45),
        ),
        fails(
            "an_unknown_issuer",
            "unknown_issuer",
            "error certificate: certificate: the certificate does not chain to a trusted root\n",
            Some(48),
        ),
        fails(
            "a_missing_intermediate",
            "no_intermediate",
            "error certificate: certificate: the certificate does not chain to a trusted root\n",
            Some(48),
        ),
        Scenario {
            name: "the_default_roots_do_not_trust_the_test_ca",
            server: "good",
            host: "127.0.0.1",
            mode: "default_roots",
            output: "error certificate: certificate: the certificate does not chain to a trusted root\n",
            alert: Some(48),
            both: false,
        },
        fails(
            "a_bad_certificate_verify",
            "bad_signature",
            "error certificate: certificate: a certificate signature does not verify\n",
            Some(51),
        ),
        fails(
            "a_bad_finished",
            "bad_finished",
            "error protocol: protocol error: the Finished message does not verify\n",
            Some(51),
        ),
        fails(
            "an_oversized_record",
            "oversized",
            "connected\nerror protocol: protocol error: a record longer than 2^14 + 256 bytes\n",
            Some(22),
        ),
        fails(
            "a_stream_cut_without_close_notify",
            "truncate",
            "connected\nerror truncated: the connection ended without close_notify\n",
            None,
        ),
        fails(
            "an_alert_from_the_server",
            "alert",
            "error alert: the server sent the alert handshake_failure\n",
            None,
        ),
        fails(
            "an_alert_after_the_handshake",
            "alert_after",
            "connected\nerror alert: the server sent the alert internal_error\n",
            None,
        ),
        fails(
            "the_downgrade_marker",
            "downgrade",
            "error protocol: protocol error: the server set the downgrade marker\n",
            Some(47),
        ),
        fails(
            "an_older_version",
            "tls12",
            "error protocol: protocol error: the server chose a version older than TLS 1.3\n",
            Some(70),
        ),
        fails(
            "a_record_that_fails_authentication",
            "bad_mac",
            "connected\nerror protocol: protocol error: a record failed authentication\n",
            Some(20),
        ),
        fails(
            "a_suite_that_was_not_offered",
            "bad_suite",
            "error protocol: protocol error: the server chose a cipher suite that was not offered\n",
            Some(47),
        ),
        fails(
            "a_session_id_that_is_not_echoed",
            "bad_session_id",
            "error protocol: protocol error: the server did not echo the legacy session ID\n",
            Some(47),
        ),
        fails(
            "a_group_that_was_not_offered",
            "bad_group",
            "error protocol: protocol error: the server chose a group that was not offered\n",
            Some(47),
        ),
        fails(
            "a_second_hello_retry_request",
            "hrr_twice",
            "error protocol: protocol error: a second HelloRetryRequest\n",
            Some(10),
        ),
        fails(
            "a_hello_retry_request_that_changes_nothing",
            "hrr_same_group",
            "error protocol: protocol error: the HelloRetryRequest asks for the group already sent\n",
            Some(47),
        ),
        fails(
            "a_hello_retry_request_for_a_group_not_offered",
            "hrr_unsupported",
            "error unsupported: not supported: the server asks for a group this client does not offer\n",
            Some(40),
        ),
        fails(
            "an_extension_that_was_not_offered",
            "unoffered_extension",
            "error protocol: protocol error: the server sent an extension that was not offered (21)\n",
            Some(110),
        ),
    ]
}

/// The client program, built natively once.
fn client_exe() -> &'static (PathBuf, PathBuf) {
    static EXE: OnceLock<(PathBuf, PathBuf)> = OnceLock::new();
    EXE.get_or_init(|| build_native(&Case::file("tls_client_native", CLIENT)))
}

/// Run one scenario against its own server: natively, and on the
/// interpreter too for the core set (the differential harness requires the
/// same output, exit code and tree), and check what the client printed and
/// the alert the server saw from each run.
fn run_scenario(s: &Scenario) {
    let runs = if s.both { 2 } else { 1 };
    let server = Server::start(s.server, runs);
    let port = server.port.to_string();
    let args = [port.as_str(), s.host, s.mode, ROOT.trim()];
    let (stdout, exit_code, stderr) = if s.both {
        let mut case = Case::file(&format!("tls_{}", s.name), CLIENT).with_args(&args);
        if let Ok(root) = std::env::var("SYSTEMROOT") {
            case = case.with_env("SYSTEMROOT", &root);
        }
        let outcome = assert_same(&case);
        (outcome.stdout, outcome.exit_code, outcome.stderr)
    } else {
        let (exe, dir) = client_exe();
        let output = Command::new(exe)
            .current_dir(dir)
            .args(args)
            .output()
            .expect("the client should run");
        (
            String::from_utf8_lossy(&output.stdout).into_owned(),
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    };
    assert_eq!(exit_code, Some(0), "{}: {stdout} {stderr}", s.name);
    assert_eq!(stdout, s.output, "{}", s.name);
    assert_eq!(stderr, "", "{}", s.name);
    let wanted = match s.alert {
        Some(code) => format!("alert {code}"),
        None => "no alert".to_string(),
    };
    assert_eq!(
        server.alerts(),
        vec![wanted; runs],
        "{}: the alert the client sent, on each run",
        s.name
    );
}

#[test]
fn every_scenario_of_the_loopback_server_behaves_as_it_should() {
    // Build the programs before the threads race to.
    server_exe();
    client_exe();
    let all = scenarios();
    let failures: Vec<String> = in_parallel(&all, |s| {
        std::panic::catch_unwind(|| run_scenario(s)).err().map(|e| {
            let text = e
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            format!("{}: {text}", s.name)
        })
    })
    .into_iter()
    .flatten()
    .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn the_server_signs_with_the_nonce_of_rfc_6979() {
    // RFC 6979, appendix A.2.5: P-256 with SHA-256, the message "sample".
    let output = Command::new(server_exe())
        .arg(pki_dir())
        .args([
            "sign",
            "1",
            "af2bdbe1aa9b6ec1e2ade1d694f41fc71a831d0268e9891562113d8a62add1bf",
            "c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721",
        ])
        .output()
        .expect("the server should run");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "3046022100efd48b2aacb6a8fd1140dd9cd45e81d69d2c877b56aaf991c34d0ea84eaf3716\
         022100f7cb1c942d657c41d436c7a1b6e29f65f3e900dbb9aff4064dc4ab2f843acda8"
    );
}

// ---- Recorded handshakes ------------------------------------------------------------

const RECORDINGS: [&str; 5] = [
    "chacha_x25519_ecdsa_p256",
    "aes_x25519_ecdsa_p256",
    "hello_retry_request_p256",
    "rsa_pss",
    "ecdsa_p384",
];

#[test]
fn recorded_handshakes_replay_byte_for_byte_on_both_backends() {
    let outcomes = in_parallel(&RECORDINGS, |name| {
        let case = Case::file(&format!("tls_replay_{name}"), REPLAY)
            .with_args(&["exact"])
            .with_stdin(&recording(name));
        (*name, assert_same(&case))
    });
    for (name, outcome) in outcomes {
        assert_eq!(outcome.stdout, "done\n", "{name}: {outcome:#?}");
        assert_eq!(outcome.exit_code, Some(0), "{name}");
    }
}

/// The replay program, built once, and what it prints for `mode` and the
/// recording `name`.
fn replay(name: &str, mode: &[&str]) -> String {
    static EXE: OnceLock<(PathBuf, PathBuf)> = OnceLock::new();
    let (exe, dir) = EXE.get_or_init(|| build_native(&Case::file("tls_replay_mutations", REPLAY)));
    let mut child = Command::new(exe)
        .current_dir(dir)
        .args(mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the replay program should run");
    let text = recording(name);
    child
        .stdin
        .take()
        .expect("a piped stdin")
        .write_all(text.as_bytes())
        .expect("the recording goes to stdin");
    let output = child.wait_with_output().expect("the replay program ends");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// The server's records of a recording, as bytes.
fn server_records(name: &str) -> Vec<Vec<u8>> {
    recording(name)
        .lines()
        .filter_map(|l| l.strip_prefix("S "))
        .map(|hex| {
            (0..hex.len() / 2)
                .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("hexadecimal"))
                .collect()
        })
        .collect()
}

#[test]
fn a_bit_flipped_in_the_server_hello_fails_with_the_alert_its_field_names() {
    let name = "chacha_x25519_ecdsa_p256";
    // (offset in the record, bit, what the engine prints). The record is
    // the 5-byte header, the 4-byte handshake header, then the ServerHello:
    // version 9..11, random 11..43, session ID length 43, session ID
    // 44..76, suite 76..78, compression 78, extensions from 79.
    let cases: [(usize, u8, &str); 12] = [
        (0, 0, "failed protocol alert 10"),         // the record type
        (1, 0, "failed protocol alert 70"),         // the record version
        (9, 0, "failed protocol alert 70"),         // the legacy version
        (11, 0, "failed protocol encrypted alert"), // the random, so the keys differ
        (43, 0, "failed protocol alert 47"),        // the session ID length
        (44, 0, "failed protocol alert 47"),        // the session ID
        (75, 7, "failed protocol alert 47"),        // its last byte
        (77, 0, "failed protocol alert 47"),        // a suite that was not offered
        (78, 0, "failed protocol alert 47"),        // a compression method
        (86, 0, "failed protocol alert 47"),        // the version in supported_versions
        (92, 0, "failed protocol alert 47"),        // the group
        (96, 0, "failed protocol encrypted alert"), // the key share, so the keys differ
    ];
    let outputs = in_parallel(&cases, |(offset, bit, _)| {
        replay(name, &["flip", "0", &offset.to_string(), &bit.to_string()])
    });
    for ((offset, bit, expected), got) in cases.iter().zip(outputs) {
        assert_eq!(&got, expected, "byte {offset} bit {bit} of the ServerHello");
    }
}

#[test]
fn a_bit_flipped_in_a_protected_record_fails_before_any_byte_is_used() {
    // Every record after the ServerHello and the compatibility
    // `change_cipher_spec`: the type, the first byte of the ciphertext and
    // the last byte of the tag. The engine fails with a protocol error and an
    // alert sealed under the keys it has.
    for name in RECORDINGS {
        let records = server_records(name);
        let first_protected = records
            .iter()
            .position(|r| r[0] == 23)
            .expect("a protected record");
        let jobs: Vec<(usize, usize)> = (first_protected..records.len())
            .flat_map(|index| {
                let last = records[index].len() - 1;
                [(index, 0), (index, 5), (index, last)]
            })
            .collect();
        let outputs = in_parallel(&jobs, |(index, offset)| {
            replay(
                name,
                &["flip", &index.to_string(), &offset.to_string(), "0"],
            )
        });
        for ((index, offset), got) in jobs.iter().zip(outputs) {
            assert_eq!(
                got, "failed protocol encrypted alert",
                "{name}: byte {offset} of server record {index}"
            );
        }
    }
}

#[test]
fn records_that_are_dropped_repeated_swapped_or_cut_off_fail() {
    for name in RECORDINGS {
        let records = server_records(name);
        let first_protected = records
            .iter()
            .position(|r| r[0] == 23)
            .expect("a protected record");
        let last = records.len() - 1;
        // (A record dropped from the end leaves the stream short, which the
        // cuts below cover; one repeated after the last is never read.)
        for (mode, at) in [
            ("drop", first_protected),
            ("swap", first_protected),
            ("swap", last - 1),
            ("duplicate", first_protected),
        ] {
            assert_eq!(
                replay(name, &[mode, &at.to_string()]),
                "failed protocol encrypted alert",
                "{name}: {mode} {at}"
            );
        }
        // The stream ends before a record: the handshake never finishes.
        for at in 0..records.len() {
            let got = replay(name, &["cut", &at.to_string()]);
            assert!(
                got == "failed truncated no alert",
                "{name}: cut before record {at}: {got}"
            );
        }
    }
}

#[test]
fn a_hello_retry_request_repeated_is_refused_in_plaintext() {
    // Record 0 of this recording is the HelloRetryRequest: a second one is
    // `unexpected_message` (10), sent before any key is set.
    assert_eq!(
        replay("hello_retry_request_p256", &["duplicate", "0"]),
        "failed protocol alert 10"
    );
}

#[test]
fn some_mutations_are_the_same_on_both_backends() {
    let jobs = [
        (
            "chacha_x25519_ecdsa_p256",
            ["flip", "0", "9", "0"],
            "failed protocol alert 70",
        ),
        (
            "rsa_pss",
            ["flip", "3", "5", "0"],
            "failed protocol encrypted alert",
        ),
        (
            "hello_retry_request_p256",
            ["duplicate", "0", "0", "0"],
            "failed protocol alert 10",
        ),
    ];
    for (i, (name, mode, expected)) in jobs.iter().enumerate() {
        let case = Case::file(&format!("tls_replay_mutation_{i}"), REPLAY)
            .with_args(mode)
            .with_stdin(&recording(name));
        let outcome = assert_same(&case);
        assert_eq!(outcome.stdout, format!("{expected}\n"), "{name}: {mode:?}");
    }
}
