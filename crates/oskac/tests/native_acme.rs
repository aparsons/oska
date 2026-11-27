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
