// SPDX-License-Identifier: Apache-2.0
//! `std.http` (#245): an HTTPS GET client written in Oska, a `std` module that
//! imports `std.tls`, linked into a program that imports it and checked under
//! the rules for user code. The request goes out over a `HostConnect`, so a
//! program cannot point `get` at another host; what comes back is untrusted;
//! the module has no `.assume_trusted()` and no audit site. What the module
//! does at run time is held by `crates/oskac/tests/native_http.rs` (a
//! loopback server) and `crates/oskac/tests/fixtures/http_reader.osk` (the
//! response parser, on both backends).

mod common;

use common::{assert_errors, assert_ok, check};

/// A program that uses every public item.
const EVERY_FUNCTION: &str = r#"use std.http
use std.tls

fn parse(data: Untrusted<Bytes>) -> Int {
    var reader = http.Reader.new(http.MAX_BODY)
    match reader.feed(data) {
        Ok(_) => {}
        Err(_) => { return -1 }
    }
    match reader.end() {
        Ok(_) => {}
        Err(_) => { return -2 }
    }
    if reader.is_done() {
        match reader.response() {
            Some(response) => { return response.status + response.headers.len() + response.body.len() }
            None => { return -3 }
        }
    }
    0
}

fn main(io: Stdout, net: Net, random: Random, clock: Clock, args: Args) -> Result<(), IoError> {
    let host = net.restrict_to("example.test", 443)
    let request = http.Request.get(args.get(1).unwrap_or("/")).with_header("Accept", args.get(2).unwrap_or("*/*")).with_max_body(1000)
    match http.get(host, request, tls.TlsConfig.default(), random, clock) {
        Ok(response) => {
            let status = response.status
            io.println("{status}")?
            let kind = response.header("content-type").unwrap_or("none")
            io.println(kind)?
            let n = response.body.len() + http.MAX_HEAD + http.MAX_FIELDS
            io.println("{n}")?
        }
        Err(error) => {
            let kind = match error {
                http.HttpError.Tls(_) => "tls"
                http.HttpError.Malformed(_) => "malformed"
                http.HttpError.TooLarge => "too large"
                http.HttpError.InvalidRequest(_) => "invalid"
            }
            io.println(kind)?
            io.println(error.to_string())?
        }
    }
    let n = parse("HTTP/1.1 204 No Content\r\n\r\n".to_bytes())
    io.println("{n}")
}
"#;

fn function<'r>(
    report: &'r oska_check::CheckReport,
    name: &str,
) -> &'r oska_check::FunctionAuthority {
    report
        .functions
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("the report should name `{name}`"))
}

#[test]
fn a_program_calling_every_item_checks_and_lowers() {
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
        "fn f() -> Int {\n    http.MAX_BODY\n}",
        &[("E0221", "http")],
    );
}

#[test]
fn get_takes_the_host_capability_and_no_other_way_to_a_host() {
    let report = check(EVERY_FUNCTION);
    assert_eq!(
        function(&report, "main").authority,
        ["Stdout", "Net", "Random", "Clock"]
    );
    // The host a request goes to is the capability's own: a name is not one.
    assert_errors(
        "use std.http\nuse std.tls\n\
         fn f(random: Random, clock: Clock) -> Bool {\n    \
             http.get(\"example.test\", http.Request.get(\"/\"), tls.TlsConfig.default(), random, clock).is_ok()\n}",
        &[("E0220", "\"example.test\"")],
    );
    // A connect-only capability is not a host capability.
    assert_errors(
        "use std.http\nuse std.tls\n\
         fn f(net: NetConnect, random: Random, clock: Clock) -> Bool {\n    \
             http.get(net, http.Request.get(\"/\"), tls.TlsConfig.default(), random, clock).is_ok()\n}",
        &[("E0220", "net")],
    );
}

#[test]
fn a_path_and_a_header_value_may_be_untrusted_but_a_header_name_may_not() {
    assert_ok(
        "use std.http\n\
         fn make(path: Untrusted<String>, value: Untrusted<String>) -> http.Request {\n    \
             http.Request.get(path).with_header(\"X-Id\", value)\n}",
    );
    assert_errors(
        "use std.http\n\
         fn make(name: Untrusted<String>) -> http.Request {\n    \
             http.Request.get(\"/\").with_header(name, \"1\")\n}",
        &[("E0300", "name")],
    );
}

// ---- Trust ---------------------------------------------------------------------------

#[test]
fn the_headers_and_body_of_a_response_are_untrusted() {
    assert_errors(
        "use std.http\n\
         fn body(r: http.Response) -> Bytes {\n    r.body\n}",
        &[("E0300", "r.body")],
    );
    assert_errors(
        "use std.http\n\
         fn type_of(r: http.Response) -> String {\n    r.header(\"content-type\").unwrap_or(\"\")\n}",
        &[("E0300", "r.header(\"content-type\").unwrap_or(\"\")")],
    );
    assert_errors(
        "use std.http\n\
         fn name(h: http.Header) -> String {\n    h.name\n}",
        &[("E0300", "h.name")],
    );
    // The status is a number the parser read, so it is trusted.
    assert_ok("use std.http\nfn status(r: http.Response) -> Int {\n    r.status\n}");
}

#[test]
fn a_request_and_the_parser_state_are_private() {
    let forged = check(
        "use std.http\n\
         fn forge() -> http.Request {\n    http.Request(path: \"/\", fields: [], max_body: 0)\n}",
    );
    assert!(
        !forged.diagnostics.is_empty(),
        "a request has private fields"
    );
    let peek = check(
        "use std.http\n\
         fn peek(r: http.Reader) -> Int {\n    r.phase\n}",
    );
    let codes: Vec<&str> = peek.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, ["E0360"], "{:#?}", peek.diagnostics);
}

// ---- Purity and audit sites ---------------------------------------------------------

#[test]
fn the_parser_is_pure_and_only_get_has_authority() {
    // The report lists only a program's own functions: the module's are the
    // toolchain's code. So authority is checked on the module itself, as a
    // file, where every function it declares, `pub` or not, is named.
    let report = check(include_str!("../src/std/http.osk"));
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    for f in &report.functions {
        if f.name != "get" {
            assert!(f.pure, "`{}` should be pure: {:?}", f.name, f.authority);
        }
    }
    assert_eq!(
        function(&report, "get").authority,
        ["HostConnect", "Random", "Clock"]
    );
}

#[test]
fn the_module_has_no_trust_assertion_and_no_audit_site() {
    let source = include_str!("../src/std/http.osk");
    let report = check(source);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let lines: Vec<&str> = source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect();
    assert!(
        !lines.iter().any(|line| line.contains(".assume_trusted()")),
        "`std.http` uses `.assume_trusted()`"
    );
    assert!(
        !lines.iter().any(|line| line.contains("expose()")),
        "`std.http` handles no secret"
    );
    assert!(report.audit.is_empty(), "{:#?}", report.audit);
}
