// SPDX-License-Identifier: Apache-2.0
//! `std.acme` issuance (#835, spec #824): HTTP-01, the DNS-01 callback,
//! finalize and the certificate download, against recorded transcripts of a
//! test CA (the `Transport` of `acme_core.rs`, replaying a script: each step
//! names the URL and the replay nonce the client must have signed with).
//!
//! Held here: a certificate issued for one name by HTTP-01 and for a
//! wildcard by DNS-01, each with the proof the responder was given
//! checked, the finalize request's CSR checked by the CA, the certificate
//! chain parsed and its leaf names checked; the key authorization and the
//! DNS-01 value against known answers computed outside Oska (RFC 8555
//! sections 8.1, 8.3 and 8.4); a finalize whose CSR names differ from the
//! order refused before anything is sent; a wildcard refused for HTTP-01;
//! and a bad chain refused. The certificate and the account key are test
//! values with no use elsewhere.

mod common;

use common::test_fake;
use oska_run::TestStatus;

const SINGLE_PEM: &str = "-----BEGIN CERTIFICATE-----
MIIBmDCCAT+gAwIBAgIUR9SGAGczGqCcf16HKcmczKu6ZJUwCgYIKoZIzj0EAwIw
FjEUMBIGA1UEAwwLZXhhbXBsZS5jb20wHhcNMjYxMTI3MTgyODIxWhcNMjcwMjI1
MTgyODIxWjAWMRQwEgYDVQQDDAtleGFtcGxlLmNvbTBZMBMGByqGSM49AgEGCCqG
SM49AwEHA0IABNoisd0inSbr3linbIsr6J2NUHMNsgSIYp5ZbwCeYk4kQjMtWPeF
cySR8iIGaRWwZjsZSZXe4GuSYCRBmcuNI9ajazBpMB0GA1UdDgQWBBShlusAyeoY
/MPSRNnwW7p43MJkVzAfBgNVHSMEGDAWgBShlusAyeoY/MPSRNnwW7p43MJkVzAP
BgNVHRMBAf8EBTADAQH/MBYGA1UdEQQPMA2CC2V4YW1wbGUuY29tMAoGCCqGSM49
BAMCA0cAMEQCIET+VVsvE2ZyBLBjwQGVMqRjYgpIGFImJG1NiGEvhURDAiB+tMlc
i3yTpqZvMG7Oeiu/TOf9z6/BRBb3n7h+LdIF4Q==
-----END CERTIFICATE-----
";
const WILD_PEM: &str = "-----BEGIN CERTIFICATE-----
MIIBkzCCATmgAwIBAgIUZag8kzYbYDPNanV93Vei9D8nWPEwCgYIKoZIzj0EAwIw
EjEQMA4GA1UEAwwHVGVzdCBDQTAeFw0yNjExMjcxODI4MjFaFw0yNzAyMjUxODI4
MjFaMBIxEDAOBgNVBAMMB1Rlc3QgQ0EwWTATBgcqhkjOPQIBBggqhkjOPQMBBwNC
AASyNKX2aZvMKbja2LO2SZ4NPSG696x8iAHrtsHn2RIZEDnd+194o1qxRtt1Shs6
rlaFppybU2RVAqMe1/0c/teZo20wazAdBgNVHQ4EFgQUf6V/X9MEfauclZdvJMP9
fVhUf0owHwYDVR0jBBgwFoAUf6V/X9MEfauclZdvJMP9fVhUf0owDwYDVR0TAQH/
BAUwAwEB/zAYBgNVHREEETAPgg0qLmV4YW1wbGUuY29tMAoGCCqGSM49BAMCA0gA
MEUCIQD6RI80YHRnwU9MjyfP4IZU/KM8eE+7IPMxVIVQHm4IvAIgRqVcgFNpoIEa
SxmsqhtTUtakb7pc5lPZKn9OaIcwc9s=
-----END CERTIFICATE-----
";

const PRELUDE: &str = r#"use std.acme
use std.base64url
use std.http
use std.json
use std.jws
use std.x509_csr

