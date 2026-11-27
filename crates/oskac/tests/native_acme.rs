// SPDX-License-Identifier: Apache-2.0
//! `std.acme` (#834) as a whole program: the pure functions of the ACME
//! core on recorded CA responses (a directory, an account, an order, an
//! authorization and a problem document), run on the interpreter and
//! natively with the same output. This file runs on every platform the
//! native backend supports (Linux, macOS and Windows), so a result that
//! differs by platform fails the differential suite there.

#![cfg(any(
    all(
        any(target_os = "linux", target_os = "macos"),
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(windows, target_arch = "x86_64")
))]

mod common;
mod differential;

use differential::{assert_same, Case};

const PROGRAM: &str = r#"use std.acme
use std.http

fn reply(status: Int, body: String) -> http.Response {
    http.Response(status: status, headers: [http.Header(name: "Location", value: "https://ca.example/acct/1")], body: body.to_bytes())
}

fn main(io: Stdout) -> Result<(), IoError> {
    let directory = acme.parse_directory(reply(200, "\{\"newNonce\":\"https://ca.example/n\",\"newAccount\":\"https://ca.example/a\",\"newOrder\":\"https://ca.example/o\"\}"), false)
    match directory {
        Ok(d) => io.println("directory {d.new_order.host} {d.new_order.port} {d.new_order.path}")?
        Err(e) => io.println(e.to_string())?
    }
    match acme.parse_account(reply(201, "\{\"status\":\"valid\"\}"), false) {
        Ok(a) => io.println("account {a.url.text}")?
        Err(e) => io.println(e.to_string())?
    }
    match acme.parse_authorization(reply(200, "\{\"status\":\"pending\",\"identifier\":\{\"type\":\"dns\",\"value\":\"example.com\"\},\"challenges\":[\{\"type\":\"http-01\",\"url\":\"https://ca.example/c\",\"token\":\"abc-_9\",\"status\":\"pending\"\}]\}"), false) {
        Ok(a) => {
            let first = a.challenges.at(0)
            io.println("authorization {a.identifier.value} {first.token}")?
        }
        Err(e) => io.println(e.to_string())?
    }
    io.println(acme.error_from_response(reply(429, "\{\"type\":\"urn:ietf:params:acme:error:rateLimited\"\}")).to_string())?
    match acme.parse_url("http://ca.example/dir", false) {
        Ok(_) => io.println("accepted")
        Err(e) => io.println(e.to_string())
    }
}
"#;

#[test]
fn acme_core_agrees_on_both_backends() {
    let outcome = assert_same(&Case::file("acme_core", PROGRAM));
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
    let lines: Vec<&str> = outcome.stdout.lines().collect();
    assert_eq!(
        lines,
        [
            "directory ca.example 443 /o",
            "account https://ca.example/acct/1",
            "authorization example.com abc-_9",
            "the CA says rate limited",
            "the URL is not https"
        ],
        "{outcome:#?}"
    );
}

const ISSUE_PROGRAM: &str = r#"use std.acme
use std.http
use std.x509_csr

fn main(io: Stdout) -> Result<(), IoError> {
    let thumbprint = "oKIywvGUpTVTyxMQ3bwIIeQUudfr_CkLMjCE19ECD-U"
    let token = "LoqXcYV8q5ONbJQxbmR7SCTNo3tiAXDfowyjxAjEuX0"
    let proof = acme.key_authorization(token, thumbprint)
    io.println(proof)?
    io.println(acme.dns01_value(proof))?
    io.println(acme.dns01_name("*.example.com"))?
    io.println(acme.http01_path(token))?
    io.println(acme.finalize_body("abc".to_bytes()).to_text().unwrap_or(""))?
    let authz_reply = http.Response(status: 200, headers: [], body: "\{\"status\":\"pending\",\"wildcard\":true,\"identifier\":\{\"type\":\"dns\",\"value\":\"example.com\"\},\"challenges\":[\{\"type\":\"http-01\",\"url\":\"https://ca.example/c\",\"token\":\"LoqXcYV8q5ONbJQxbmR7SCTNo3tiAXDfowyjxAjEuX0\",\"status\":\"pending\"\}]\}".to_bytes())
    match acme.parse_authorization(authz_reply, false) {
        Ok(authorization) => {
            match acme.proof_for(authorization, authorization.challenges.at(0), thumbprint) {
                Ok(_) => io.println("proof")?
                Err(e) => io.println(e.to_string())?
            }
        }
        Err(e) => io.println(e.to_string())?
    }
    let body = "@PEM@"
    let reply = http.Response(status: 200, headers: [], body: body.to_bytes())
    match acme.parse_certificate_chain(reply, ["example.com"]) {
        Ok(c) => {
            let count = c.certificates.len()
            io.println("chain {count}")?
        }
        Err(e) => io.println(e.to_string())?
    }
    match acme.parse_certificate_chain(reply, ["other.example"]) {
        Ok(_) => io.println("chain")?
        Err(e) => io.println(e.to_string())?
    }
    let order_reply = http.Response(status: 200, headers: [http.Header(name: "Location", value: "https://ca.example/o")], body: "\{\"status\":\"ready\",\"identifiers\":[\{\"type\":\"dns\",\"value\":\"example.com\"\}],\"authorizations\":[],\"finalize\":\"https://ca.example/f\"\}".to_bytes())
    match acme.parse_order(order_reply, None, false) {
        Ok(order) => {
            match acme.check_csr(order, "junk".to_bytes()) {
                Ok(_) => io.println("csr")?
                Err(e) => io.println(e.to_string())?
            }
        }
        Err(e) => io.println(e.to_string())?
    }
    Ok(())
}
"#;

#[test]
fn acme_issuance_agrees_on_both_backends() {
    let program = ISSUE_PROGRAM.replace("@PEM@", &PEM.replace('\n', "\\n"));
    let outcome = assert_same(&Case::file("acme_issue", &program));
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
    let lines: Vec<&str> = outcome.stdout.lines().collect();
    assert_eq!(
        lines,
        [
            "LoqXcYV8q5ONbJQxbmR7SCTNo3tiAXDfowyjxAjEuX0.oKIywvGUpTVTyxMQ3bwIIeQUudfr_CkLMjCE19ECD-U",
            "TsRKynps8WR2t_mg14_-b9KAPI5z6dVS6CzKil2SuXU",
            "_acme-challenge.example.com",
            "/.well-known/acme-challenge/LoqXcYV8q5ONbJQxbmR7SCTNo3tiAXDfowyjxAjEuX0",
            "{\"csr\":\"YWJj\"}",
            "invalid input: a wildcard name cannot use http-01",
            "chain 1",
            "malformed response: the certificate does not carry every name of the order",
            "invalid input: the CSR is not valid"
        ],
        "{outcome:#?}"
    );
}

const PEM: &str = "-----BEGIN CERTIFICATE-----
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
