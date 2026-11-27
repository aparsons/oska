// SPDX-License-Identifier: Apache-2.0
//! `std.x509_csr` (#833, spec #824): a certificate signing request signed
//! with ECDSA P-256 over a `Secret` key. Every program runs through the
//! `oska-run` API on a `FakeHost` and, where native executables run, natively
//! (`common/native.rs`); `crates/oskac/tests/native_x509_csr.rs` runs one on
//! every supported platform.
//!
//! The fixtures were made by an independent tool: `cryptography` 50
//! (`CertificateSigningRequestBuilder.sign(..., ecdsa_deterministic=True)`,
//! RFC 6979) for the key of RFC 6979 appendix A.2.5, and checked with
//! `openssl req -verify`. The request this module writes is byte for byte
//! the one `cryptography` writes, and the one `cryptography` writes parses
//! and verifies here.

mod common;

use common::{run_fake, test_fake};
use oska_run::TestStatus;

/// The key of RFC 6979 appendix A.2.5.
const KEY: &str = "c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721";

/// A request with an empty subject for `example.com` and `www.example.com`.
const TWO: &str = "3081f930819f02010030003059301306072a8648ce3d020106082a8648ce3d0301070342000460fed4ba255a9d31c961eb74c6356d68c049b8923b61fa6ce669622e60f29fb67903fe1008b8bc99a41ae9e95628bc64f2f1b20c2d7e9f5177a3c294d4462299a03d303b06092a864886f70d01090e312e302c302a0603551d110101ff0420301e820b6578616d706c652e636f6d820f7777772e6578616d706c652e636f6d300a06082a8648ce3d0403020349003046022100d4520617b5ecffc33d842b1d499250a1055997621c1d4fedb4cedda053c851d5022100adb8625febf48d5c7ea00a5c74fbe43145abd87cb3a63f332478210ab4715dbf";

/// A request with the common name `example.com` for a DNS name, a wildcard
/// and an IPv4 and an IPv6 address.
const MIXED: &str = "308201213081c802010030163114301206035504030c0b6578616d706c652e636f6d3059301306072a8648ce3d020106082a8648ce3d0301070342000460fed4ba255a9d31c961eb74c6356d68c049b8923b61fa6ce669622e60f29fb67903fe1008b8bc99a41ae9e95628bc64f2f1b20c2d7e9f5177a3c294d4462299a050304e06092a864886f70d01090e3141303f303d0603551d1104363034820b6578616d706c652e636f6d820d2a2e6578616d706c652e636f6d8704c0000201871020010db8000000000000000000000001300a06082a8648ce3d0403020348003045022029e40558581764c452d4e629f816d327352a88d90eed61cbdd0a32fc676620e3022100ea442e42d33430088110bb67620e9bdef67e29993834a133902db0f29c62d7d7";

const PRELUDE: &str = r#"use std.x509_csr
use std.base64url

fn hex(text: String) -> Bytes {
    Bytes.from_hex(text).unwrap_or(Bytes.new())
}

fn key(text: String) -> Secret<Bytes> {
    Secret.new(hex(text))
}

// The hexadecimal request, or the error's text.
fn made(k: String, cn: String, names: List<String>) -> String {
    match x509_csr.create_with_common_name(key(k), cn, names) {
        Ok(der) => der.to_hex()
        Err(e) => e.to_string()
    }
}

fn names_of(text: String) -> String {
    describe(hex(text))
}

fn describe(der: Bytes) -> String {
    match x509_csr.parse(der) {
        Ok(csr) => {
            let dns = csr.dns_names().len()
            let ips = csr.ip_addresses().len()
            let cn = csr.common_name()
            "{cn}|{dns}|{ips}"
        }
        Err(e) => e.to_string()
    }
}
"#;

fn passes(body: &str) {
    let src = format!("{PRELUDE}\n{body}");
    let report = test_fake(&src);
    assert!(report.diagnostics.is_empty(), "{report:#?}");
    assert!(!report.tests.is_empty(), "{report:#?}");
    for test in &report.tests {
        assert_eq!(test.status, TestStatus::Passed, "{report:#?}");
    }
}