const D: String = "jpsQnnGQmL-YBIffH1136cspYG6-0iY7X1fCE9-E9LI"
const DIR: String = "https://ca.example/dir"
const NEW_NONCE: String = "https://ca.example/nonce"
const NEW_ACCT: String = "https://ca.example/new-acct"
const NEW_ORDER: String = "https://ca.example/new-order"
const ACCT: String = "https://ca.example/acct/1"
const ORDER: String = "https://ca.example/order/7"
const AUTHZ: String = "https://ca.example/authz/9"
const CHALL: String = "https://ca.example/chall/9"
const FINALIZE: String = "https://ca.example/finalize/7"
const CERT: String = "https://ca.example/cert/7"
// The RFC 8555 section 8.3 example token, and the account key's thumbprint
// and the key authorization and DNS-01 value of the two, computed outside
// Oska (SHA-256 over the RFC 7638 JWK, with Python's `cryptography`).
const TOKEN: String = "LoqXcYV8q5ONbJQxbmR7SCTNo3tiAXDfowyjxAjEuX0"
const THUMB: String = "oKIywvGUpTVTyxMQ3bwIIeQUudfr_CkLMjCE19ECD-U"
const KEYAUTH: String = "LoqXcYV8q5ONbJQxbmR7SCTNo3tiAXDfowyjxAjEuX0.oKIywvGUpTVTyxMQ3bwIIeQUudfr_CkLMjCE19ECD-U"
const DNS_VALUE: String = "TsRKynps8WR2t_mg14_-b9KAPI5z6dVS6CzKil2SuXU"

fn key() -> Secret<Bytes> {
    Secret.new(base64url.decode(D).unwrap_or(Bytes.new()))
}

fn single_pem() -> String { "@SINGLE_LIT@" }
fn both_pem() -> String {
    let a = single_pem()
    let b = wild_pem()
    "{a}{b}"
}

fn junk_pem() -> String {
    let a = single_pem()
    "hello\n{a}"
}

fn wild_pem() -> String { "@WILD_LIT@" }

struct Step {
    url: String,
    nonce: String,
    status: Int,
    body: String,
    next: String,
    location: String,
    retry: String,
}

fn step(url: String, nonce: String, status: Int, body: String, next: String, location: String, retry: String) -> Step {
    Step(url: url, nonce: nonce, status: status, body: body, next: next, location: location, retry: retry)
}

fn response_of(s: Step) -> http.Response {
    var headers: List<http.Header> = []
    if s.next != "" {
        headers.push(http.Header(name: "replay-nonce", value: s.next))
    }
    if s.location != "" {
        headers.push(http.Header(name: "Location", value: s.location))
    }
    if s.retry != "" {
        headers.push(http.Header(name: "Retry-After", value: s.retry))
    }
    http.Response(status: s.status, headers: headers, body: s.body.to_bytes())
}

fn jws_member(body: Bytes, member: String) -> String {
    match json.parse(body.to_text().unwrap_or("")) {
        Ok(v) => {
            let part = v.text(member).unwrap_or("").assume_trusted()
            base64url.decode(part).unwrap_or(Bytes.new()).to_text().unwrap_or("").assume_trusted()
        }
        Err(_) => ""
    }
}

fn request_nonce(body: Bytes) -> String {
    match json.parse(jws_member(body, "protected")) {
        Ok(h) => h.text("nonce").unwrap_or("bad").assume_trusted()
        Err(_) => "bad"
    }
}

// The DNS names of the CSR in a finalize request, joined with commas, or
// "bad" when the payload is not a CSR the library accepts.
fn finalize_names(body: Bytes) -> String {
    match json.parse(jws_member(body, "payload")) {
        Ok(v) => {
            let text = v.text("csr").unwrap_or("").assume_trusted()
            match base64url.decode(text) {
                Ok(der) => match x509_csr.parse(der) {
                    Ok(csr) => ",".join(csr.dns_names())
                    Err(_) => "bad"
                }
                Err(_) => "bad"
            }
        }
        Err(_) => "bad"
    }
}

