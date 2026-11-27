// SPDX-License-Identifier: Apache-2.0
//! `std.x509_csr` (#833) as a whole program: a request written for two DNS
//! names and a request written by an independent tool (`cryptography` 50,
//! RFC 6979 signing) and read back, run on the interpreter and natively
//! with the same output. This file runs on every platform the native
//! backend supports (Linux, macOS and Windows), so a result that differs
//! by platform fails the differential suite there.

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

const TWO: &str = "3081f930819f02010030003059301306072a8648ce3d020106082a8648ce3d0301070342000460fed4ba255a9d31c961eb74c6356d68c049b8923b61fa6ce669622e60f29fb67903fe1008b8bc99a41ae9e95628bc64f2f1b20c2d7e9f5177a3c294d4462299a03d303b06092a864886f70d01090e312e302c302a0603551d110101ff0420301e820b6578616d706c652e636f6d820f7777772e6578616d706c652e636f6d300a06082a8648ce3d0403020349003046022100d4520617b5ecffc33d842b1d499250a1055997621c1d4fedb4cedda053c851d5022100adb8625febf48d5c7ea00a5c74fbe43145abd87cb3a63f332478210ab4715dbf";

fn program() -> String {
    r#"use std.x509_csr

fn hex(text: String) -> Bytes {
    Bytes.from_hex(text).unwrap_or(Bytes.new())
}

fn main(io: Stdout) -> Result<(), IoError> {
    let key = Secret.new(hex("c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721"))
    match x509_csr.create(key, ["example.com", "www.example.com"]) {
        Ok(der) => {
            io.println(if der.to_hex() == "TWO_HEX" { "same as the independent tool" } else { "different" })?
            match x509_csr.parse(der) {
                Ok(csr) => {
                    let count = csr.dns_names().len()
                    io.println("parsed, {count} names")?
                }
                Err(e) => io.println(e.to_string())?
            }
        }
        Err(e) => io.println(e.to_string())?
    }
    let bad = Secret.new(hex("00"))
    match x509_csr.create(bad, ["example.com"]) {
        Ok(_) => io.println("signed")
        Err(e) => io.println(e.to_string())
    }
}
"#
    .replace("TWO_HEX", TWO)
}

#[test]
fn a_request_is_the_same_on_both_backends() {
    let outcome = assert_same(&Case::file("x509_csr_all", &program()));
    assert_eq!(
        outcome.stdout,
        "same as the independent tool\nparsed, 2 names\nthe key is not a valid P-256 key\n",
        "{outcome:#?}"
    );
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
}
