// SPDX-License-Identifier: Apache-2.0
//! Host-narrowed network access on the interpreter (spec #228, ADR 0025):
//! `restrict_to(host, port)` on `Net` or `NetConnect` gives the only
//! capability that connects, `restrict_to_loopback(port)` on `NetListen`
//! gives the test server's listener, and the test host's `t.net` refuses
//! every connection in a plain test.
//!
//! These run the interpreter; the native backend's are in
//! `crates/oskac/tests/native_sockets.rs`. The real-network tests use only
//! the loopback address.

use std::net::{Ipv4Addr, TcpListener, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use oska_run::{
    run, test, FakeHost, Host, NameFiles, RealHost, RunOutcome, TestOptions, TestReport, TestStatus,
};

const PROGRAM: &[&str] = &["prog.osk"];

fn args() -> Vec<String> {
    PROGRAM.iter().map(|a| a.to_string()).collect()
}

fn run_real(src: &str) -> RunOutcome {
    run(src, &mut RealHost::new(), &args())
}

fn run_fake(src: &str) -> RunOutcome {
    run(src, &mut FakeHost::new(), &args())
}

fn assert_ran(outcome: &RunOutcome, src: &str) {
    assert!(
        outcome.diagnostics.is_empty() && outcome.panic.is_none() && outcome.exit_code == 0,
        "{src} should run cleanly: {outcome:#?}"
    );
}

/// The tests of `src`, plain ones on fresh fake hosts and `#[integration]`
/// ones on `host`.
fn run_tests(src: &str, host: &mut dyn Host, integration: bool) -> TestReport {
    test(
        src,
        host,
        TestOptions {
            integration,
            ..TestOptions::default()
        },
    )
}

fn passed(report: &TestReport) {
    let statuses: Vec<TestStatus> = report.tests.iter().map(|t| t.status).collect();
    assert!(
        statuses.iter().all(|s| *s == TestStatus::Passed),
        "{report:#?}"
    );
}

// ---- Running a program given the network ------------------------------------------------

#[test]
fn a_program_given_net_runs_instead_of_being_refused() {
    for ty in ["Net", "NetConnect", "NetListen"] {
        let src = format!("fn main(net: {ty}) {{\n}}\n");
        assert_ran(&run_fake(&src), &src);
    }
}

#[test]
fn a_host_or_loopback_capability_cannot_be_granted_to_main() {
    for ty in ["HostConnect", "LoopbackListen"] {
        let outcome = run_fake(&format!("fn main(c: {ty}) {{\n}}\n"));
        assert_eq!(outcome.diagnostics[0].code, "E0217", "{outcome:#?}");
        assert!(
            outcome.diagnostics[0]
                .message
                .starts_with(&format!("`main` cannot take `{ty}`: only attenuation")),
            "{outcome:#?}"
        );
    }
}

#[test]
fn net_connect_has_no_connect_of_its_own() {
    let outcome =
        run_fake("fn main(net: NetConnect) {\n    let _ = net.connect(\"example.com\", 443)\n}\n");
    assert!(
        outcome.diagnostics.iter().any(|d| d.code.starts_with('E')),
        "{outcome:#?}"
    );
    assert_ne!(outcome.exit_code, 0);
}

#[test]
fn untrusted_host_text_is_refused_where_it_picks_the_host() {
    for call in [
        "net.restrict_to(host, 443)",
        "net.connect_only().restrict_to(host, 443)",
    ] {
        let src = format!(
            "fn reach(net: Net, host: Untrusted<String>) {{\n    let _ = {call}\n}}\n\
             fn main() {{\n}}\n"
        );
        let outcome = run_fake(&src);
        assert!(
            outcome.diagnostics.iter().any(|d| d.code == "E0300"),
            "{call}: {outcome:#?}"
        );
    }
}

#[test]
fn a_trust_assertion_lets_a_validated_host_through() {
    let src = "fn reach(net: Net, host: Untrusted<String>) {\n    \
               let _ = net.restrict_to(host.assume_trusted(), 443)\n}\nfn main() {\n}\n";
    let outcome = run_fake(src);
    assert!(outcome.diagnostics.is_empty(), "{outcome:#?}");
}

// ---- The real network, on the loopback address ----------------------------------------

/// Run the `#[integration]` tests of `src` on the real host, where `t.net`
/// is the real network, and expect them to pass.
fn passes_on_the_real_network(src: &str) {
    let report = run_tests(src, &mut RealHost::new(), true);
    assert_eq!(report.tests.len(), 1, "{report:#?}");
    passed(&report);
}

/// Whether this machine has an IPv6 loopback address.
fn has_ipv6_loopback() -> bool {
    TcpListener::bind((std::net::Ipv6Addr::LOCALHOST, 0)).is_ok()
}

#[test]
fn a_loopback_listener_accepts_on_every_loopback_address_the_machine_has() {
    let mut hosts = vec!["127.0.0.1"];
    if has_ipv6_loopback() {
        hosts.push("::1");
    }
    let calls: String = hosts
        .iter()
        .map(|host| format!("    reach(t, listener, \"{host}\")\n"))
        .collect();
    passes_on_the_real_network(&format!(
        r#"
#[integration]
test "both" {{
    let listener = t.net.listen_only().restrict_to_loopback(0)
{calls}}}

fn reach(t: TestContext, listener: LoopbackListen, address: String) {{
    match t.net.restrict_to(address, listener.port()).connect() {{
        Ok(client) => match listener.accept() {{
            Ok(server) => {{
                t.assert(client.send("hi").is_ok())
                t.assert_eq(server.receive().unwrap_or("none"), "hi")
            }}
            Err(_) => t.fail("accept {{address}}")
        }}
        Err(_) => t.fail("connect {{address}}")
    }}
}}
"#
    ));
}

#[test]
fn a_loopback_server_and_a_client_talk_through_host_and_listener_capabilities() {
    passes_on_the_real_network(
        r#"
#[integration]
test "ping" {
    let listener = t.net.listen_only().restrict_to_loopback(0)
    let host = t.net.restrict_to("127.0.0.1", listener.port())
    t.assert_eq(host.name(), "127.0.0.1")
    match host.connect() {
        Ok(client) => match listener.accept() {
            Ok(server) => talk(t, client, server)
            Err(_) => t.fail("accept")
        }
        Err(_) => t.fail("connect")
    }
}

fn talk(t: TestContext, client: Connection, server: Connection) {
    t.assert_eq(client.host(), "127.0.0.1")
    t.assert(client.send("ping").is_ok())
    t.assert_eq(server.receive().unwrap_or("none"), "ping")
    t.assert(server.send_bytes(Bytes.zeroed(3)).is_ok())
    t.assert_eq(client.receive_bytes().unwrap_or(Bytes.zeroed(0)).len(), 3)
    client.close()
    t.assert_eq(server.receive_bytes().unwrap_or(Bytes.zeroed(9)).len(), 0)
    t.assert(!client.send("again").is_ok())
}
"#,
    );
}

#[test]
fn a_connection_to_a_closed_port_fails_with_the_same_words_on_every_platform() {
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        listener.local_addr().expect("addr").port()
    };
    passes_on_the_real_network(&format!(
        r#"
#[integration]
test "refused" {{
    match t.net.restrict_to("127.0.0.1", {port}).connect() {{
        Ok(_) => t.fail("connected")
        Err(e) => t.assert_eq(e.message, "cannot connect to 127.0.0.1:{port}: connection refused")
    }}
}}
"#
    ));
}

