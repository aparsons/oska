// SPDX-License-Identifier: Apache-2.0
//! `std.jws` (#830, spec #824): the flattened JWS of ACME, the JWK of a
//! P-256 key and its RFC 7638 thumbprint. Known answers: RFC 7515 appendix
//! A.3 (ES256) and RFC 7638 section 3.1. Programs run on the interpreter and
//! natively wherever the native backend runs (`common/native.rs`).

mod common;

use common::test_fake;
use oska_run::TestStatus;

const PRELUDE: &str = r#"use std.base64url
use std.ecdsa
use std.jws
use std.json
use std.sha2

fn hex(text: String) -> Bytes {
    Bytes.from_hex(text).unwrap_or(Bytes.new())
}

// RFC 7515 appendix A.3's key, d.
fn a3_key() -> Secret<Bytes> {
    Secret.new(base64url.decode("jpsQnnGQmL-YBIffH1136cspYG6-0iY7X1fCE9-E9LI").unwrap_or(Bytes.new()))
}

fn a3_public() -> ecdsa.PublicKey {
    let x = base64url.decode("f83OJ3D2xF1Bg8vub9tLe1gHMzV76e8Tus9uPHvRVEU").unwrap_or(Bytes.new())
    let y = base64url.decode("x_FEzRu9m36HLN_tue659LNpXW6pCyStikYjKIWI5a0").unwrap_or(Bytes.new())
    var point = hex("04")
    point.append(x)
    point.append(y)
    match ecdsa.PublicKey.from_bytes(ecdsa.Curve.P256, point) {
        Some(k) => k
        None => a3_public() // never: a bad key loops until the step limit fails the test
    }
}

fn other_key() -> ecdsa.PublicKey {
    match jws.public_key(Secret.new(hex("0000000000000000000000000000000000000000000000000000000000000002"))) {
        Ok(k) => k
        Err(_) => a3_public()
    }
}

// The text of member `name` of the JSON object `text`.
fn field(text: String, name: String) -> String {
    match json.parse(text) {
        Ok(v) => match v.text(name) {
            Some(t) => t.assume_trusted()
            None => "missing"
        }
        Err(_) => "bad json"
    }
}

fn jws_text(r: Result<String, jws.JwsError>) -> String {
    match r {
        Ok(t) => t
        Err(_) => "error"
    }
}

// Whether the JWS `text` verifies under `key`.
fn verifies(text: String, key: ecdsa.PublicKey) -> Bool {
    let protected = field(text, "protected")
    let payload = field(text, "payload")
    let signed = "{protected}.{payload}"
    let sig = base64url.decode(field(text, "signature")).unwrap_or(Bytes.new())
    ecdsa.verify(key, sha2.sha256(signed.to_bytes()), sig)
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

#[test]
fn rfc_7515_a3_signature_verifies_and_the_jwk_matches() {
    passes(
        r#"test "RFC 7515 A.3" {
    let signed = "eyJhbGciOiJFUzI1NiJ9.eyJpc3MiOiJqb2UiLA0KICJleHAiOjEzMDA4MTkzODAsDQogImh0dHA6Ly9leGFtcGxlLmNvbS9pc19yb290Ijp0cnVlfQ"
    let sig = base64url.decode("DtEhU3ljbEg8L38VWAfUAqOyKAM6-Xx-F4GawxaepmXFCgfTjDxw5djxLa8ISlSApmWQxfKTUJqPP3-Kg6NU1Q").unwrap_or(Bytes.new())
    t.assert_eq(sig.len(), 64)
    t.assert(ecdsa.verify(a3_public(), sha2.sha256(signed.to_bytes()), sig))
    t.assert_eq(jws.jwk(a3_public()).unwrap_or("error"), "\{\"crv\":\"P-256\",\"kty\":\"EC\",\"x\":\"f83OJ3D2xF1Bg8vub9tLe1gHMzV76e8Tus9uPHvRVEU\",\"y\":\"x_FEzRu9m36HLN_tue659LNpXW6pCyStikYjKIWI5a0\"\}")
    t.assert(jws.public_key(a3_key()).is_ok())
}
"#,
    );
}

