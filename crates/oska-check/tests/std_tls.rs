// SPDX-License-Identifier: Apache-2.0
//! `std.tls` (#244): a TLS 1.3 client written in Oska, a `std` module that
//! imports the crypto modules and `std.x509`, linked into a program that
//! imports it and checked under the rules for user code. The engine is pure:
//! no capability in a signature, so it is deterministic (ADR 0002); the
//! stream and `connect` carry a `HostConnect`, a `Random` and a `Clock`. The
//! module has no `.assume_trusted()` (nor has `std.x509`), and its
//! `expose()` sites are the short, reviewed list below, held by the report's
//! audit list for the module's own file. What the module does at run time is
//! held by `crates/oskac/tests/native_tls.rs`.

mod common;

use common::{assert_errors, assert_ok, check};
use oska_check::{AuditKind, CheckReport};

/// The public functions and methods of `std.tls`, as the authority report
/// names them.
const FUNCTIONS: [&str; 14] = [
    "TlsConfig.default",
    "TlsConfig.with_trust",
    "TlsConfig.with_alpn",
    "Engine.new",
    "Engine.is_connected",
    "Engine.selected_protocol",
    "Engine.start",
    "Engine.receive",
    "Engine.end_of_input",
    "Engine.send",
    "Engine.close",
    "TlsStream.send",
    "TlsStream.receive",
    "connect",
];

/// Every `expose()` of the module, as the text of the line it is on: the
/// whole list. A secret becomes bytes only for a value that is public by
/// design (the key share, the random and legacy session ID the client sends
/// in the clear, the IV of a traffic key, which a record's nonce is made of)
/// or after it has been checked or sealed (the plaintext of a record whose
/// tag verified, the ciphertext to send).
const EXPOSED: [&str; 8] = [
    "self.write_iv = iv.expose()",
    "self.read_iv = iv.expose()",
    "self.out.append(sealed.expose())",
    "let plain = opened.expose()",
    "Some(bytes) => { client_random = bytes.expose() }",
    "Some(bytes) => { session_id = bytes.expose() }",
    "Some(point) => { share = point.expose() }",
    "self.share = point.expose()",
];

fn function<'r>(report: &'r CheckReport, name: &str) -> &'r oska_check::FunctionAuthority {
    report
        .functions
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| {
            let names: Vec<&str> = report.functions.iter().map(|f| f.name.as_str()).collect();
            panic!("the report should name `{name}`: {names:?}")
        })
}

/// A program that uses every public function and the engine's events.
const EVERY_FUNCTION: &str = r#"use std.tls
use std.x509

fn pump(engine: tls.Engine, data: Untrusted<Bytes>) -> Int {
    var e = engine
    let step = e.receive(data)
    var count = step.output.len()
    for event in step.events {
        match event {
            tls.Event.HandshakeDone => { count = count + 1 }
            tls.Event.Data(bytes) => { count = count + bytes.len() }
            tls.Event.Closed => { count = count + 1 }
            tls.Event.Failed(_) => { count = count + 1 }
        }
    }
    count
}

fn engine_calls(random: Secret<Bytes>) -> Int {
    let config = tls.TlsConfig.default().with_trust(x509.TrustConfig.default()).with_alpn(["http/1.1"])
    var e = tls.Engine.new("example.test", config, random, 1750000000)
    let first = e.start()
    var total = first.output.len()
    total = total + pump(e, Bytes.new())
    let sent = e.send("hello".to_bytes())
    let closing = e.close()
    let ended = e.end_of_input()
    if e.is_connected() {
        total = total + 1
    }
    let chosen = e.selected_protocol().unwrap_or("none")
    total + sent.output.len() + closing.output.len() + ended.output.len() + chosen.len()
}

fn main(io: Stdout, net: Net, random: Random, clock: Clock) -> Result<(), IoError> {
    let host = net.restrict_to("example.test", 443)
    match tls.connect(host, tls.TlsConfig.default(), random, clock) {
        Ok(stream) => {
            var open = stream
            let _ = open.send("GET / HTTP/1.1\r\n\r\n".to_bytes())
            match open.receive() {
                Ok(data) => {
                    let n = data.len()
                    io.println("{n}")?
                }
                Err(error) => {
                    io.println("{error}")?
                }
            }
            open.close()
        }
        Err(error) => {
            io.println("{error}")?
        }
    }
    let n = engine_calls(random.secure_bytes(64))
    io.println("{n}")
}
"#;

// ---- Importing it ------------------------------------------------------------------

