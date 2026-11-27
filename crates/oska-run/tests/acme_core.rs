// SPDX-License-Identifier: Apache-2.0
//! `std.acme` core (#834, spec #824) against recorded transcripts of a test
//! CA. The CA is a `Transport` written in Oska that replays a script: each
//! step names the URL and the replay nonce the client must have signed
//! with, and what the CA answers. A client that uses a stale or missing
//! nonce, or posts to another URL, gets no step, so the transcript also
//! proves the order of the requests. Programs run on `FakeHost` on the
//! interpreter and natively wherever the native backend runs
//! (`common/native.rs`).
//!
//! Held here: a full account and order reaching `ready`; `badNonce`
//! retried once and a second one an error; `rateLimited`, `unauthorized`
//! and a malformed or oversized response each a typed error; a non-HTTPS
//! directory refused (loopback allowed for tests); and the account key
//! never printed, interpolated or sent.

mod common;

use common::{run_fake, test_fake};
use oska_run::TestStatus;

const PRELUDE: &str = r#"use std.acme
use std.base64url
use std.http
use std.json
use std.tls

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

fn key() -> Secret<Bytes> {
    Secret.new(base64url.decode(D).unwrap_or(Bytes.new()))
}

// One recorded exchange: the CA answers a request to `url` signed with
// `nonce` (the empty nonce is a GET) with `status`, `body`, and the headers
// `next` (a Replay-Nonce) and `location`, `retry` when not empty.
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

// The nonce a JWS was signed with, or "bad" when the request is not a JWS
// of the shape or carries the account key.
fn request_nonce(body: Bytes) -> String {
    let text = body.to_text().unwrap_or("")
    if text.contains(D) {
        return "leak"
    }
    match json.parse(text) {
        Ok(v) => {
            let protected = v.text("protected").unwrap_or("").assume_trusted()
            let header = base64url.decode(protected).unwrap_or(Bytes.new()).to_text().unwrap_or("")
            match json.parse(header) {
                Ok(h) => {
                    if h.text("alg").unwrap_or("").assume_trusted() == "ES256" {
                        h.text("nonce").unwrap_or("").assume_trusted()
                    } else {
                        "bad"
                    }
                }
                Err(_) => "bad"
            }
        }
        Err(_) => "bad"
    }
}

struct Ca { steps: List<Step> }

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
        let nonce = request_nonce(body)
        for s in self.steps {
            if s.url == url.text && s.nonce == nonce {
                return Ok(response_of(s))
            }
        }
        Err(http.HttpError.Malformed("no recorded POST"))
    }
}

fn dir_body() -> String {
    "\{\"newNonce\":\"{NEW_NONCE}\",\"newAccount\":\"{NEW_ACCT}\",\"newOrder\":\"{NEW_ORDER}\",\"revokeCert\":\"https://ca.example/revoke\",\"meta\":\{\"termsOfService\":\"https://ca.example/tos\",\"externalAccountRequired\":false\}\}"
}

fn acct_body() -> String {
    "\{\"status\":\"valid\",\"contact\":[\"mailto:a@example.com\"]\}"
}

fn order_body(status: String) -> String {
    "\{\"status\":\"{status}\",\"identifiers\":[\{\"type\":\"dns\",\"value\":\"example.com\"\}],\"authorizations\":[\"{AUTHZ}\"],\"finalize\":\"{FINALIZE}\"\}"
}

fn authz_body(status: String, chall_status: String) -> String {
    "\{\"status\":\"{status}\",\"identifier\":\{\"type\":\"dns\",\"value\":\"example.com\"\},\"challenges\":[\{\"type\":\"dns-01\",\"url\":\"https://ca.example/other\",\"token\":\"tok-dns\",\"status\":\"pending\"\},\{\"type\":\"http-01\",\"url\":\"{CHALL}\",\"token\":\"evaGxfADs6pSRb2LAv9IZf17Dt3juxGJ-PCt92wr-oA\",\"status\":\"{chall_status}\"\}]\}"
}

