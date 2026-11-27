// SPDX-License-Identifier: Apache-2.0
//! `std.acme` External Account Binding (#836) as a whole program: the
//! account body with `externalAccountBinding`, and the HS256 JWS of
//! `std.jws`, run on the interpreter and natively with the same output. The
//! HMAC key is a test value. This file runs on every platform the native
//! backend supports, so a result that differs by platform fails the
//! differential suite there.

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
use std.base64url
use std.hmac
use std.json
use std.jws

fn main(io: Stdout) -> Result<(), IoError> {
    let mac = Secret.new(base64url.decode("c2VjcmV0LWVhYi1rZXktMDEyMzQ1Njc4OWFiY2RlZg").unwrap_or(Bytes.new()))
    let text = match jws.sign_hs256(mac, "kid-1", "https://ca.example/new-acct", "payload".to_bytes()) {
        Ok(t) => t
        Err(e) => { return io.println(e.to_string()) }
    }
    io.println(text)?
    match jws.sign_hs256(mac, "", "https://ca.example/new-acct", Bytes.new()) {
        Ok(_) => io.println("accepted")?
        Err(e) => io.println(e.to_string())?
    }
    match acme.new_account_body_with_eab(["mailto:a@example.com"], true, text) {
        Ok(b) => io.println(b.to_text().unwrap_or(""))?
        Err(e) => io.println(e.to_string())?
    }
    match acme.Eab.new("", mac) {
        Ok(_) => io.println("accepted")
        Err(e) => io.println(e.to_string())
    }
}
"#;

#[test]
fn acme_eab_agrees_on_both_backends() {
    let outcome = assert_same(&Case::file("acme_eab", PROGRAM));
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
    let lines: Vec<&str> = outcome.stdout.lines().collect();
    assert_eq!(lines.len(), 4, "{outcome:#?}");
    assert!(
        lines[0].starts_with("{\"protected\":\"eyJhbGciOiJIUzI1NiIs"),
        "{outcome:#?}"
    );
    assert_eq!(
        lines[1],
        "a header field is empty or has a character that needs escaping"
    );
    assert!(
        lines[2].contains("\"externalAccountBinding\":{\"protected\":"),
        "{outcome:#?}"
    );
    assert_eq!(
        lines[3],
        "invalid input: an EAB key id or key is empty or has a character that is not allowed"
    );
}