#[test]
fn a_malformed_host_or_a_port_out_of_range_is_an_error_that_says_why() {
    passes_on_the_real_network(
        r#"
fn why(net: Net, host: String, port: Int) -> Untrusted<String> {
    match net.restrict_to(host, port).connect() {
        Ok(_) => "connected"
        Err(e) => e.message
    }
}

#[integration]
test "malformed" {
    let bad = "it is not a DNS name or an IP address"
    t.assert_eq(why(t.net, "exa mple.com", 443), "invalid host \"exa mple.com\": {bad}")
    t.assert_eq(why(t.net, "", 443), "invalid host \"\": {bad}")
    t.assert_eq(why(t.net, "127.0.0.1", 0), "port 0 is out of range: use 1 to 65535")
    t.assert_eq(why(t.net, "127.0.0.1", 65536), "port 65536 is out of range: use 1 to 65535")
    t.assert_eq(why(t.net, "127.0.0.1", -5), "port -5 is out of range: use 1 to 65535")
}
"#,
    );
}

#[test]
fn a_program_run_on_the_real_host_listens_and_connects_with_main_s_net() {
    let src = r#"
fn main(net: Net) -> Result<(), IoError> {
    let listener = net.listen_only().restrict_to_loopback(0)
    let client = net.restrict_to("127.0.0.1", listener.port()).connect()?
    let server = listener.accept()?
    client.send("ping")?
    let got = server.receive()?
    server.send(got)?
    let echoed = client.receive()?
    let _ = echoed
    Ok(())
}
"#;
    assert_ran(&run_real(src), src);
}