fn problem(name: String) -> String {
    "\{\"type\":\"urn:ietf:params:acme:error:{name}\",\"detail\":\"the CA says so\"\}"
}

fn problem_text(p: acme.Problem) -> String {
    "{p.status}"
}

// The names of the errors, for assertions. No CA text.
fn name(e: acme.AcmeError) -> String {
    match e {
        acme.AcmeError.InsecureUrl => "insecure"
        acme.AcmeError.InvalidUrl => "invalid url"
        acme.AcmeError.ForeignHost => "foreign"
        acme.AcmeError.Transport(_) => "transport"
        acme.AcmeError.Malformed(_) => "malformed"
        acme.AcmeError.TooLarge => "too large"
        acme.AcmeError.BadNonce(p) => "bad nonce {p.status}"
        acme.AcmeError.RateLimited(p) => "rate limited {p.status}"
        acme.AcmeError.Unauthorized(p) => "unauthorized {p.status}"
        acme.AcmeError.Problem(p) => "problem {p.status}"
        acme.AcmeError.Key => "key"
        acme.AcmeError.InvalidInput(_) => "input"
        acme.AcmeError.Failed => "failed"
        acme.AcmeError.PollLimit => "poll limit"
        acme.AcmeError.NoAccount => "no account"
    }
}

fn start(ca: Ca) -> Result<acme.Client, acme.AcmeError> {
    let directory = acme.fetch_directory(ca, DIR, false)?
    acme.Client.new(directory, key(), false)
}

// The result of `new_order` after a new account, as a string.
fn order_result(steps: List<Step>) -> String {
    let ca = Ca(steps: steps)
    var client = match start(ca) {
        Ok(c) => c
        Err(e) => { return name(e) }
    }
    match client.new_account(ca, ["mailto:a@example.com"], true, false) {
        Ok(_) => {}
        Err(e) => { return name(e) }
    }
    match client.new_order(ca, [acme.Identifier.dns("example.com")]) {
        Ok(o) => "order ok"
        Err(e) => name(e)
    }
}

// The first steps of every script: directory, nonce n0, account.
fn opening() -> List<Step> {
    [
        step(DIR, "", 200, dir_body(), "", "", ""),
        step(NEW_NONCE, "", 204, "", "n0", "", ""),
        step(NEW_ACCT, "n0", 201, acct_body(), "n1", ACCT, "")
    ]
}

fn with(first: List<Step>, more: List<Step>) -> List<Step> {
    var all = first
    for s in more {
        all.push(s)
    }
    all
}
fn err_of<T>(r: Result<T, acme.AcmeError>) -> String {
    match r {
        Ok(_) => "ok"
        Err(e) => name(e)
    }
}

fn url_of(text: String) -> Option<acme.Url> {
    match acme.parse_url(text, false) {
        Ok(u) => Some(u)
        Err(_) => None
    }
}

fn reply(status: Int, body: String) -> http.Response {
    http.Response(status: status, headers: [], body: body.to_bytes())
}

