// SPDX-License-Identifier: Apache-2.0
//! `std.http` (#245 of spec #228) as programs run: an HTTPS GET with
//! `http.get` against the loopback test server of #244 (`tls/server.osk`,
//! with the test CA of `tls/pki/`), whose `http_` scenarios answer with a
//! response well formed or hostile.
//!
//! The server is built natively once and started on port 0 for each
//! scenario, and the client program (`tls/http_client.osk`) runs against it
//! natively, and on the interpreter too for the core set, through the
//! differential harness, which requires the same output, exit code and tree.
//! After each connection the server says which alert the client sent, so
//! that the client is held to ending every connection it opened (a
//! `close_notify`, alert 0) and to the alert RFC 8446 names for a TLS failure.
//! The scenarios cover every framing (`Content-Length`, chunked, the end of
//! the stream), a `100 Continue`, a redirect that is returned and never
//! followed, the request the server received (`Host`, the path, the
//! headers), a body past its limit and every way a response can be
//! malformed or cut off. The response parser alone, on every framing and
//! hostile input, is in `tests/fixtures/http_reader.osk`, which the corpus
//! runs on both backends.
//!
//! These handshakes run the TLS engine's cryptography, so the slow set runs
//! natively only, as the TLS tests of #244 do: only the core scenarios
//! (a plain GET and a certificate that is for another host) run on the
//! interpreter too.

#![cfg(any(
    all(
        any(target_os = "linux", target_os = "macos"),
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(windows, target_arch = "x86_64")
))]

mod common;
mod differential;

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;

use differential::{assert_same, build_native, in_parallel, Case};

const SERVER: &str = include_str!("tls/server.osk");
const CLIENT: &str = include_str!("tls/http_client.osk");
const ROOT: &str = include_str!("tls/pki/root.hex");

fn pki(file: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/tls/pki")
        .join(file)
}

/// The certificate chain file the server sends for a scenario.
fn chain_for(scenario: &str) -> PathBuf {
    pki(match scenario {
        "wrong_host" => "chain_wrong_host.hex",
        _ => "chain_good.hex",
    })
}

/// The test server, built natively once for every test of this file.
fn server_exe() -> &'static Path {
    static EXE: OnceLock<PathBuf> = OnceLock::new();
    EXE.get_or_init(|| build_native(&Case::file("http_server", SERVER)).0)
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
            .arg(chain_for(scenario))
            .arg(pki("leaf.key.hex"))
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

/// One scenario of the loopback server and what the client sees.
struct Scenario {
    name: &'static str,
    /// The server's scenario.
    server: &'static str,
    /// The client's mode, and the arguments after it.
    mode: &'static str,
    extra: &'static [&'static str],
    /// What the client prints, with `PORT` for the server's port.
    output: &'static str,
    /// The description of the alert the client sends the server on each
    /// connection, if any (0 is `close_notify`).
    alert: Option<u8>,
    /// How many connections the client makes.
    connections: usize,
    /// Whether the interpreter runs it too (the rest run natively only).
    both: bool,
}

fn ok(
    name: &'static str,
    server: &'static str,
    mode: &'static str,
    output: &'static str,
) -> Scenario {
    Scenario {
        name,
        server,
        mode,
        extra: &[],
        output,
        alert: Some(0),
        connections: 1,
        both: false,
    }
}

fn with_alert(mut s: Scenario, alert: Option<u8>) -> Scenario {
    s.alert = alert;
    s
}

fn on_both(mut s: Scenario) -> Scenario {
    s.both = true;
    s
}

const HELLO: &str = "status 200\nx-test a\nlocation none\nbody [hello]\n";