fn refused(body: &str) -> Vec<String> {
    let src = format!("{PRELUDE}\n{body}");
    let outcome = run_fake(&src, &[]);
    assert!(
        !outcome.diagnostics.is_empty(),
        "should be refused: {outcome:#?}"
    );
    assert_eq!(outcome.stdout, "", "a refused program prints nothing");
    outcome
        .diagnostics
        .iter()
        .map(|d| d.code.to_string())
        .collect()
}

#[test]
fn a_request_for_two_dns_names_is_the_one_an_independent_tool_writes() {
    passes(&format!(
        r#"test "two names" {{
    t.assert_eq(made("{KEY}", "", ["example.com", "www.example.com"]), "{TWO}")
    t.assert_eq(names_of("{TWO}"), "|2|0")
}}
"#
    ));
}

#[test]
fn a_request_with_a_common_name_wildcard_and_ip_addresses_matches_too() {
    passes(&format!(
        r#"test "mixed names" {{
    t.assert_eq(made("{KEY}", "example.com", ["example.com", "*.example.com", "192.0.2.1", "2001:db8::1"]), "{MIXED}")
    t.assert_eq(names_of("{MIXED}"), "example.com|2|2")
}}
"#
    ));
}

#[test]
fn what_it_writes_parses_and_verifies_with_the_names_and_key_it_carries() {
    passes(&format!(
        r#"test "round trip" {{
    match x509_csr.create(key("{KEY}"), ["Example.COM", "10.0.0.1", "::1"]) {{
        Ok(der) => match x509_csr.parse(der) {{
            Ok(csr) => {{
                t.assert_eq(csr.dns_names(), ["example.com"])
                t.assert_eq(csr.ip_addresses().len(), 2)
                t.assert_eq(csr.ip_addresses().at(0).to_hex(), "0a000001")
                t.assert_eq(csr.ip_addresses().at(1).to_hex(), "00000000000000000000000000000001")
                t.assert_eq(csr.to_der(), der)
                t.assert_eq(csr.public_key().to_bytes().len(), 65)
            }}
            Err(e) => t.fail(e.to_string())
        }}
        Err(e) => t.fail(e.to_string())
    }}
}}
"#
    ));
}

#[test]
fn the_base64url_of_a_request_is_what_acme_sends() {
    passes(&format!(
        r#"test "acme finalize" {{
    let der = hex("{TWO}")
    match base64url.decode(base64url.encode(der)) {{
        Ok(back) => t.assert_eq(back, der)
        Err(e) => t.fail(e.to_string())
    }}
}}
"#
    ));
}

#[test]
fn names_that_are_not_dns_names_or_ip_addresses_are_refused() {
    let bad = [
        "",
        " ",
        "a b.com",
        "-a.com",
        "a-.com",
        "a..com",
        ".a.com",
        "a.com.",
        "a_b.com",
        "ex*ample.com",
        "*",
        "*.",
        "**.a.com",
        "a.*.com",
        "1.2.3",
        "1.2.3.4.5",
        "999.1.1.1",
        "01.2.3.4",
        "1.2.3.4/24",
        "1234",
        "a.123",
        "xn--\u{e9}.com",
        "caf\u{e9}.com",
        "\u{212a}.com",
        "[::1]",
        ":::",
        "1::2::3",
        "1:2:3:4:5:6:7:8:9",
        "12345::1",
        "::g",
        "::ffff:1.2.3.4",
        "fe80::1%eth0",
        "http://a.com",
        "a.com:443",
    ];
    let mut body = String::from("test \"bad names\" {\n");
    for name in bad {
        body.push_str(&format!(
            "    t.assert_eq(made(\"{KEY}\", \"\", [\"{name}\"]), \"a name is not a valid DNS name or IP address\")\n"
        ));
    }
    let long_label = "a".repeat(64);
    let long_name = format!("{}.com", vec!["a".repeat(63); 4].join("."));
    for name in [long_label, long_name] {
        body.push_str(&format!(
            "    t.assert_eq(made(\"{KEY}\", \"\", [\"{name}\"]), \"a name is not a valid DNS name or IP address\")\n"
        ));
    }
    body.push_str("}\n");
    passes(&body);
}