// The whole flow of a new account and order to `ready`, as one line.
fn flow(steps: List<Step>) -> String {
    let ca = Ca(steps: steps)
    let directory = match acme.fetch_directory(ca, DIR, false) {
        Ok(d) => d
        Err(e) => { return name(e) }
    }
    var client = match acme.Client.new(directory, key(), false) {
        Ok(c) => c
        Err(e) => { return name(e) }
    }
    let account = match client.new_account(ca, ["mailto:a@example.com"], true, false) {
        Ok(a) => a
        Err(e) => { return name(e) }
    }
    let order = match client.new_order(ca, [acme.Identifier.dns("example.com")]) {
        Ok(o) => o
        Err(e) => { return name(e) }
    }
    let authz_url = order.authorizations.at(0)
    let first = match client.fetch_authorization(ca, authz_url) {
        Ok(a) => a.value
        Err(e) => { return name(e) }
    }
    let challenge = first.challenges.at(1)
    let thumbprint = match client.thumbprint() {
        Ok(x) => x
        Err(e) => { return name(e) }
    }
    let proof = acme.key_authorization(challenge.token, thumbprint)
    if !proof.starts_with("evaGxfADs6pSRb2LAv9IZf17Dt3juxGJ-PCt92wr-oA.") {
        return "bad key authorization"
    }
    match client.respond_to_challenge(ca, challenge.url) {
        Ok(_) => {}
        Err(e) => { return name(e) }
    }
    var polls = 0
    var waited = 0
    var done = false
    while !done {
        let got = match client.fetch_authorization(ca, authz_url) {
            Ok(a) => a
            Err(e) => { return name(e) }
        }
        match acme.poll_authorization(got.value, got.retry_after, polls, 5) {
            Ok(acme.Poll.Done) => { done = true }
            Ok(acme.Poll.Wait(s)) => { waited = waited + s }
            Err(e) => { return name(e) }
        }
        polls = polls + 1
    }
    var order_polls = 0
    var ready = false
    while !ready {
        let got = match client.fetch_order(ca, order.url) {
            Ok(o) => o
            Err(e) => { return name(e) }
        }
        match acme.poll_order(got.value, got.retry_after, order_polls, 5) {
            Ok(acme.Poll.Done) => { ready = true }
            Ok(acme.Poll.Wait(_)) => {}
            Err(e) => { return name(e) }
        }
        order_polls = order_polls + 1
    }
    let count = first.challenges.len()
    "ready account={account.url.text} order={order.url.text} finalize={order.finalize.text} authz_polls={polls} waited={waited} order_polls={order_polls} challenges={count}"
}

fn poll_of(status: String, retry: Option<Int>, made: Int) -> String {
    let order = match acme.parse_order(reply(200, order_body(status)), url_of(ORDER), false) {
        Ok(o) => o
        Err(e) => { return name(e) }
    }
    match acme.poll_order(order, retry, made, 3) {
        Ok(acme.Poll.Done) => "done"
        Ok(acme.Poll.Wait(s)) => "wait {s}"
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

// ---- The full transcript ---------------------------------------------------------

#[test]
fn a_full_account_and_order_reaches_ready() {
    passes(
        r#"test "directory, nonce, account, order, authorization, challenge, polling" {
    let steps = with(opening(), [
        step(NEW_ORDER, "n1", 201, order_body("pending"), "n2", ORDER, ""),
        step(AUTHZ, "n2", 200, authz_body("pending", "pending"), "n3", "", ""),
        step(CHALL, "n3", 200, "\{\"type\":\"http-01\",\"url\":\"{CHALL}\",\"token\":\"evaGxfADs6pSRb2LAv9IZf17Dt3juxGJ-PCt92wr-oA\",\"status\":\"processing\"\}", "n4", "", ""),
        step(AUTHZ, "n4", 200, authz_body("pending", "processing"), "n5", "", "3"),
        step(AUTHZ, "n5", 200, authz_body("valid", "valid"), "n6", "", ""),
        step(ORDER, "n6", 200, order_body("pending"), "n7", "", ""),
        step(ORDER, "n7", 200, order_body("ready"), "n8", "", "")
    ])
    t.assert_eq(flow(steps), "ready account=https://ca.example/acct/1 order=https://ca.example/order/7 finalize=https://ca.example/finalize/7 authz_polls=2 waited=3 order_polls=2 challenges=2")
}

test "the directory is read" {
    let d = acme.parse_directory(reply(200, dir_body()), false)
    t.assert_eq(err_of(d), "ok")
    t.assert_eq(err_of(acme.fetch_directory(Ca(steps: opening()), DIR, false)), "ok")
}

test "a request signed with a stale nonce has no recorded answer" {
    // The order step wants n1; the account step answers n1 only if the
    // client signed the account request with n0.
    let steps = [
        step(DIR, "", 200, dir_body(), "", "", ""),
        step(NEW_NONCE, "", 204, "", "n0", "", ""),
        step(NEW_ACCT, "n-other", 201, acct_body(), "n1", ACCT, "")
    ]
    t.assert_eq(flow(steps), "malformed")
}

test "an order needs an account" {
    let ca = Ca(steps: opening())
    var client = match start(ca) {
        Ok(c) => c
        Err(_) => { return }
    }
    t.assert_eq(err_of(client.new_order(ca, [acme.Identifier.dns("example.com")])), "no account")
}
"#,
    );
}

// ---- badNonce -------------------------------------------------------------------

#[test]
fn bad_nonce_is_retried_once_with_the_fresh_nonce() {
    passes(
        r#"test "one badNonce, then success on the nonce it sent" {
    let steps = with(opening(), [
        step(NEW_ORDER, "n1", 400, problem("badNonce"), "n2", "", ""),
        step(NEW_ORDER, "n2", 201, order_body("pending"), "n3", ORDER, "")
    ])
    t.assert_eq(order_result(steps), "order ok")
}

test "a badNonce with no fresh nonce asks newNonce" {
    // The script's newNonce answers n0 each time, so the retry is signed
    // with n0: a nonce the client got from newNonce, not one it kept.
    let steps = with(opening(), [
        step(NEW_ORDER, "n1", 400, problem("badNonce"), "", "", ""),
        step(NEW_ORDER, "n0", 201, order_body("pending"), "n10", ORDER, "")
    ])
    t.assert_eq(order_result(steps), "order ok")
}

test "a second badNonce is an error" {
    let steps = with(opening(), [
        step(NEW_ORDER, "n1", 400, problem("badNonce"), "n2", "", ""),
        step(NEW_ORDER, "n2", 400, problem("badNonce"), "n3", "", ""),
        step(NEW_ORDER, "n3", 201, order_body("pending"), "n4", ORDER, "")
    ])
    t.assert_eq(order_result(steps), "bad nonce 400")
}
"#,
    );
}

