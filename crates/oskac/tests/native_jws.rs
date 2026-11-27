// SPDX-License-Identifier: Apache-2.0
//! `std.jws` (#830) as a whole program: an ES256 JWS, a JWK and the RFC 7638
//! thumbprint, run on the interpreter and natively with the same output.
//! This file runs on every platform the native backend supports (Linux,
//! macOS and Windows), so a result that differs by platform fails the
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

const PROGRAM: &str = r#"use std.base64url
use std.jws

fn main(io: Stdout) -> Result<(), IoError> {
    let d = base64url.decode("jpsQnnGQmL-YBIffH1136cspYG6-0iY7X1fCE9-E9LI").unwrap_or(Bytes.new())
    let key = Secret.new(d)
    match jws.public_key(key) {
        Ok(public) => {
            io.println(jws.jwk(public).unwrap_or("error"))?
            io.println(jws.thumbprint(public).unwrap_or("error"))?
        }
        Err(_) => io.println("bad key")?
    }
    io.println(jws.sign_kid(key, "https://ca.example/acct/1", "n0nce", "https://ca.example/order/9", Bytes.new()).unwrap_or("error"))?
    io.println(jws.sign_jwk(key, "n0nce", "https://ca.example/new-acct", "\{\}".to_bytes()).unwrap_or("error"))?
    io.println(jws.thumbprint_of_jwk("\{\"kty\":\"RSA\",\"e\":\"AQAB\",\"n\":\"AQAB\"\}").unwrap_or("error"))
}
"#;

#[test]
fn jws_agrees_on_both_backends() {
    let outcome = assert_same(&Case::file("jws_all", PROGRAM));
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
    let lines: Vec<&str> = outcome.stdout.lines().collect();
    assert_eq!(lines.len(), 5, "{outcome:#?}");
    assert_eq!(
        lines[0],
        r#"{"crv":"P-256","kty":"EC","x":"f83OJ3D2xF1Bg8vub9tLe1gHMzV76e8Tus9uPHvRVEU","y":"x_FEzRu9m36HLN_tue659LNpXW6pCyStikYjKIWI5a0"}"#
    );
    assert!(lines[2].starts_with(r#"{"protected":""#), "{outcome:#?}");
    assert!(lines[3].contains(r#""payload":"e30""#), "{outcome:#?}");
}