#[test]
fn a_bad_name_among_good_ones_refuses_the_request() {
    passes(&format!(
        r#"test "one bad name" {{
    t.assert_eq(made("{KEY}", "", ["example.com", "bad name", "192.0.2.1"]), "a name is not a valid DNS name or IP address")
}}
"#
    ));
}

#[test]
fn the_count_of_names_and_the_common_name_are_checked() {
    let many: Vec<String> = (0..101).map(|i| format!("\"h{i}.example.com\"")).collect();
    let hundred: Vec<String> = (0..100).map(|i| format!("\"h{i}.example.com\"")).collect();
    passes(&format!(
        r#"test "counts" {{
    t.assert_eq(made("{KEY}", "", []), "the request has no names")
    t.assert_eq(made("{KEY}", "", [{}]), "the request has more than 100 names")
    t.assert(made("{KEY}", "", [{}]).len() > 1000)
    t.assert_eq(made("{KEY}", "bad name", ["a.com"]), "the common name is not a valid DNS name")
    t.assert_eq(made("{KEY}", "{}.com", ["a.com"]), "the common name is not a valid DNS name")
}}
"#,
        many.join(", "),
        hundred.join(", "),
        "a".repeat(62)
    ));
}

#[test]
fn a_key_that_is_not_a_private_key_signs_nothing() {
    passes(
        r#"test "bad keys" {
    let message = "the key is not a valid P-256 key"
    t.assert_eq(made("00", "", ["a.com"]), message)
    t.assert_eq(made("0000000000000000000000000000000000000000000000000000000000000000", "", ["a.com"]), message)
    t.assert_eq(made("ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551", "", ["a.com"]), message)
}
"#,
    );
}

#[test]
fn a_request_with_any_byte_changed_does_not_parse_or_does_not_verify() {
    passes(&format!(
        r#"test "tampered" {{
    let good = hex("{TWO}")
    t.assert_eq(describe(good), "|2|0")
    for i in 0..good.len() {{
        var copy = Bytes.new()
        for j in 0..good.len() {{
            if j == i {{
                copy.push(good.at(j) ^ 1)
            }} else {{
                copy.push(good.at(j))
            }}
        }}
        t.assert(describe(copy) != "|2|0")
    }}
    var longer = good
    longer.push(0)
    t.assert(describe(longer) != "|2|0")
    t.assert(describe(good.slice(0, good.len() - 1)) != "|2|0")
}}
"#
    ));
}

#[test]
fn the_same_key_and_names_give_the_same_request() {
    passes(&format!(
        r#"test "deterministic" {{
    t.assert_eq(made("{KEY}", "", ["a.com"]), made("{KEY}", "", ["a.com"]))
    t.assert(made("{KEY}", "", ["a.com"]) != made("{KEY}", "", ["b.com"]))
}}
"#
    ));
}

#[test]
fn the_private_key_cannot_be_printed_or_interpolated() {
    let codes = refused(
        r#"fn main(io: Stdout) -> Result<(), IoError> {
    let k = key("c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721")
    io.println("{k}")
}
"#,
    );
    assert!(!codes.is_empty());
}

#[test]
fn the_module_exposes_only_the_public_point_and_the_signature() {
    let source = include_str!("../../oska-check/src/std/x509_csr.osk");
    let uses: Vec<&str> = source
        .lines()
        .filter(|l| !l.trim_start().starts_with("//") && l.contains(".expose("))
        .collect();
    assert_eq!(uses.len(), 2, "{uses:?}");
    assert!(uses[0].contains("secret_point"), "{uses:?}");
    assert!(uses[1].contains("secret_signature"), "{uses:?}");
    assert!(!source
        .lines()
        .any(|l| !l.trim_start().starts_with("//") && l.contains(".assume_trusted(")));
}