#[test]
fn a_host_capability_names_its_host_and_port() {
    passes_on_the_real_network(
        r#"
#[integration]
test "names" {
    let host = t.net.restrict_to("oskalang.dev", 443)
    t.assert_eq(host.name(), "oskalang.dev")
    t.assert_eq(host.port(), 443)
}
"#,
    );
}

// ---- The test host's `t.net` --------------------------------------------------------------

#[test]
fn t_net_refuses_every_connect_and_listen_with_a_stable_error_and_records_where() {
    let src = r#"
test "connects are refused" {
    let host = t.net.restrict_to("evil.example", 8080)
    match host.connect() {
        Ok(_) => t.fail("connected")
        Err(e) => t.assert_eq(e.message, "network access is not available here: a plain test is offline")
    }
    match t.net.listen_only().restrict_to_loopback(0).accept() {
        Ok(_) => t.fail("accepted")
        Err(_) => t.assert(true)
    }
    t.assert_eq(t.net_attempts(), ["evil.example:8080", "loopback:0"])
}

test "each test starts with no attempts" {
    t.assert_eq(t.net_attempts().len(), 0)
}
"#;
    let report = run_tests(src, &mut FakeHost::new(), false);
    passed(&report);
    assert_eq!(report.tests.len(), 2);
}

#[test]
fn code_under_test_that_goes_somewhere_unexpected_is_caught() {
    let src = r#"
fn deliver(net: NetConnect, host: String) {
    let _ = net.restrict_to(host, 443).connect()
}

test "delivery goes where it should" {
    deliver(t.net.connect_only(), "hooks.example")
    t.assert_eq(t.net_attempts(), ["hooks.example:443"])
}
"#;
    passed(&run_tests(src, &mut FakeHost::new(), false));
}

#[test]
fn t_net_is_the_fake_unless_integration_is_asked_for() {
    let src = r#"
#[integration]
test "reaches nowhere in a skipped run" {
    t.fail("must not run")
}
"#;
    let report = run_tests(src, &mut RealHost::new(), false);
    assert_eq!(report.tests[0].status, TestStatus::Skipped, "{report:#?}");
}

// ---- `connect()` resolves names with `std.dns` ----------------------------------------------

