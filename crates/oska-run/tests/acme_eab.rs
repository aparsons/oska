// SPDX-License-Identifier: Apache-2.0
//! `std.acme` External Account Binding (#836, spec #824, RFC 8555 section
//! 7.3.4) against a recorded test CA. The CA is a `Transport` written in
//! Oska that checks the binding the way a real one does: the
//! `externalAccountBinding` member of `newAccount` must be a flattened JWS
//! with `alg` HS256, the caller's `kid`, the `newAccount` URL and no nonce,
//! whose payload is the account key's JWK and whose signature is the
//! HMAC-SHA-256 of `protected.payload` under the CA's key. A request with
//! no binding gets `externalAccountRequired`; a wrong key or key id gets
//! `unauthorized`. Programs run on `FakeHost`.
//!
//! Held here: a transcript with EAB creates an account; the key id and key
//! come from the caller; a missing or wrong binding is the CA's typed
//! error; the HMAC key never prints, interpolates or leaves the client.

mod common;

use common::{run_fake, test_fake};
use oska_run::TestStatus;

const PRELUDE: &str = r#"use std.acme
use std.base64url
use std.hmac
use std.http
use std.json
use std.jws

const D: String = "jpsQnnGQmL-YBIffH1136cspYG6-0iY7X1fCE9-E9LI"
// The CA's key for the key id "kid-1", in base64url as a CA shows it.
const MAC: String = "c2VjcmV0LWVhYi1rZXktMDEyMzQ1Njc4OWFiY2RlZg"
const DIR: String = "https://ca.example/dir"
const NEW_NONCE: String = "https://ca.example/nonce"
const NEW_ACCT: String = "https://ca.example/new-acct"
const NEW_ORDER: String = "https://ca.example/new-order"
const ACCT: String = "https://ca.example/acct/1"

fn key() -> Secret<Bytes> {
    Secret.new(base64url.decode(D).unwrap_or(Bytes.new()))
}

fn mac_key() -> Secret<Bytes> {
    Secret.new(base64url.decode(MAC).unwrap_or(Bytes.new()))
}

fn wrong_key() -> Secret<Bytes> {
    Secret.new(base64url.decode("d3Jvbmcta2V5LXdyb25nLWtleS13cm9uZy1rZXk").unwrap_or(Bytes.new()))
}

fn field(v: json.Value, name: String) -> String {
    v.text(name).unwrap_or("").assume_trusted()
}

fn problem(name: String) -> http.Response {
    http.Response(status: 403, headers: [http.Header(name: "replay-nonce", value: "n1")], body: "\{\"type\":\"urn:ietf:params:acme:error:{name}\",\"detail\":\"binding\"\}".to_bytes())
}

fn dir_body() -> String {
    "\{\"newNonce\":\"{NEW_NONCE}\",\"newAccount\":\"{NEW_ACCT}\",\"newOrder\":\"{NEW_ORDER}\",\"meta\":\{\"externalAccountRequired\":true\}\}"
}

// What the CA made of the binding in a `newAccount` request: "ok", or why
// not. `leak` marks a request that holds the account key or the MAC key.
fn check_binding(body: Bytes, want_kid: String) -> String {
    let text = body.to_text().unwrap_or("")
    if text.contains(D) || text.contains(MAC) {
        return "leak"
    }
    let outer = match json.parse(text) {
        Ok(v) => v
        Err(_) => { return "no jws" }
    }
    let protected = field(outer, "protected")
    let header_text = base64url.decode(protected).unwrap_or(Bytes.new()).to_text().unwrap_or("")
    let header = match json.parse(header_text) {
        Ok(h) => h
        Err(_) => { return "no header" }
    }
    let payload_text = base64url.decode(field(outer, "payload")).unwrap_or(Bytes.new()).to_text().unwrap_or("")
    let payload = match json.parse(payload_text) {
        Ok(p) => p
        Err(_) => { return "no payload" }
    }
    let eab = match payload.object("externalAccountBinding") {
        Some(b) => b
        None => { return "missing" }
    }
    let inner_protected = field(eab, "protected")
    let inner_header_text = base64url.decode(inner_protected).unwrap_or(Bytes.new()).to_text().unwrap_or("")
    let inner = match json.parse(inner_header_text) {
        Ok(h) => h
        Err(_) => { return "bad binding header" }
    }
    if field(inner, "alg") != "HS256" {
        return "alg"
    }
    if field(inner, "kid") != want_kid {
        return "kid"
    }
    if field(inner, "url") != NEW_ACCT {
        return "url"
    }
    if inner.text("nonce").is_some() {
        return "nonce"
    }
    // The payload of the binding is the account key's JWK.
    let public = match jws.public_key(key()) {
        Ok(p) => p
        Err(_) => { return "key" }
    }
    let want_jwk = match jws.jwk(public) {
        Ok(j) => j
        Err(_) => { return "key" }
    }
    let got_jwk = base64url.decode(field(eab, "payload")).unwrap_or(Bytes.new()).to_text().unwrap_or("")
    if got_jwk != want_jwk {
        return "jwk"
    }
    let inner_payload = field(eab, "payload")
    let signing_input = "{inner_protected}.{inner_payload}"
    let tag = base64url.decode(field(eab, "signature")).unwrap_or(Bytes.new())
    if !hmac.verify_sha256(mac_key(), signing_input.to_bytes(), tag) {
        return "mac"
    }
    "ok"
}