#[test]
fn a_program_calling_every_function_checks_and_lowers() {
    let report = check(EVERY_FUNCTION);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    assert!(report.program.is_some(), "a clean program lowers");
    assert!(
        report.audit.is_empty(),
        "importing the module adds no audit site: {:#?}",
        report.audit
    );
}

#[test]
fn without_its_use_the_module_is_unknown() {
    assert_errors(
        "fn f(host: HostConnect, random: Random, clock: Clock) -> Bool {\n    \
             tls.connect(host, tls.TlsConfig.default(), random, clock).is_ok()\n}",
        &[("E0221", "tls"), ("E0221", "tls")],
    );
}

// ---- Authority ---------------------------------------------------------------------

#[test]
fn connect_takes_the_host_capability_and_no_other_way_to_a_host() {
    let report = check(EVERY_FUNCTION);
    let main = function(&report, "main");
    assert_eq!(main.authority, ["Stdout", "Net", "Random", "Clock"]);
    // The name sent and checked is the capability's own host: a program
    // gives `connect` a `HostConnect`, never a name.
    assert_errors(
        "use std.tls\n\
         fn f(random: Random, clock: Clock) -> Bool {\n    \
             tls.connect(\"example.test\", tls.TlsConfig.default(), random, clock).is_ok()\n}",
        &[("E0220", "\"example.test\"")],
    );
    // A connect-only capability is not a host capability.
    assert_errors(
        "use std.tls\n\
         fn f(net: NetConnect, random: Random, clock: Clock) -> Bool {\n    \
             tls.connect(net, tls.TlsConfig.default(), random, clock).is_ok()\n}",
        &[("E0220", "net")],
    );
}

#[test]
fn the_engine_takes_the_server_name_as_trusted_text() {
    // Which name the certificate is checked for is the program's choice: an
    // untrusted name needs the caller's own trust assertion.
    assert_errors(
        "use std.tls\n\
         fn make(name: Untrusted<String>, random: Secret<Bytes>) -> tls.Engine {\n    \
             tls.Engine.new(name, tls.TlsConfig.default(), random, 0)\n}",
        &[("E0300", "name")],
    );
}

// ---- Trust ---------------------------------------------------------------------------

#[test]
fn what_the_stream_receives_is_untrusted_bytes() {
    assert_errors(
        "use std.tls\n\
         fn first(stream: tls.TlsStream) -> Bytes {\n    \
             var open = stream\n    \
             match open.receive() {\n        \
                 Ok(data) => data\n        \
                 Err(_) => Bytes.new()\n    }\n}",
        &[("E0300", "data")],
    );
}

#[test]
fn what_is_sent_may_be_untrusted() {
    assert_ok(
        "use std.tls\n\
         fn forward(stream: tls.TlsStream, body: Untrusted<Bytes>) -> Bool {\n    \
             var open = stream\n    \
             open.send(body).is_ok()\n}",
    );
}

#[test]
fn a_program_cannot_change_when_the_keys_change() {
    // The rekey interval and the limit on records read under one key are
    // fixed by RFC 8446, section 5.5: no function of the module lets a
    // program choose them (the tests build a copy of the module with other
    // constants, see `crates/oskac/tests/native_tls.rs`).
    for call in ["with_rekey_after(1)", "with_read_limit(1)"] {
        let src = format!(
            "use std.tls\nfn f() -> tls.TlsConfig {{\n    tls.TlsConfig.default().{call}\n}}"
        );
        let report = check(&src);
        assert!(!report.diagnostics.is_empty(), "`{call}` should not check");
    }
    let source = include_str!("../src/std/tls.osk");
    assert!(
        !source.contains("pub fn with_rekey_after"),
        "the rekey hook is not public"
    );
}

#[test]
fn the_engines_state_is_private_and_a_stream_cannot_be_forged() {
    let report = check(
        "use std.tls\n\
         fn peek(e: tls.Engine) -> Int {\n    e.state\n}",
    );
    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, ["E0360"], "{:#?}", report.diagnostics);
    let forged = check(
        "use std.tls\n\
         fn forge(c: Connection, e: tls.Engine) -> tls.TlsStream {\n    \
             tls.TlsStream(connection: c, engine: e, pending: Bytes.new(), closed: false, failure: None)\n}",
    );
    assert!(
        !forged.diagnostics.is_empty(),
        "a stream has private fields"
    );
}

// ---- Purity and audit sites ---------------------------------------------------------