// ---- Typed errors ------------------------------------------------------------------

#[test]
fn problems_and_bad_responses_are_typed_errors() {
    passes(
        r#"fn order_with(status: Int, body: String) -> String {
    order_result(with(opening(), [step(NEW_ORDER, "n1", status, body, "n2", ORDER, "")]))
}

test "rateLimited keeps Retry-After and the CA's text untrusted" {
    let p = match acme.parse_problem(http.Response(status: 429, headers: [http.Header(name: "retry-after", value: "120")], body: problem("rateLimited").to_bytes())) {
        Some(p) => p
        None => { return }
    }
    t.assert_eq(p.retry_after.unwrap_or(0), 120)
    t.assert(p.detail.to_bytes() == "the CA says so".to_bytes())
    t.assert_eq(p.status, 429)
    // Display carries no CA text.
    let shown = acme.AcmeError.RateLimited(p).to_string()
    t.assert(!shown.contains("the CA says so"))
}

test "rateLimited and unauthorized by problem type or by status" {
    t.assert_eq(order_with(429, problem("rateLimited")), "rate limited 429")
    t.assert_eq(order_with(429, "slow down"), "rate limited 429")
    t.assert_eq(order_with(403, problem("unauthorized")), "unauthorized 403")
    t.assert_eq(order_with(401, ""), "unauthorized 401")
    t.assert_eq(order_with(400, problem("rejectedIdentifier")), "problem 400")
    t.assert_eq(order_with(500, "<html>oops</html>"), "malformed")
}

test "a malformed success body is malformed" {
    t.assert_eq(order_with(201, "not json"), "malformed")
    t.assert_eq(order_with(201, "[]"), "malformed")
    t.assert_eq(order_with(201, "\{\"status\":\"pending\"\}"), "malformed")
    t.assert_eq(order_with(201, order_body("weird")), "malformed")
    t.assert_eq(order_with(201, "\{\"status\":\"pending\",\"identifiers\":[],\"authorizations\":[\"http://ca.example/a\"],\"finalize\":\"{FINALIZE}\"\}"), "insecure")
    t.assert_eq(order_with(201, "\{\"status\":\"pending\",\"identifiers\":[],\"authorizations\":[],\"finalize\":\"https://u:p@ca.example/f\"\}"), "invalid url")
}

test "an oversized response is TooLarge" {
    let pad = "x".repeat(acme.MAX_RESPONSE)
    let big = "\{\"status\":\"pending\",\"pad\":\"{pad}\"\}"
    t.assert_eq(order_with(201, big), "too large")
    t.assert_eq(order_with(400, big), "too large")
}

test "the transport's own failures are typed" {
    t.assert_eq(name(acme.error_from_response(reply(200, ""))), "malformed")
    t.assert_eq(err_of(acme.fetch_directory(Ca(steps: []), DIR, false)), "malformed")
}

test "a directory without a member, or with a bad URL, is refused" {
    let no_order = "\{\"newNonce\":\"{NEW_NONCE}\",\"newAccount\":\"{NEW_ACCT}\"\}"
    t.assert_eq(err_of(acme.parse_directory(reply(200, no_order), false)), "malformed")
    let plain = "\{\"newNonce\":\"http://ca.example/n\",\"newAccount\":\"{NEW_ACCT}\",\"newOrder\":\"{NEW_ORDER}\"\}"
    t.assert_eq(err_of(acme.parse_directory(reply(200, plain), false)), "insecure")
    t.assert_eq(err_of(acme.parse_directory(reply(200, plain), true)), "insecure")
    let loopback = "\{\"newNonce\":\"http://127.0.0.1:1/n\",\"newAccount\":\"http://127.0.0.1:1/a\",\"newOrder\":\"http://127.0.0.1:1/o\"\}"
    t.assert_eq(err_of(acme.parse_directory(reply(200, loopback), true)), "ok")
    t.assert_eq(err_of(acme.parse_directory(reply(200, loopback), false)), "insecure")
    t.assert_eq(err_of(acme.parse_directory(reply(503, problem("serverInternal")), false)), "problem 503")
}

test "the HTTPS transport is confined to its host and port" {
    let host = t.net.restrict_to("ca.example", 443)
    let transport = acme.HttpsTransport.new(host, tls.TlsConfig.default(), t.random, t.clock)
    t.assert_eq(err_of(acme.fetch_directory(transport, "https://evil.example/dir", false)), "foreign")
    t.assert_eq(err_of(acme.fetch_directory(transport, "https://ca.example:8443/dir", false)), "foreign")
    t.assert_eq(err_of(acme.fetch_directory(transport, "http://127.0.0.1/dir", true)), "foreign")
}

test "polling decisions" {
    t.assert_eq(poll_of("ready", None, 0), "done")
    t.assert_eq(poll_of("valid", None, 0), "done")
    t.assert_eq(poll_of("pending", Some(500), 0), "wait 60")
    t.assert_eq(poll_of("pending", Some(0), 0), "wait 1")
    t.assert_eq(poll_of("processing", None, 2), "wait 2")
    t.assert_eq(poll_of("pending", None, 3), "poll limit")
    t.assert_eq(poll_of("invalid", None, 0), "failed")
}
"#,
    );
}