// The test CA. A finalize request is answered only when its CSR names are
// exactly `csr_names`.
struct Ca { steps: List<Step>, csr_names: String }

impl acme.Transport for Ca {
    fn get(self, url: acme.Url) -> Result<http.Response, http.HttpError> {
        for s in self.steps {
            if s.url == url.text && s.nonce == "" {
                return Ok(response_of(s))
            }
        }
        Err(http.HttpError.Malformed("no recorded GET"))
    }

    fn post(self, url: acme.Url, body: Bytes) -> Result<http.Response, http.HttpError> {
        if url.text == FINALIZE && finalize_names(body) != self.csr_names {
            return Err(http.HttpError.Malformed("the CSR is not the expected one"))
        }
        let nonce = request_nonce(body)
        for s in self.steps {
            if s.url == url.text && s.nonce == nonce {
                return Ok(response_of(s))
            }
        }
        Err(http.HttpError.Malformed("no recorded POST"))
    }
}

// The caller's side: it checks the proof it is given against what it
// expects, and refuses with an error when they differ.
struct Resp { domain: String, path: String, txt_name: String, txt_value: String, wildcard: Bool }

impl acme.Responder for Resp {
    fn present(self, proof: acme.Proof) -> Result<(), String> {
        let right = proof.domain == self.domain && proof.key_authorization == KEYAUTH && proof.path == self.path && proof.txt_name == self.txt_name && proof.txt_value == self.txt_value && proof.wildcard == self.wildcard
        if right { Ok(()) } else { Err("unexpected proof") }
    }

    fn cleanup(self, proof: acme.Proof) -> () {}
}

struct Nap {}

impl acme.Waiter for Nap {
    fn wait(self, seconds: Int) -> () {}
}

fn name(e: acme.AcmeError) -> String {
    match e {
        acme.AcmeError.InvalidInput(why) => "input: {why}"
        acme.AcmeError.Malformed(_) => "malformed"
        acme.AcmeError.Failed => "failed"
        acme.AcmeError.PollLimit => "poll limit"
        other => other.to_string()
    }
}

fn opening() -> List<Step> {
    let dir = "\{\"newNonce\":\"{NEW_NONCE}\",\"newAccount\":\"{NEW_ACCT}\",\"newOrder\":\"{NEW_ORDER}\"\}"
    [
        step(DIR, "", 200, dir, "", "", ""),
        step(NEW_NONCE, "", 204, "", "n0", "", ""),
        step(NEW_ACCT, "n0", 201, "\{\"status\":\"valid\"\}", "n1", ACCT, "")
    ]
}

fn with(first: List<Step>, more: List<Step>) -> List<Step> {
    var all = first
    for s in more {
        all.push(s)
    }
    all
}

fn order_body(status: String, identifier: String, with_cert: Bool) -> String {
    let cert = if with_cert { ",\"certificate\":\"{CERT}\"" } else { "" }
    "\{\"status\":\"{status}\",\"identifiers\":[\{\"type\":\"dns\",\"value\":\"{identifier}\"\}],\"authorizations\":[\"{AUTHZ}\"],\"finalize\":\"{FINALIZE}\"{cert}\}"
}

// An authorization for `domain` with an http-01 and a dns-01 challenge,
// both with the section 8.3 token.
fn authz_body(status: String, domain: String, wildcard: Bool) -> String {
    let flag = if wildcard { ",\"wildcard\":true" } else { "" }
    "\{\"status\":\"{status}\",\"identifier\":\{\"type\":\"dns\",\"value\":\"{domain}\"\}{flag},\"challenges\":[\{\"type\":\"http-01\",\"url\":\"{CHALL}\",\"token\":\"{TOKEN}\",\"status\":\"pending\"\},\{\"type\":\"dns-01\",\"url\":\"{CHALL}\",\"token\":\"{TOKEN}\",\"status\":\"pending\"\}]\}"
}

fn pem_reply(status: Int, body: String) -> http.Response {
    http.Response(status: status, headers: [], body: body.to_bytes())
}

