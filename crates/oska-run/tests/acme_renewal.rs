// SPDX-License-Identifier: Apache-2.0
//! `std.acme` renewal (#837, spec #824): ARI scheduling (RFC 9773) against
//! recorded CA answers, with the time and the random number passed in, so
//! a test needs no clock and no entropy.
//!
//! Held here: a recorded renewal window is read and the chosen time is
//! inside it and varies with the random source; the backstop (one third of
//! the lifetime left, one half under ten days) is right for 90-day, 45-day
//! and 6-day certificates; `Retry-After` is honoured; a failed renewal is
//! not tried again within a day (the clock is a number the test moves); the
//! ARI id of a certificate is a known answer computed outside Oska; and the
//! new order carries `replaces`, which a CA that does not see it refuses.

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

const PRELUDE: &str = r#"use std.acme
use std.base64url
use std.http
use std.json
use std.jws
use std.x509

const D: String = "jpsQnnGQmL-YBIffH1136cspYG6-0iY7X1fCE9-E9LI"
const DIR: String = "https://ca.example/dir"
const NEW_NONCE: String = "https://ca.example/nonce"
const NEW_ACCT: String = "https://ca.example/new-acct"
const NEW_ORDER: String = "https://ca.example/new-order"
const RENEWAL: String = "https://ca.example/renewal-info"
const ACCT: String = "https://ca.example/acct/1"
const ORDER: String = "https://ca.example/order/7"
const AUTHZ: String = "https://ca.example/authz/9"
const FINALIZE: String = "https://ca.example/finalize/7"
// The ARI id of the test certificate, computed outside Oska (Python's
// `cryptography`): the base64url of the Authority Key Identifier's key
// identifier, a dot, and of the serial's content bytes.
const ARI_ID: String = "oZbrAMnqGPzD0kTZ8Fu6eNzCZFc.R9SGAGczGqCcf16HKcmczKu6ZJU"
const ARI_URL: String = "https://ca.example/renewal-info/oZbrAMnqGPzD0kTZ8Fu6eNzCZFc.R9SGAGczGqCcf16HKcmczKu6ZJU"
// 2026-12-20T10:00:00Z and 2026-12-21T10:00:00Z.
const START: Int = 1797760800
const END: Int = 1797847200
const DAY: Int = 86400

fn key() -> Secret<Bytes> {
    Secret.new(base64url.decode(D).unwrap_or(Bytes.new()))
}

fn pem() -> String { "@SINGLE_LIT@" }

fn reply(status: Int, body: String, retry: String, nonce: String) -> http.Response {
    var headers: List<http.Header> = []
    if nonce != "" {
        headers.push(http.Header(name: "replay-nonce", value: nonce))
    }
    if retry != "" {
        headers.push(http.Header(name: "Retry-After", value: retry))
    }
    http.Response(status: status, headers: headers, body: body.to_bytes())
}

fn window_body(start: String, end: String) -> String {
    "\{\"suggestedWindow\":\{\"start\":\"{start}\",\"end\":\"{end}\"\},\"explanationURL\":\"https://ca.example/why\"\}"
}

fn payload_member(body: Bytes, member: String) -> String {
    match json.parse(body.to_text().unwrap_or("")) {
        Ok(v) => {
            let part = v.text("payload").unwrap_or("").assume_trusted()
            let inner = base64url.decode(part).unwrap_or(Bytes.new()).to_text().unwrap_or("")
            match json.parse(inner) {
                Ok(p) => p.text(member).unwrap_or("").assume_trusted()
                Err(_) => ""
            }
        }
        Err(_) => ""
    }
}

// The test CA. It has renewal information for the certificate when
// `window` is not empty (an answer of 404 when it is), and it accepts an
// order only when its `replaces` is `want_replaces`.
struct Ca { window: String, retry: String, want_replaces: String }

impl acme.Transport for Ca {
    fn get(self, url: acme.Url) -> Result<http.Response, http.HttpError> {
        if url.text == DIR {
            let dir = "\{\"newNonce\":\"{NEW_NONCE}\",\"newAccount\":\"{NEW_ACCT}\",\"newOrder\":\"{NEW_ORDER}\",\"renewalInfo\":\"{RENEWAL}\"\}"
            return Ok(reply(200, dir, "", ""))
        }
        if url.text == NEW_NONCE {
            return Ok(reply(204, "", "", "n0"))
        }
        if url.text == ARI_URL {
            if self.window == "" {
                return Ok(reply(404, "\{\"type\":\"urn:ietf:params:acme:error:malformed\"\}", "", ""))
            }
            return Ok(reply(200, self.window, self.retry, ""))
        }
        Err(http.HttpError.Malformed("no recorded GET"))
    }

