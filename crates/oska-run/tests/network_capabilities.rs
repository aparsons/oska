//! Host-narrowed network access on the interpreter (spec #228, ADR 0023):
//! `restrict_to(host, port)` on `Net` or `NetConnect` gives the only
//! capability that connects, `restrict_to_loopback(port)` on `NetListen`
//! gives the test server's listener, and the test host's `t.net` refuses
//! every connection in a plain test.
//!
//! These run the interpreter; the native backend's are in
//! `crates/oskac/tests/native_sockets.rs`. The real-network tests use only
//! the loopback address.

use std::net::TcpListener;

use oska_run::{
    run, test, FakeHost, Host, RealHost, RunOutcome, TestOptions, TestReport, TestStatus,
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