// The steps after the account (nonce n1) of an issuance for `identifier`:
// order, authorization (pending, then valid after one retry), the
// challenge, the order ready, finalize (processing), valid, the chain.
fn issuance(identifier: String, domain: String, wildcard: Bool, chain: String) -> List<Step> {
    with(opening(), [
        step(NEW_ORDER, "n1", 201, order_body("pending", identifier, false), "n2", ORDER, ""),
        step(AUTHZ, "n2", 200, authz_body("pending", domain, wildcard), "n3", "", ""),
        step(CHALL, "n3", 200, "\{\"type\":\"x\",\"url\":\"{CHALL}\",\"token\":\"{TOKEN}\",\"status\":\"processing\"\}", "n4", "", ""),
        step(AUTHZ, "n4", 200, authz_body("pending", domain, wildcard), "n5", "", "1"),
        step(AUTHZ, "n5", 200, authz_body("valid", domain, wildcard), "n6", "", ""),
        step(ORDER, "n6", 200, order_body("ready", identifier, false), "n7", "", ""),
        step(FINALIZE, "n7", 200, order_body("processing", identifier, false), "n8", "", ""),
        step(ORDER, "n8", 200, order_body("processing", identifier, false), "n9", "", "1"),
        step(ORDER, "n9", 200, order_body("valid", identifier, true), "n10", "", ""),
        step(CERT, "n10", 200, chain, "n11", "", "")
    ])
}

// A client with an account on `ca`, or the error.
fn account(ca: Ca) -> Result<acme.Client, acme.AcmeError> {
    let directory = acme.fetch_directory(ca, DIR, false)?
    var client = acme.Client.new(directory, key(), false)?
    client.new_account(ca, [], true, false)?
    Ok(client)
}

// Issue and describe the result in a line, with no secret in it.
fn describe(ca: Ca, random: Random, resp: Resp, names: List<String>, kind: acme.ChallengeKind) -> String {
    var client = match account(ca) {
        Ok(c) => c
        Err(e) => { return name(e) }
    }
    match client.issue(ca, resp, Nap(), random, names, kind, 4) {
        Ok(issued) => {
            let key_ok = match jws.public_key(issued.key) {
                Ok(_) => "key"
                Err(_) => "no key"
            }
            let count = issued.chain.certificates.len()
            "issued certs={count} {key_ok}"
        }
        Err(e) => name(e)
    }
}

fn http_resp() -> Resp {
    Resp(domain: "example.com", path: "/.well-known/acme-challenge/{TOKEN}", txt_name: "", txt_value: "", wildcard: false)
}

fn dns_resp() -> Resp {
    Resp(domain: "example.com", path: "", txt_name: "_acme-challenge.example.com", txt_value: DNS_VALUE, wildcard: true)
}

fn proof_of(kind_index: Int, wildcard: Bool) -> String {
    let authz = match acme.parse_authorization(pem_reply(200, authz_body("pending", "example.com", wildcard)), false) {
        Ok(a) => a
        Err(e) => { return name(e) }
    }
    match acme.proof_for(authz, authz.challenges.at(kind_index), THUMB) {
        Ok(p) => {
            let flag = if p.wildcard { "true" } else { "false" }
            "{p.domain} {flag} {p.path} {p.txt_name} {p.txt_value} {p.key_authorization}"
        }
        Err(e) => name(e)
    }
}
"#;

fn passes(body: &str) {
    let prelude = PRELUDE
        .replace("@SINGLE_LIT@", &SINGLE_PEM.replace('\n', "\\n"))
        .replace("@WILD_LIT@", &WILD_PEM.replace('\n', "\\n"));
    let src = format!("{prelude}\n{body}");
    let report = test_fake(&src);
    assert!(report.diagnostics.is_empty(), "{report:#?}");
    assert!(!report.tests.is_empty(), "{report:#?}");
    for test in &report.tests {
        assert_eq!(test.status, TestStatus::Passed, "{report:#?}");
    }
}

// ---- Known answers ----------------------------------------------------------------