// ---- URLs ---------------------------------------------------------------------------

#[test]
fn a_non_https_directory_is_refused_except_loopback_for_tests() {
    passes(
        r#"fn refusal(url: String, loopback: Bool) -> String {
    match acme.parse_url(url, loopback) {
        Ok(_) => "ok"
        Err(e) => name(e)
    }
}

test "https is accepted, http is not" {
    t.assert_eq(refusal("https://ca.example/dir", false), "ok")
    t.assert_eq(refusal("https://ca.example:8443/dir?x=1", false), "ok")
    t.assert_eq(refusal("https://[2001:db8::1]:8443/dir", false), "ok")
    t.assert_eq(refusal("http://ca.example/dir", false), "insecure")
    t.assert_eq(refusal("http://ca.example/dir", true), "insecure")
    t.assert_eq(refusal("ftp://ca.example/dir", false), "invalid url")
}

test "loopback http only when the caller allows it" {
    t.assert_eq(refusal("http://127.0.0.1:14000/dir", false), "insecure")
    t.assert_eq(refusal("http://127.0.0.1:14000/dir", true), "ok")
    t.assert_eq(refusal("http://localhost:14000/dir", true), "ok")
    t.assert_eq(refusal("http://[::1]:14000/dir", true), "ok")
    t.assert_eq(refusal("http://127.0.0.1.evil.example/dir", true), "insecure")
    t.assert_eq(refusal("http://1270.0.0.1/dir", true), "insecure")
}

fn parts(url: String, loopback: Bool) -> String {
    match acme.parse_url(url, loopback) {
        Ok(u) => "{u.host} {u.port} {u.path}"
        Err(e) => name(e)
    }
}

test "the parts of a URL" {
    t.assert_eq(parts("https://CA.Example:8443/a/b?c=d", false), "ca.example 8443 /a/b?c=d")
    t.assert_eq(parts("https://ca.example", false), "ca.example 443 /")
    t.assert_eq(parts("http://localhost", true), "localhost 80 /")
    t.assert_eq(parts("https://[2001:db8::1]:8443/dir", false), "[2001:db8::1] 8443 /dir")
    t.assert_eq(parts("https://[::1]/", false), "[::1] 443 /")
}

test "malformed URLs" {
    for bad in ["", "https://", "https:///x", "https://u@ca.example/", "https://ca.example:0/", "https://ca.example:65536/", "https://ca.example:80a/", "https://ca.example/a b", "https://ca.example/a\"b", "https://ca.example/a#frag", "https://ca.example/\\x", "https://ca.example?x", "https://-ca.example/", "https://ca_example/", "https://[::1/", "https://ca.example/é"] {
        t.assert_eq(refusal(bad, true), "invalid url")
    }
    let long = "x".repeat(2100)
    t.assert_eq(refusal("https://ca.example/{long}", false), "invalid url")
}

test "the directory URL is refused before anything is fetched" {
    let ca = Ca(steps: opening())
    t.assert_eq(err_of(acme.fetch_directory(ca, "http://ca.example/dir", false)), "insecure")
}
"#,
    );
}