    fn post(self, url: acme.Url, body: Bytes) -> Result<http.Response, http.HttpError> {
        if url.text == NEW_ACCT {
            return Ok(http.Response(status: 201, headers: [http.Header(name: "Location", value: ACCT), http.Header(name: "Replay-Nonce", value: "n1")], body: "\{\"status\":\"valid\"\}".to_bytes()))
        }
        if url.text == NEW_ORDER {
            if payload_member(body, "replaces") != self.want_replaces {
                return Ok(reply(400, "\{\"type\":\"urn:ietf:params:acme:error:malformed\"\}", "", "n2"))
            }
            let order = "\{\"status\":\"pending\",\"identifiers\":[\{\"type\":\"dns\",\"value\":\"example.com\"\}],\"authorizations\":[\"{AUTHZ}\"],\"finalize\":\"{FINALIZE}\"\}"
            return Ok(http.Response(status: 201, headers: [http.Header(name: "Location", value: ORDER), http.Header(name: "Replay-Nonce", value: "n2")], body: order.to_bytes()))
        }
        Err(http.HttpError.Malformed("no recorded POST"))
    }
}

// The test certificate, or an empty list (a test then fails on `at(0)`).
fn certificate() -> List<x509.Certificate> {
    match acme.parse_certificate_chain(reply(200, pem(), "", ""), ["example.com"]) {
        Ok(c) => c.certificates
        Err(_) => []
    }
}

fn account(ca: Ca) -> Result<acme.Client, acme.AcmeError> {
    let directory = acme.fetch_directory(ca, DIR, false)?
    var client = acme.Client.new(directory, key(), false)?
    client.new_account(ca, [], true, false)?
    Ok(client)
}

// The parsed answer as a list of one, or an empty list (a test then fails
// on `at(0)`).
fn info_list(status: Int, body: String, retry: String) -> List<acme.RenewalInfo> {
    match acme.parse_renewal_info(reply(status, body, retry, ""), false) {
        Ok(i) => [i]
        Err(_) => []
    }
}

fn info_error(status: Int, body: String) -> String {
    match acme.parse_renewal_info(reply(status, body, "", ""), false) {
        Ok(_) => "ok"
        Err(acme.AcmeError.Malformed(_)) => "malformed"
        Err(other) => other.to_string()
    }
}
"#;

fn passes(body: &str) {
    let prelude = PRELUDE.replace("@SINGLE_LIT@", &SINGLE_PEM.replace('\n', "\\n"));
    let src = format!("{prelude}\n{body}");
    let report = test_fake(&src);
    assert!(report.diagnostics.is_empty(), "{report:#?}");
    assert!(!report.tests.is_empty(), "{report:#?}");
    for test in &report.tests {
        assert_eq!(test.status, TestStatus::Passed, "{report:#?}");
    }
}

// ---- The window --------------------------------------------------------------------------

#[test]
fn a_recorded_window_is_read_and_the_chosen_time_is_inside_it() {
    passes(
        r#"test "the window, the explanation and Retry-After are read" {
    let info = info_list(200, window_body("2026-12-20T10:00:00Z", "2026-12-21T10:00:00Z"), "3600").at(0)
    t.assert_eq(info.window.start, START)
    t.assert_eq(info.window.end, END)
    t.assert_eq(info.retry_after, Some(3600))
    t.assert(info.explanation_url.is_some())
}

test "an offset and a fraction of a second are read" {
    let info = info_list(200, window_body("2026-12-20T12:00:00+02:00", "2026-12-21T10:00:00.123Z"), "").at(0)
    t.assert_eq(info.window.start, START)
    t.assert_eq(info.window.end, END)
    t.assert_eq(info.retry_after, None)
}

test "timestamps that are not RFC 3339 are refused" {
    t.assert_eq(acme.parse_rfc3339("2028-02-29T23:59:59Z"), Some(1835481599))
    t.assert_eq(acme.parse_rfc3339("1970-01-01T00:00:00Z"), Some(0))
    t.assert_eq(acme.parse_rfc3339("2027-02-29T00:00:00Z"), None)
    t.assert_eq(acme.parse_rfc3339("2026-13-01T00:00:00Z"), None)
    t.assert_eq(acme.parse_rfc3339("2026-12-20 10:00:00Z"), None)
    t.assert_eq(acme.parse_rfc3339("2026-12-20T10:00:00"), None)
    t.assert_eq(acme.parse_rfc3339("2026-12-20T10:00:00Zjunk"), None)
    t.assert_eq(acme.parse_rfc3339("2026-12-20T25:00:00Z"), None)
    t.assert_eq(acme.parse_rfc3339("2026-12-20T10:00:00.Z"), None)
}

test "a missing window, a reversed window and an error status are refused" {
    t.assert_eq(info_error(200, "\{\}"), "malformed")
    t.assert_eq(info_error(200, window_body("2026-12-21T10:00:00Z", "2026-12-20T10:00:00Z")), "malformed")
    t.assert_eq(info_error(200, window_body("tomorrow", "2026-12-20T10:00:00Z")), "malformed")
    t.assert_eq(info_error(404, "\{\"type\":\"urn:ietf:params:acme:error:malformed\"\}"), "the CA answered a problem (status 404)")
}

test "the chosen time is inside the window and varies with the random source" {
    let window = acme.RenewalWindow(start: START, end: END)
    var seen: List<Int> = []
    var drawn = 0
    while drawn < 50 {
        let at = acme.choose_renewal_time(window, t.random.next_int())
        t.assert(at >= START && at <= END)
        var known = false
        for s in seen {
            if s == at {
                known = true
            }
        }
        if !known {
            seen.push(at)
        }
        drawn = drawn + 1
    }
    t.assert(seen.len() > 40)
    t.assert_eq(acme.choose_renewal_time(window, 0), START)
    t.assert_eq(acme.choose_renewal_time(window, END - START), END)
    t.assert_eq(acme.choose_renewal_time(window, END - START + 1), START)
    t.assert_eq(acme.choose_renewal_time(window, 0 - 5), START)
    t.assert_eq(acme.choose_renewal_time(acme.RenewalWindow(start: START, end: START), 12345), START)
}
"#,
    );
}

