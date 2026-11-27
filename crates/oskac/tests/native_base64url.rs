// SPDX-License-Identifier: Apache-2.0
//! `std.base64url` (#829) as a whole program: encode, strict decode and
//! the raw ES256 signature, run on the interpreter and natively, with the
//! same output. This file runs on every platform the native backend
//! supports (Linux, macOS and Windows), so a result that differs by
//! platform fails the differential suite there.

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

fn verdict(r: Result<Bytes, base64url.Base64UrlError>) -> String {
    match r {
        Ok(b) => {
            let hex = b.to_hex()
            "ok:{hex}"
        }
        Err(e) => {
            let text = e.to_string()
            "err:{text}"
        }
    }
}

fn main(io: Stdout) -> Result<(), IoError> {
    io.println(base64url.encode("foobar".to_bytes()))?
    io.println(base64url.encode("fooba".to_bytes()))?
    io.println(base64url.encode(Bytes.from_hex("fbff").unwrap_or(Bytes.new())))?
    io.println(verdict(base64url.decode("Zm9vYmE")))?
    io.println(verdict(base64url.decode("Zm9vYg==")))?
    io.println(verdict(base64url.decode("+/8")))?
    io.println(verdict(base64url.decode("Zm 9v")))?
    io.println(verdict(base64url.decode("Zm9vY")))?
    io.println(verdict(base64url.decode("Zh")))?
    let r = Bytes.from_hex("0000ab").unwrap_or(Bytes.new())
    let s = Bytes.from_hex("00ff").unwrap_or(Bytes.new())
    io.println(verdict(base64url.es256_raw(r, s)))?
    let der = Bytes.from_hex("300702020080020105").unwrap_or(Bytes.new())
    io.println(verdict(base64url.es256_raw_from_der(der)))
}
"#;

#[test]
fn encoding_decoding_and_raw_signatures_agree_on_both_backends() {
    let outcome = assert_same(&Case::file("base64url_all", PROGRAM));
    let expected = format!(
        "Zm9vYmFy\nZm9vYmE\n-_8\nok:666f6f6261\n\
         err:base64url text has padding\n\
         err:base64url text uses the standard alphabet\n\
         err:base64url text has a character outside the alphabet\n\
         err:base64url text has a length no bytes encode to\n\
         err:base64url text has unused bits that are not zero\n\
         ok:{}ab{}ff\n\
         ok:{}80{}05\n",
        "00".repeat(31),
        "00".repeat(31),
        "00".repeat(31),
        "00".repeat(31)
    );
    assert_eq!(outcome.stdout, expected, "{outcome:#?}");
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
}