fn scenarios() -> Vec<Scenario> {
    vec![
        on_both(ok("a_plain_get", "http_ok", "get", HELLO)),
        ok(
            "the_request_the_server_receives",
            "http_echo",
            "echo",
            "status 200\nx-test none\nlocation none\nbody [GET /a%20b?q=1 HTTP/1.1\r\nHost: 127.0.0.1:PORT\r\nConnection: close\r\nAccept-Encoding: identity\r\nX-One: 1\r\nX-Two: two\r\nUser-Agent: oska\r\n\r\n]\n",
        ),
        ok("a_chunked_body", "http_chunked", "get", "status 200\nx-test a\nlocation none\nbody [hello world]\n"),
        ok("a_response_in_small_records", "http_split", "get", "status 200\nx-test none\nlocation none\nbody [hello]\n"),
        ok("a_body_that_ends_with_close_notify", "http_close", "get", "status 200\nx-test a\nlocation none\nbody [until the end]\n"),
        ok(
            "a_redirect_is_returned_not_followed",
            "http_redirect",
            "get",
            "status 302\nx-test none\nlocation https://elsewhere.test/\nbody [moved]\n",
        ),
        ok("a_100_continue_is_skipped", "http_continue", "get", "status 404\nx-test none\nlocation none\nbody [nope]\n"),
        ok("a_body_in_three_records", "http_large", "large", "status 200\nx-test none\nlocation none\nbody 40000 bytes, checksum 549862\n"),
        ok("a_body_past_the_maximum", "http_large", "limit", "error the response is too large\n"),
        ok(
            "a_body_with_two_framings",
            "http_both",
            "get",
            "error malformed response: both Transfer-Encoding and Content-Length\n",
        ),
        ok(
            "two_different_lengths",
            "http_conflict",
            "get",
            "error malformed response: two different Content-Length values\n",
        ),
        ok("obsolete_line_folding", "http_fold", "get", "error malformed response: obsolete line folding\n"),
        ok("a_bare_line_feed", "http_bare_lf", "get", "error malformed response: a bare line feed\n"),
        ok(
            "a_body_that_ends_early",
            "http_short",
            "get",
            "error malformed response: the response ended before it was complete\n",
        ),
        ok(
            "a_chunk_size_that_overflows",
            "http_overflow",
            "get",
            "error malformed response: a chunk size that overflows\n",
        ),
        ok("more_than_100_headers", "http_many_headers", "get", "error the response is too large\n"),
        with_alert(
            ok(
                "a_stream_cut_without_close_notify",
                "http_truncate",
                "get",
                "error tls: the connection ended without close_notify\n",
            ),
            None,
        ),
        on_both(with_alert(
            ok(
                "a_certificate_for_another_host",
                "wrong_host",
                "get",
                "error tls: certificate: the certificate is not for this host name\n",
            ),
            Some(42),
        )),
        with_alert(
            ok(
                "the_default_roots_do_not_trust_the_test_ca",
                "http_ok",
                "default_roots",
                "error tls: certificate: the certificate does not chain to a trusted root\n",
            ),
            Some(48),
        ),
        Scenario {
            name: "a_path_that_does_not_start_with_a_slash_is_refused_before_connecting",
            server: "http_ok",
            mode: "echo",
            extra: &["no-slash"],
            output: "error invalid request: the path must start with /\n",
            alert: None,
            connections: 0,
            both: false,
        },
        Scenario {
            name: "a_path_with_a_space_is_refused_before_connecting",
            server: "http_ok",
            mode: "echo",
            extra: &["/a b"],
            output: "error invalid request: the path holds a control character or a space\n",
            alert: None,
            connections: 0,
            both: false,
        },
        Scenario {
            name: "a_path_with_a_fragment_is_refused_before_connecting",
            server: "http_ok",
            mode: "echo",
            extra: &["/a#b"],
            output: "error invalid request: the path holds a #\n",
            alert: None,
            connections: 0,
            both: false,
        },
    ]
}

/// The client program, built natively once.
fn client_exe() -> &'static (PathBuf, PathBuf) {
    static EXE: OnceLock<(PathBuf, PathBuf)> = OnceLock::new();
    EXE.get_or_init(|| build_native(&Case::file("http_client_native", CLIENT)))
}

/// Run one scenario against its own server: natively, and on the
/// interpreter too for the core set (the differential harness requires the
/// same output, exit code and tree), and check what the client printed and
/// the alert the server saw from each run.
fn run_scenario(s: &Scenario) {
    let runs = if s.both { 2 } else { s.connections };
    let server = Server::start(s.server, runs);
    let port = server.port.to_string();
    let mut args = vec![port.as_str(), "127.0.0.1", s.mode, ROOT.trim()];
    args.extend(s.extra.iter().copied());
    let (stdout, exit_code, stderr) = if s.both {
        let mut case = Case::file(&format!("http_{}", s.name), CLIENT).with_args(&args);
        if let Ok(root) = std::env::var("SYSTEMROOT") {
            case = case.with_env("SYSTEMROOT", &root);
        }
        let outcome = assert_same(&case);
        (outcome.stdout, outcome.exit_code, outcome.stderr)
    } else {
        let (exe, dir) = client_exe();
        let output = Command::new(exe)
            .current_dir(dir)
            .args(&args)
            .output()
            .expect("the client should run");
        (
            String::from_utf8_lossy(&output.stdout).into_owned(),
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    };
    assert_eq!(exit_code, Some(0), "{}: {stdout} {stderr}", s.name);
    assert_eq!(stdout, s.output.replace("PORT", &port), "{}", s.name);
    assert_eq!(stderr, "", "{}", s.name);
    let wanted = match s.alert {
        Some(code) => format!("alert {code}"),
        None => "no alert".to_string(),
    };
    assert_eq!(
        server.alerts(),
        vec![wanted; if s.connections == 0 { 0 } else { runs }],
        "{}: the alert the client sent, on each run",
        s.name
    );
}

#[test]
fn every_scenario_of_the_loopback_server_gives_the_response_it_should() {
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