// ---- The backstop ---------------------------------------------------------------------------

#[test]
fn the_backstop_dates_are_right_for_90_45_and_6_day_certificates() {
    passes(
        r#"test "a 90-day certificate is renewed with 30 days left" {
    t.assert_eq(acme.backstop_time(1000000, 1000000 + 90 * DAY), 1000000 + 60 * DAY)
}

test "a 45-day certificate is renewed with 15 days left" {
    t.assert_eq(acme.backstop_time(1000000, 1000000 + 45 * DAY), 1000000 + 30 * DAY)
}

test "a 6-day certificate is renewed with half its lifetime, 3 days, left" {
    t.assert_eq(acme.backstop_time(1000000, 1000000 + 6 * DAY), 1000000 + 3 * DAY)
}

test "ten days is the edge: under it half, from it a third" {
    t.assert_eq(acme.backstop_time(0, 10 * DAY - 1), (10 * DAY - 1) - (10 * DAY - 1) / 2)
    t.assert_eq(acme.backstop_time(0, 10 * DAY), 10 * DAY - 10 * DAY / 3)
}

test "the test certificate is 90 days, and with no window it gets the backstop" {
    let cert = certificate().at(0)
    t.assert_eq(cert.not_after() - cert.not_before(), 90 * DAY)
    let plan = acme.plan_renewal(cert, None, 7)
    t.assert_eq(plan.renew_at, cert.not_after() - 30 * DAY)
    let from_window = acme.plan_renewal(cert, Some(info_list(200, window_body("2026-12-20T10:00:00Z", "2026-12-21T10:00:00Z"), "").at(0)), 7)
    t.assert_eq(from_window.renew_at, START + 7)
}
"#,
    );
}

// ---- Retry-After, failures and the clock ----------------------------------------------------------

#[test]
fn retry_after_is_honoured_and_a_failed_renewal_waits_a_day() {
    passes(
        r#"test "the next check is Retry-After later, within bounds" {
    t.assert_eq(acme.next_check(Some(3600), 1000), 4600)
    t.assert_eq(acme.next_check(Some(0), 1000), 1060)
    t.assert_eq(acme.next_check(Some(10 * DAY), 1000), 1000 + DAY)
    t.assert_eq(acme.next_check(None, 1000), 1000 + 21600)
}

test "a failed renewal is not tried again within a day" {
    let plan = acme.RenewalPlan(renew_at: 5000, source: acme.PlanSource.Backstop)
    t.assert(!acme.may_attempt(plan, None, DAY, 4999))
    t.assert(acme.may_attempt(plan, None, DAY, 5000))
    // It failed at 6000.
    t.assert(!acme.may_attempt(plan, Some(6000), DAY, 6001))
    t.assert(!acme.may_attempt(plan, Some(6000), DAY, 6000 + DAY - 1))
    t.assert(acme.may_attempt(plan, Some(6000), DAY, 6000 + DAY))
    // It fails again at 6000 + DAY: another day.
    t.assert(!acme.may_attempt(plan, Some(6000 + DAY), DAY, 6000 + DAY + 100))
    t.assert(acme.may_attempt(plan, Some(6000 + DAY), DAY, 6000 + 2 * DAY))
}

test "the wait after a failure is a day, or the CA's longer Retry-After" {
    let plain = acme.Problem(kind: acme.ProblemKind.RateLimited, uri: "u", detail: "d", status: 429, retry_after: None)
    let short = acme.Problem(kind: acme.ProblemKind.RateLimited, uri: "u", detail: "d", status: 429, retry_after: Some(60))
    let long = acme.Problem(kind: acme.ProblemKind.RateLimited, uri: "u", detail: "d", status: 429, retry_after: Some(3 * DAY))
    t.assert_eq(acme.failure_wait(acme.AcmeError.RateLimited(plain)), DAY)
    t.assert_eq(acme.failure_wait(acme.AcmeError.RateLimited(short)), DAY)
    t.assert_eq(acme.failure_wait(acme.AcmeError.RateLimited(long)), 3 * DAY)
    t.assert_eq(acme.failure_wait(acme.AcmeError.Failed), DAY)
}
"#,
    );
}