struct Ca { kid: String }

impl acme.Transport for Ca {
    fn get(self, url: acme.Url) -> Result<http.Response, http.HttpError> {
        if url.text == DIR {
            return Ok(http.Response(status: 200, headers: [], body: dir_body().to_bytes()))
        }
        if url.text == NEW_NONCE {
            return Ok(http.Response(status: 204, headers: [http.Header(name: "replay-nonce", value: "n0")], body: Bytes.new()))
        }
        Err(http.HttpError.Malformed("no recorded GET"))
    }

    fn post(self, url: acme.Url, body: Bytes) -> Result<http.Response, http.HttpError> {
        if url.text != NEW_ACCT {
            return Err(http.HttpError.Malformed("no recorded POST"))
        }
        let verdict = check_binding(body, self.kid)
        if verdict == "ok" {
            return Ok(http.Response(status: 201, headers: [http.Header(name: "replay-nonce", value: "n1"), http.Header(name: "Location", value: ACCT)], body: "\{\"status\":\"valid\"\}".to_bytes()))
        }
        if verdict == "missing" {
            return Ok(problem("externalAccountRequired"))
        }
        if verdict == "mac" || verdict == "kid" {
            return Ok(problem("unauthorized"))
        }
        Ok(problem("malformed"))
    }
}

fn name(e: acme.AcmeError) -> String {
    match e {
        acme.AcmeError.Problem(p) => match p.kind {
            acme.ProblemKind.ExternalAccountRequired => "external account required"
            acme.ProblemKind.Malformed => "malformed problem"
            _ => "problem"
        }
        acme.AcmeError.Unauthorized(p) => "unauthorized {p.status}"
        acme.AcmeError.InvalidInput(_) => "input"
        acme.AcmeError.Key => "key"
        acme.AcmeError.Malformed(_) => "malformed"
        _ => "other"
    }
}

// What `new_account_with_eab` answered, as one line, for a client that
// gives `kid` and `mac` against a CA that holds "kid-1" and `mac_key()`.
fn make(kid: String, mac: Secret<Bytes>) -> String {
    let ca = Ca(kid: "kid-1")
    let directory = match acme.fetch_directory(ca, DIR, false) {
        Ok(d) => d
        Err(_) => { return "no directory" }
    }
    if !directory.external_account_required {
        return "directory does not ask for EAB"
    }
    var client = match acme.Client.new(directory, key(), false) {
        Ok(c) => c
        Err(e) => { return name(e) }
    }
    let eab = match acme.Eab.new(kid, mac) {
        Ok(b) => b
        Err(e) => { return name(e) }
    }
    match client.new_account_with_eab(ca, eab, ["mailto:a@example.com"], true) {
        Ok(a) => "account {a.url.text}"
        Err(e) => name(e)
    }
}

fn refused_sign(r: Result<String, jws.JwsError>) -> Bool {
    match r {
        Ok(_) => false
        Err(_) => true
    }
}