/// A name server on `127.0.0.1` (UDP only) that answers every `A` query
/// with what `answer` makes of it, until dropped.
struct NameServer {
    port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl NameServer {
    fn start(answer: fn(&[u8]) -> Vec<u8>) -> NameServer {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("udp");
        let port = socket.local_addr().expect("address").port();
        socket
            .set_read_timeout(Some(Duration::from_millis(20)))
            .expect("timeout");
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::spawn(move || {
            let mut buffer = [0u8; 512];
            while !flag.load(Ordering::Relaxed) {
                if let Ok((read, peer)) = socket.recv_from(&mut buffer) {
                    let _ = socket.send_to(&answer(&buffer[..read]), peer);
                }
            }
        });
        NameServer {
            port,
            stop,
            thread: Some(thread),
        }
    }

    fn files(&self, hosts: &str) -> NameFiles {
        NameFiles {
            hosts: hosts.to_string(),
            resolv_conf: "nameserver 127.0.0.1\noptions attempts:1\n".to_string(),
            port: i128::from(self.port),
        }
    }
}

impl Drop for NameServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The end of the question of `query` (its name, type and class).
fn question_end(query: &[u8]) -> usize {
    let mut at = 12;
    while query[at] != 0 {
        at += 1 + query[at] as usize;
    }
    at + 5
}

/// A reply to `query`: `id` and `question` as given, one `A` record for
/// 127.0.0.1 whose name is `owner`, for an `A` query; no answer otherwise.
fn reply(query: &[u8], id: [u8; 2], question: &[u8], owner: &[u8]) -> Vec<u8> {
    let kind = u16::from_be_bytes([
        query[question_end(query) - 4],
        query[question_end(query) - 3],
    ]);
    let mut out = vec![id[0], id[1], 0x81, 0x80, 0, 1];
    out.extend_from_slice(&u16::from(kind == 1).to_be_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(question);
    if kind == 1 {
        out.extend_from_slice(owner);
        out.extend_from_slice(&[0, 1, 0, 1, 0, 0, 1, 0x2c, 0, 4, 127, 0, 0, 1]);
    }
    out
}

fn honest(query: &[u8]) -> Vec<u8> {
    let question = &query[12..question_end(query)];
    reply(query, [query[0], query[1]], question, &[0xc0, 0x0c])
}

fn wrong_id(query: &[u8]) -> Vec<u8> {
    let question = &query[12..question_end(query)];
    reply(query, [query[0] ^ 1, query[1]], question, &[0xc0, 0x0c])
}

fn wrong_question(query: &[u8]) -> Vec<u8> {
    let question = [&[5u8][..], b"other", &[4], b"test", &[0, 0, 1, 0, 1]].concat();
    reply(query, [query[0], query[1]], &question, &[0xc0, 0x0c])
}

fn bad_compression_pointer(query: &[u8]) -> Vec<u8> {
    let question = &query[12..question_end(query)];
    // The name of the record points forward, past the end of the message.
    reply(query, [query[0], query[1]], question, &[0xc0, 0xff])
}

/// Connect to `name` on the loopback listener, on a real host whose name
/// resolution reads `files`, and say what happened: `connected` or the
/// error's text.
fn connect_by_name(name: &str, files: NameFiles) -> String {
    let src = format!(
        r#"
#[integration]
test "connect" {{
    let listener = t.net.listen_only().restrict_to_loopback(0)
    match t.net.restrict_to("{name}", listener.port()).connect() {{
        Ok(client) => {{
            t.assert(listener.accept().is_ok())
            client.close()
        }}
        Err(error) => {{
            let why = error.message.assume_trusted()
            t.fail("connect failed: {{why}}")
        }}
    }}
}}
"#
    );
    let report = run_tests(&src, &mut RealHost::new().with_name_files(files), true);
    assert_eq!(report.tests.len(), 1, "{report:#?}");
    match &report.tests[0].status {
        TestStatus::Passed => "connected".to_string(),
        _ => format!("{report:#?}"),
    }
}

#[test]
fn localhost_and_a_name_in_the_hosts_file_connect_through_std_dns() {
    // `printer.invalid` is a name no operating system resolver knows, and
    // nothing listens for a name server: only the hosts file can answer.
    let silent = NameServer::start(honest);
    let hosts = "127.0.0.1 localhost\n127.0.0.1 printer.invalid\n";
    for name in ["localhost", "printer.invalid"] {
        assert_eq!(connect_by_name(name, silent.files(hosts)), "connected");
    }
}

#[test]
fn a_name_from_a_test_name_server_connects_through_std_dns() {
    let server = NameServer::start(honest);
    assert_eq!(
        connect_by_name("svc.invalid", server.files("")),
        "connected"
    );
}

/// Connect to `name`, which must fail with an `IoError` whose text starts
/// `cannot connect to {name}:` and ends `: {why}`; true when it did.
fn fails_with(name: &str, why: &str, files: NameFiles) -> bool {
    let src = format!(
        r#"
#[integration]
test "connect" {{
    let listener = t.net.listen_only().restrict_to_loopback(0)
    match t.net.restrict_to("{name}", listener.port()).connect() {{
        Ok(_) => t.fail("connected")
        Err(error) => {{
            let text = "{{error}}"
            t.assert(text.starts_with("cannot connect to {name}:"))
            t.assert(text.ends_with(": {why}"))
        }}
    }}
}}
"#
    );
    let report = run_tests(&src, &mut RealHost::new().with_name_files(files), true);
    report.diagnostics.is_empty()
        && report.tests.len() == 1
        && report.tests[0].status == TestStatus::Passed
}

#[test]
fn a_hostile_answer_makes_connect_fail_with_an_io_error() {
    for (answer, why) in [
        (
            bad_compression_pointer as fn(&[u8]) -> Vec<u8>,
            "malformed answer",
        ),
        (wrong_question, "answer does not match the query"),
        (wrong_id, "timed out"),
    ] {
        let server = NameServer::start(answer);
        assert!(
            fails_with("hostile.invalid", why, server.files("")),
            "{why}"
        );
    }
}

#[test]
fn connect_to_a_name_the_hosts_file_and_name_server_lack_is_host_not_found() {
    fn nothing(query: &[u8]) -> Vec<u8> {
        let mut out = vec![query[0], query[1], 0x81, 0x83, 0, 1, 0, 0, 0, 0, 0, 0];
        out.extend_from_slice(&query[12..question_end(query)]);
        out
    }
    let server = NameServer::start(nothing);
    assert!(fails_with(
        "absent.invalid",
        "host not found",
        server.files("")
    ));
}