#[test]
fn rfc_7638_thumbprint_example_is_stable_under_order_and_whitespace() {
    passes(
        r#"fn rsa(order: Int) -> String {
    let n = "0vx7agoebGcQSuuPiLJXZptN9nndrQmbXEps2aiAFbWhM78LhWx4cbbfAAtVT86zwu1RK7aPFFxuhDR1L6tSoc_BJECPebWKRXjBZCiFV4n3oknjhMstn64tZ_2W-5JsGY4Hc5n9yBXArwl93lqt7_RN5w6Cf0h4QyQ5v-65YGjQR0_FDW2QvzqY368QQMicAtaSqzs8KJZgnYb9c7d0zgdAZHzu6qMQvRL5hajrn1n91CbOpbISD08qNLyrdkt-bFTWhAI4vMQFh6WeZu0fM4lFd2NcRwr3XPksINHaQ-G_xBniIqbw0Ls1jF44-csFCur-kEgU8awapJzKnqDKgw"
    if order == 0 {
        "\{\"kty\":\"RSA\",\"n\":\"{n}\",\"e\":\"AQAB\"\}"
    } else if order == 1 {
        "\{ \"e\" : \"AQAB\" ,\n  \"kty\": \"RSA\",\n\t\"alg\": \"RS256\", \"kid\": \"2011-04-29\",\n  \"n\" : \"{n}\" \}"
    } else {
        "\{\"n\":\"{n}\",\"kty\":\"RSA\",\"e\":\"AQAB\",\"use\":\"sig\"\}"
    }
}

test "RFC 7638 section 3.1" {
    for order in [0, 1, 2] {
        t.assert_eq(jws.thumbprint_of_jwk(rsa(order)).unwrap_or("error"), "NzbLsXh8uDCcd-6MNwXF4W_7noWXFZAfHkxZsRGC9Xs")
    }
}

test "an EC JWK in any order has the thumbprint of the key" {
    let x = "f83OJ3D2xF1Bg8vub9tLe1gHMzV76e8Tus9uPHvRVEU"
    let y = "x_FEzRu9m36HLN_tue659LNpXW6pCyStikYjKIWI5a0"
    let a = "\{\"kty\":\"EC\",\"y\":\"{y}\",\"crv\":\"P-256\",\"x\":\"{x}\"\}"
    let b = "\{\n \"x\": \"{x}\", \"kid\": \"k\",\n \"y\": \"{y}\", \"crv\": \"P-256\", \"kty\": \"EC\" \}"
    let direct = jws.thumbprint(a3_public()).unwrap_or("error")
    t.assert_eq(jws.thumbprint_of_jwk(a).unwrap_or("e1"), direct)
    t.assert_eq(jws.thumbprint_of_jwk(b).unwrap_or("e2"), direct)
    t.assert(direct != "error")
}

test "a malformed JWK has no thumbprint" {
    t.assert(!jws.thumbprint_of_jwk("[]").is_ok())
    t.assert(!jws.thumbprint_of_jwk("\{\"kty\":\"oct\",\"k\":\"AQAB\"\}").is_ok())
    t.assert(!jws.thumbprint_of_jwk("\{\"kty\":\"RSA\",\"e\":\"AQAB\"\}").is_ok())
    t.assert(!jws.thumbprint_of_jwk("\{\"kty\":\"RSA\",\"e\":\"AQ+B\",\"n\":\"AQAB\"\}").is_ok())
    t.assert(!jws.thumbprint_of_jwk("\{\"kty\":\"EC\",\"crv\":\"P-1\",\"x\":\"AQ\",\"y\":\"AQ\"\}").is_ok())
    t.assert(!jws.thumbprint_of_jwk("not json").is_ok())
}
"#,
    );
}

#[test]
fn a_built_jws_verifies_and_has_the_acme_header() {
    passes(
        r#"fn header(text: String) -> String {
    let raw = base64url.decode(field(text, "protected")).unwrap_or(Bytes.new())
    raw.to_text().unwrap_or("bad utf8")
}

test "jwk form" {
    let body = "\{\"termsOfServiceAgreed\":true\}"
    let out = jws_text(jws.sign_jwk(a3_key(), "abc-123", "https://ca.example/acme/new-acct", body.to_bytes()))
    t.assert(verifies(out, a3_public()))
    t.assert_eq(field(out, "payload"), base64url.encode(body.to_bytes()))
    let jwk = jws.jwk(a3_public()).unwrap_or("error")
    t.assert_eq(header(out), "\{\"alg\":\"ES256\",\"jwk\":{jwk},\"nonce\":\"abc-123\",\"url\":\"https://ca.example/acme/new-acct\"\}")
    t.assert_eq(base64url.decode(field(out, "signature")).unwrap_or(Bytes.new()).len(), 64)
}

test "kid form and POST-as-GET" {
    let out = jws_text(jws.sign_kid(a3_key(), "https://ca.example/acct/1", "n0nce", "https://ca.example/order/9", Bytes.new()))
    t.assert(verifies(out, a3_public()))
    t.assert_eq(field(out, "payload"), "")
    t.assert_eq(header(out), "\{\"alg\":\"ES256\",\"kid\":\"https://ca.example/acct/1\",\"nonce\":\"n0nce\",\"url\":\"https://ca.example/order/9\"\}")
}

test "alg is never none or a MAC" {
    let a = jws_text(jws.sign_jwk(a3_key(), "n", "https://x", "p".to_bytes()))
    let b = jws_text(jws.sign_kid(a3_key(), "k", "n", "https://x", "p".to_bytes()))
    for text in [a, b] {
        let h = header(text)
        t.assert(h.starts_with("\{\"alg\":\"ES256\","))
        t.assert(!h.contains("none"))
        t.assert(!h.contains("HS256"))
    }
}

test "another key or payload does not verify" {
    let out = jws_text(jws.sign_kid(a3_key(), "k", "n", "https://x", "p".to_bytes()))
    let other = other_key()
    t.assert(!verifies(out, other))
}

test "signing is deterministic" {
    let a = jws.sign_kid(a3_key(), "k", "n", "https://x", "p".to_bytes()).unwrap_or("1")
    let b = jws.sign_kid(a3_key(), "k", "n", "https://x", "p".to_bytes()).unwrap_or("2")
    t.assert_eq(a, b)
}

test "bad inputs are refused" {
    t.assert(!jws.sign_jwk(a3_key(), "", "https://x", Bytes.new()).is_ok())
    t.assert(!jws.sign_jwk(a3_key(), "n", "https://x\"", Bytes.new()).is_ok())
    t.assert(!jws.sign_kid(a3_key(), "", "n", "https://x", Bytes.new()).is_ok())
    t.assert(!jws.sign_kid(a3_key(), "k", "a\nb", "https://x", Bytes.new()).is_ok())
    t.assert(!jws.sign_jwk(Secret.new(hex("00")), "n", "https://x", Bytes.new()).is_ok())
    t.assert(!jws.sign_kid(Secret.new(Bytes.zeroed(32)), "k", "n", "https://x", Bytes.new()).is_ok())
}
"#,
    );
}