// ---- The ARI id, the ask and replaces ------------------------------------------------------------

#[test]
fn the_ari_url_is_known_and_the_client_asks_for_the_window() {
    passes(
        r#"test "the id of the certificate matches the value computed outside Oska" {
    let cert = certificate().at(0)
    t.assert_eq(cert.ari_id().unwrap_or(""), ARI_ID)
}

test "the client asks the CA and plans inside the answer" {
    let ca = Ca(window: window_body("2026-12-20T10:00:00Z", "2026-12-21T10:00:00Z"), retry: "7200", want_replaces: "")
    let client = match account(ca) {
        Ok(c) => c
        Err(_) => { return }
    }
    let planned = client.plan(ca, certificate().at(0), t.random)
    t.assert(planned.plan.renew_at >= START && planned.plan.renew_at <= END)
    t.assert_eq(planned.retry_after, Some(7200))
    let from_window = match planned.plan.source {
        acme.PlanSource.Window => true
        acme.PlanSource.Backstop => false
    }
    t.assert(from_window)
}

test "a CA with no answer for the certificate gets the backstop" {
    let ca = Ca(window: "", retry: "", want_replaces: "")
    let client = match account(ca) {
        Ok(c) => c
        Err(_) => { return }
    }
    let cert = certificate().at(0)
    let planned = client.plan(ca, cert, t.random)
    t.assert_eq(planned.plan.renew_at, cert.not_after() - 30 * DAY)
    t.assert_eq(planned.retry_after, None)
}

test "a CA with no renewalInfo in its directory is never asked" {
    let ca = Ca(window: "", retry: "", want_replaces: "")
    let directory = match acme.fetch_directory(ca, DIR, false) {
        Ok(d) => d
        Err(_) => { return }
    }
    let bare = acme.Directory(new_nonce: directory.new_nonce, new_account: directory.new_account, new_order: directory.new_order, revoke_cert: None, renewal_info: None, terms_of_service: None, external_account_required: false)
    let url = acme.ari_url(bare, certificate().at(0), false)
    t.assert(match url {
        Ok(None) => true
        _ => false
    })
}
"#,
    );
}

#[test]
fn the_new_order_carries_replaces() {
    passes(
        r#"test "the order body names the old certificate" {
    let body = acme.new_order_body_replacing([acme.Identifier.dns("example.com")], ARI_ID)
    let text = body.unwrap_or(Bytes.new()).to_text().unwrap_or("")
    t.assert_eq(text, "\{\"identifiers\":[\{\"type\":\"dns\",\"value\":\"example.com\"\}],\"replaces\":\"{ARI_ID}\"\}")
}

test "an id that is not two base64url parts is refused" {
    let ids = ["nodot", "a.b.c", "a b.c", ".c", "a."]
    for id in ids {
        t.assert(match acme.new_order_body_replacing([acme.Identifier.dns("example.com")], id) {
            Err(acme.AcmeError.InvalidInput(_)) => true
            _ => false
        })
    }
}

test "the order the client sends has replaces, and a plain order does not" {
    let ca = Ca(window: "", retry: "", want_replaces: ARI_ID)
    var client = match account(ca) {
        Ok(c) => c
        Err(_) => { return }
    }
    let ids = [acme.Identifier.dns("example.com")]
    t.assert(match client.new_order_replacing(ca, ids, certificate().at(0)) {
        Ok(_) => true
        Err(_) => false
    })
    t.assert(match client.new_order(ca, ids) {
        Err(acme.AcmeError.Problem(_)) => true
        _ => false
    })
}
"#,
    );
}