// ---- Secrets ------------------------------------------------------------------------

#[test]
fn the_keys_never_print_interpolate_or_leave() {
    passes(
        r#"test "no request carries the account key" {
    // `Ca` answers a request whose body holds the key's text as "leak" and
    // finds no step for it, so a leaking client could not finish the order.
    let steps = with(opening(), [
        step(NEW_ORDER, "n1", 201, order_body("pending"), "n2", ORDER, "")
    ])
    t.assert_eq(order_result(steps), "order ok")
}

test "errors say nothing of the key" {
    let key_hex = "8e9b109e7190989f8804 87df1f5d77ac".replace(" ", "")
    for e in [acme.AcmeError.Key, acme.AcmeError.NoAccount, acme.AcmeError.Failed, acme.AcmeError.Malformed("x")] {
        t.assert(!e.to_string().contains(key_hex))
        t.assert(!e.to_string().contains(D))
    }
}

test "a key that is not a P-256 key is refused" {
    let directory = match acme.parse_directory(reply(200, dir_body()), false) {
        Ok(d) => d
        Err(_) => { return }
    }
    t.assert_eq(err_of(acme.Client.new(directory, Secret.new(Bytes.zeroed(32)), false)), "key")
    t.assert_eq(err_of(acme.Client.new(directory, Secret.new(Bytes.new()), false)), "key")
    t.assert_eq(err_of(acme.Client.new(directory, key(), false)), "ok")
}
"#,
    );
    // The checker refuses to print, interpolate or compare a key, and a
    // client (which holds one) has no `Display`.
    assert!(!refused(
        r#"fn main(io: Stdout) {
    let k = key()
    let _ = io.println(k)
}
"#
    )
    .is_empty());
    assert!(!refused(
        r#"fn main(io: Stdout) {
    let k = key()
    let _ = io.println("key: {k}")
}
"#
    )
    .is_empty());
    let client_shown = refused(
        r#"fn leak(client: acme.Client) -> String {
    "{client}"
}

fn main(io: Stdout) {
    let _ = io.println("x")
}
"#,
    );
    assert!(!client_shown.is_empty());
}