fn without_eab() -> String {
    let ca = Ca(kid: "kid-1")
    let directory = match acme.fetch_directory(ca, DIR, false) {
        Ok(d) => d
        Err(_) => { return "no directory" }
    }
    var client = match acme.Client.new(directory, key(), false) {
        Ok(c) => c
        Err(e) => { return name(e) }
    }
    match client.new_account(ca, ["mailto:a@example.com"], true, false) {
        Ok(a) => "account {a.url.text}"
        Err(e) => name(e)
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
fn a_transcript_with_eab_creates_an_account() {
    passes(
        r#"test "the CA verifies the binding and answers an account" {
    t.assert_eq(make("kid-1", mac_key()), "account https://ca.example/acct/1")
}

test "the binding is a JWS: HS256, the key id, the newAccount URL, no nonce, the account JWK" {
    let public = match jws.public_key(key()) {
        Ok(p) => p
        Err(_) => { return }
    }
    let account_jwk = jws.jwk(public).unwrap_or("")
    let text = match jws.sign_hs256(mac_key(), "kid-1", NEW_ACCT, account_jwk.to_bytes()) {
        Ok(s) => s
        Err(_) => { return }
    }
    let v = match json.parse(text) {
        Ok(v) => v
        Err(_) => { return }
    }
    let header = base64url.decode(field(v, "protected")).unwrap_or(Bytes.new()).to_text().unwrap_or("")
    t.assert_eq(header, "\{\"alg\":\"HS256\",\"kid\":\"kid-1\",\"url\":\"{NEW_ACCT}\"\}")
    t.assert_eq(base64url.decode(field(v, "payload")).unwrap_or(Bytes.new()).to_text().unwrap_or(""), account_jwk)
    let protected = field(v, "protected")
    let payload = field(v, "payload")
    let input = "{protected}.{payload}"
    t.assert(hmac.verify_sha256(mac_key(), input.to_bytes(), base64url.decode(field(v, "signature")).unwrap_or(Bytes.new())))
    t.assert(!hmac.verify_sha256(wrong_key(), input.to_bytes(), base64url.decode(field(v, "signature")).unwrap_or(Bytes.new())))
}

test "bad fields are refused before anything is signed" {
    t.assert(refused_sign(jws.sign_hs256(mac_key(), "", NEW_ACCT, Bytes.new())))
    t.assert(refused_sign(jws.sign_hs256(mac_key(), "k\"id", NEW_ACCT, Bytes.new())))
    t.assert(refused_sign(jws.sign_hs256(Secret.new(Bytes.new()), "kid-1", NEW_ACCT, Bytes.new())))
}
"#,
    );
}

#[test]
fn the_key_id_and_key_come_from_the_caller() {
    passes(
        r#"test "an empty key id or key is refused by the caller's input check" {
    t.assert_eq(make("", mac_key()), "input")
    t.assert_eq(make("kid 1", mac_key()), "input")
    t.assert_eq(make("kid-1", Secret.new(Bytes.new())), "input")
}

test "another key id is the CA's unauthorized" {
    t.assert_eq(make("kid-2", mac_key()), "unauthorized 403")
}
"#,
    );
}

#[test]
fn a_missing_or_wrong_eab_is_the_cas_typed_error() {
    passes(
        r#"test "no binding: externalAccountRequired" {
    t.assert_eq(without_eab(), "external account required")
}

test "a wrong key: unauthorized" {
    t.assert_eq(make("kid-1", wrong_key()), "unauthorized 403")
}

test "the plain account body has no binding member" {
    let body = acme.new_account_body(["mailto:a@example.com"], true, false).unwrap_or(Bytes.new())
    t.assert(!body.to_text().unwrap_or("").contains("externalAccountBinding"))
}
"#,
    );
}

#[test]
fn the_hmac_key_never_prints_interpolates_or_leaves() {
    passes(
        r#"test "the CA saw no request holding either key (a leak is malformed)" {
    t.assert_eq(make("kid-1", mac_key()), "account https://ca.example/acct/1")
}

test "errors say nothing of the key" {
    let key_hex = "7365637265742d6561622d6b65792d30"
    for e in [acme.AcmeError.Key, acme.AcmeError.InvalidInput("x"), acme.AcmeError.Failed] {
        t.assert(!e.to_string().contains(MAC))
        t.assert(!e.to_string().contains(key_hex))
    }
}
"#,
    );
    assert!(!refused(
        r#"fn main(io: Stdout) {
    let k = mac_key()
    let _ = io.println(k)
}
"#
    )
    .is_empty());
    assert!(!refused(
        r#"fn main(io: Stdout) {
    let k = mac_key()
    let _ = io.println("key: {k}")
}
"#
    )
    .is_empty());
    // A binding holds the key and has no `Display`.
    assert!(!refused(
        r#"fn leak(eab: acme.Eab) -> String {
    "{eab}"
}

fn main(io: Stdout) {
    let _ = io.println("x")
}
"#,
    )
    .is_empty());
}