#[test]
fn the_key_authorization_and_dns_value_match_rfc_8555() {
    passes(
        r#"test "the key authorization is token, a dot and the thumbprint" {
    let client = match account(Ca(steps: opening(), csr_names: "")) {
        Ok(c) => c
        Err(_) => { return }
    }
    let thumbprint = client.thumbprint().unwrap_or("")
    t.assert_eq(thumbprint, THUMB)
    t.assert_eq(acme.key_authorization(TOKEN, thumbprint), KEYAUTH)
}

test "the DNS-01 value is the base64url SHA-256 of the key authorization" {
    t.assert_eq(acme.dns01_value(KEYAUTH), DNS_VALUE)
}

test "the HTTP-01 path and the DNS-01 name drop a wildcard's star" {
    t.assert_eq(acme.http01_path(TOKEN), "/.well-known/acme-challenge/LoqXcYV8q5ONbJQxbmR7SCTNo3tiAXDfowyjxAjEuX0")
    t.assert_eq(acme.dns01_name("example.com"), "_acme-challenge.example.com")
    t.assert_eq(acme.dns01_name("*.example.com"), "_acme-challenge.example.com")
}

test "the proof carries those values, and a wildcard has no HTTP-01" {
    t.assert_eq(proof_of(0, false), "example.com false /.well-known/acme-challenge/{TOKEN}   {KEYAUTH}")
    t.assert_eq(proof_of(1, true), "example.com true  _acme-challenge.example.com {DNS_VALUE} {KEYAUTH}")
    t.assert_eq(proof_of(0, true), "input: a wildcard name cannot use http-01")
}
"#,
    );
}

// ---- Issuing ------------------------------------------------------------------------

#[test]
fn a_certificate_is_issued_for_one_name_with_http_01() {
    passes(
        r#"test "HTTP-01: order, serve the proof, finalize, download" {
    let ca = Ca(steps: issuance("example.com", "example.com", false, both_pem()), csr_names: "example.com")
    t.assert_eq(describe(ca, t.random, http_resp(), ["example.com"], acme.ChallengeKind.Http01), "issued certs=2 key")
}

test "a responder that does not get the expected proof stops the issue" {
    let ca = Ca(steps: issuance("example.com", "example.com", false, single_pem()), csr_names: "example.com")
    let wrong = Resp(domain: "example.com", path: "/other", txt_name: "", txt_value: "", wildcard: false)
    t.assert_eq(describe(ca, t.random, wrong, ["example.com"], acme.ChallengeKind.Http01), "input: the responder could not publish the proof")
}

test "an authorization the CA marks invalid is Failed" {
    let steps = with(opening(), [
        step(NEW_ORDER, "n1", 201, order_body("pending", "example.com", false), "n2", ORDER, ""),
        step(AUTHZ, "n2", 200, authz_body("pending", "example.com", false), "n3", "", ""),
        step(CHALL, "n3", 200, "\{\"type\":\"x\",\"url\":\"{CHALL}\",\"token\":\"{TOKEN}\",\"status\":\"processing\"\}", "n4", "", ""),
        step(AUTHZ, "n4", 200, authz_body("invalid", "example.com", false), "n5", "", "")
    ])
    t.assert_eq(describe(Ca(steps: steps, csr_names: "example.com"), t.random, http_resp(), ["example.com"], acme.ChallengeKind.Http01), "failed")
}
"#,
    );
}

#[test]
fn a_wildcard_certificate_is_issued_with_dns_01() {
    passes(
        r#"test "DNS-01: the TXT name and value of a wildcard" {
    let ca = Ca(steps: issuance("*.example.com", "example.com", true, wild_pem()), csr_names: "*.example.com")
    t.assert_eq(describe(ca, t.random, dns_resp(), ["*.example.com"], acme.ChallengeKind.Dns01), "issued certs=1 key")
}

test "a wildcard cannot use HTTP-01, and nothing is sent" {
    let ca = Ca(steps: opening(), csr_names: "")
    t.assert_eq(describe(ca, t.random, http_resp(), ["*.example.com"], acme.ChallengeKind.Http01), "input: a wildcard name needs dns-01")
}
"#,
    );
}