#[test]
fn the_engine_is_pure_and_the_stream_is_not() {
    // The report lists only a program's own functions: the module's are the
    // toolchain's code. So authority is checked on the module itself, as a
    // file, where every function it declares, `pub` or not, is named.
    let report = check(include_str!("../src/std/tls.osk"));
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    for name in FUNCTIONS {
        let f = function(&report, name);
        if name.starts_with("Engine.") || name.starts_with("TlsConfig.") {
            assert!(f.pure, "`{name}` should be pure: {:?}", f.authority);
        }
    }
    for f in &report.functions {
        if !f.name.starts_with("TlsStream.") && f.name != "connect" {
            assert!(f.pure, "`{}` should be pure: {:?}", f.name, f.authority);
        }
    }
    assert_eq!(
        function(&report, "connect").authority,
        ["HostConnect", "Random", "Clock"]
    );
}

#[test]
fn the_module_has_no_trust_assertion_and_a_short_list_of_exposures() {
    let source = include_str!("../src/std/tls.osk");
    let report = check(source);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let code: Vec<&str> = source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect();
    for module in [source, include_str!("../src/std/x509.osk")] {
        let lines: Vec<&str> = module
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect();
        assert!(
            !lines.iter().any(|line| line.contains(".assume_trusted()")),
            "a std module uses `.assume_trusted()`"
        );
    }
    assert!(
        report
            .audit
            .iter()
            .all(|site| site.kind == AuditKind::SecretExposure),
        "only `expose()` appears: {:#?}",
        report.audit
    );
    let mut found: Vec<String> = report
        .audit
        .iter()
        .map(|site| {
            let line = source[..site.span.start].matches('\n').count();
            code.iter()
                .find(|l| source.lines().nth(line) == Some(**l))
                .map(|l| l.trim().to_string())
                .unwrap_or_default()
        })
        .collect();
    found.sort();
    let mut wanted: Vec<String> = EXPOSED.iter().map(|l| l.to_string()).collect();
    wanted.sort();
    assert_eq!(
        found, wanted,
        "`std.tls` exposes a secret only where the list says; a new site needs review and a line in the list, and in the module's header"
    );
}

#[test]
fn the_crypto_modules_it_uses_have_no_audit_site() {
    for source in [
        include_str!("../src/std/hkdf.osk"),
        include_str!("../src/std/hmac.osk"),
        include_str!("../src/std/x25519.osk"),
        include_str!("../src/std/p256.osk"),
        include_str!("../src/std/chacha20poly1305.osk"),
        include_str!("../src/std/aes128gcm.osk"),
        include_str!("../src/std/sha2.osk"),
        include_str!("../src/std/montgomery.osk"),
        include_str!("../src/std/ecdsa.osk"),
        include_str!("../src/std/rsa.osk"),
        include_str!("../src/std/ed25519.osk"),
        include_str!("../src/std/x509.osk"),
    ] {
        let report = check(source);
        assert!(report.audit.is_empty(), "{:#?}", report.audit);
    }
}

/// #699: `std.dns` and `std.tls` (and the runtime's `rt.osk`) never match an
/// error by its message text. The one reading of an `IoError`'s text is
/// `std.net_error`'s `classify`, so a change of wording cannot change what
/// the modules do.
/// The test hook of the RFC 8448 replay (#695, `oskac`'s `tls_rfc8448.rs`)
/// exists only in a copy of the module that the test builds. The module a
/// program imports has no such name, and a program cannot call one.
#[test]
fn the_rfc_8448_hook_is_not_reachable_from_a_program() {
    let module = include_str!("../src/std/tls.osk");
    assert!(
        !module.to_lowercase().contains("rfc8448"),
        "std.tls must not hold the test hook"
    );
    assert_errors(
        "use std.tls\n\
         fn f() -> Int {\n    \
             var e = tls.rfc8448_engine(tls.TlsConfig.default())\n    \
             e.start().output.len()\n}",
        &[("E0312", "rfc8448_engine")],
    );
    assert_errors(
        "use std.tls\n\
         fn f() -> Int {\n    \
             let e = tls.Engine.rfc8448(tls.TlsConfig.default())\n    \
             0\n}",
        &[("E0228", "rfc8448")],
    );
}

#[test]
fn no_module_matches_an_error_by_its_message_text() {
    for (name, source) in [
        ("tls.osk", include_str!("../src/std/tls.osk")),
        ("dns.osk", include_str!("../src/std/dns.osk")),
        ("rt.osk", include_str!("../src/std/rt.osk")),
    ] {
        for (n, line) in source.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            let reads_message = code.contains("error.message")
                || code.contains("e.message.contains")
                || code.contains("message.contains(")
                || code.contains("message.ends_with(")
                || code.contains("message.starts_with(")
                || code.contains("message ==");
            assert!(
                !reads_message,
                "{name}:{}: matches a message: {line}",
                n + 1
            );
        }
    }
}