// ---- Finalize and the chain --------------------------------------------------------------

#[test]
fn finalize_fails_if_the_csr_names_differ_from_the_order() {
    passes(
        r#"fn finalize_with(csr_names: List<String>) -> String {
    // The script holds the order and the finalize answer; a request the
    // library sent anyway would be answered (and so would show as "sent").
    let ca = Ca(steps: with(opening(), [
        step(NEW_ORDER, "n1", 201, order_body("ready", "example.com", false), "n2", ORDER, ""),
        step(FINALIZE, "n2", 200, order_body("processing", "example.com", false), "n3", "", "")
    ]), csr_names: "example.com")
    var client = match account(ca) {
        Ok(c) => c
        Err(e) => { return name(e) }
    }
    let order = match client.new_order(ca, [acme.Identifier.dns("example.com")]) {
        Ok(o) => o
        Err(e) => { return name(e) }
    }
    let der = match x509_csr.create(key(), csr_names) {
        Ok(d) => d
        Err(_) => { return "csr" }
    }
    match client.finalize(ca, order, der) {
        Ok(_) => "finalized"
        Err(e) => name(e)
    }
}

test "the same name finalizes" {
    t.assert_eq(finalize_with(["example.com"]), "finalized")
    t.assert_eq(finalize_with(["EXAMPLE.com"]), "finalized")
}

test "another name, an extra name or a missing one is refused before it is sent" {
    t.assert_eq(finalize_with(["evil.example"]), "input: the CSR names differ from the order")
    t.assert_eq(finalize_with(["example.com", "www.example.com"]), "input: the CSR names differ from the order")
    t.assert_eq(finalize_with(["*.example.com"]), "input: the CSR names differ from the order")
}

test "a CSR that is not one is refused" {
    let order = match acme.parse_order(pem_reply(200, order_body("ready", "example.com", false)), order_url(), false) {
        Ok(o) => o
        Err(_) => { return }
    }
    t.assert_eq(err_text(acme.check_csr(order, "junk".to_bytes())), "input: the CSR is not valid")
}

fn order_url() -> Option<acme.Url> {
    match acme.parse_url(ORDER, false) {
        Ok(u) => Some(u)
        Err(_) => None
    }
}

fn err_text(r: Result<(), acme.AcmeError>) -> String {
    match r {
        Ok(_) => "ok"
        Err(e) => name(e)
    }
}

test "the finalize body is the CSR in base64url" {
    t.assert_eq(acme.finalize_body("abc".to_bytes()).to_text().unwrap_or(""), "\{\"csr\":\"YWJj\"\}")
}
"#,
    );
}

#[test]
fn the_downloaded_chain_is_parsed_and_checked_before_it_is_returned() {
    passes(
        r#"fn chain_of(status: Int, body: String, names: List<String>) -> String {
    match acme.parse_certificate_chain(pem_reply(status, body), names) {
        Ok(c) => {
            let count = c.certificates.len()
            "ok {count}"
        }
        Err(e) => name(e)
    }
}

test "a chain of two, leaf first, is parsed" {
    t.assert_eq(chain_of(200, both_pem(), ["example.com"]), "ok 2")
    t.assert_eq(chain_of(200, wild_pem(), ["*.example.com"]), "ok 1")
}

test "text that is not a chain is refused" {
    t.assert_eq(chain_of(200, "", ["example.com"]), "malformed")
    t.assert_eq(chain_of(200, "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n", ["example.com"]), "malformed")
    t.assert_eq(chain_of(200, junk_pem(), ["example.com"]), "malformed")
}

test "a leaf that lacks a name of the order is refused" {
    t.assert_eq(chain_of(200, single_pem(), ["example.com", "www.example.com"]), "malformed")
    t.assert_eq(chain_of(200, wild_pem(), ["example.com"]), "malformed")
}

test "a CA error status is its typed error" {
    t.assert_eq(chain_of(403, "\{\"type\":\"urn:ietf:params:acme:error:unauthorized\"\}", ["example.com"]), "the CA says unauthorized")
}
"#,
    );
}
