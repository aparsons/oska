// SPDX-License-Identifier: Apache-2.0
//! `std.x509`'s authority key identifier, serial and ARI id (#832) as a
//! whole program on the interpreter and natively, with the same output.
//! This file runs on every platform the native backend supports (Linux,
//! macOS and Windows). The cases are in
//! `crates/oska-run/tests/x509_ari_id.rs`.

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

const PROGRAM: &str = r#"use std.x509

fn report(io: Stdout, text: String) -> Result<(), IoError> {
    let raw: Untrusted<Bytes> = Bytes.from_hex(text).unwrap_or(Bytes.new())
    match x509.Certificate.parse(raw) {
        Ok(cert) => {
            let serial = cert.serial().to_hex()
            io.println("serial:{serial}")?
            match cert.ari_id() {
                Ok(id) => io.println("ari:{id}")
                Err(e) => {
                    let m = e.to_string()
                    io.println("err:{m}")
                }
            }
        }
        Err(e) => {
            let m = e.to_string()
            io.println("parse:{m}")
        }
    }
}

fn main(io: Stdout) -> Result<(), IoError> {
    report(io, "308201763082011ba003020102020900a1b2c3d4e5f60718300a06082a8648ce3d04030230123110300e06035504030c0754657374204341301e170d3236313132373133333833315a170d3237303232353133333833315a30143112301006035504030c096c6561662e746573743059301306072a8648ce3d020106082a8648ce3d030107034200041bea25d960a2c6e777f520251378747f2b1a74dcd72eac954d98539b4164657b20caccdd7e1a797a33087790792033f671b4584d05b6c9c52d6a2095f59d4d00a3583056301f0603551d2304183016801479a3570cd157fc90725897d01e126e4303ad58f530140603551d11040d300b82096c6561662e74657374301d0603551d0e04160414f9c45928475e93a0164d1faed61ccfb8a2135e27300a06082a8648ce3d0403020349003046022100b33351b6c48c6704ab7a5bfa2d9415183967392f146fafa406fe82e50cc8cabd022100a56ff58ec609e83a3211928c29f65ff21f0eed24d8cf4fb543b3420b4deedbd2")?
    report(io, "308201583081fea003020102021401730ffcc812acff69132130ffebc998eb67f9d8300a06082a8648ce3d04030230123110300e06035504030c0754657374204341301e170d3236313132373133333833345a170d3336313132343133333833345a30123110300e06035504030c07546573742043413059301306072a8648ce3d020106082a8648ce3d030107034200041a046cbb158c505fcb9b321bd0e7a697b73c3a3eb4362f6ba295ae53178e8b5e14e816a55721961f0db05d60ef9fdfc3e3f87446613e4997eafdc900edaeee84a3323030301d0603551d0e0416041479a3570cd157fc90725897d01e126e4303ad58f5300f0603551d130101ff040530030101ff300a06082a8648ce3d0403020349003046022100d48b7986b051b2720fa7980d19d8b69f7786ba91ff93b720a336ac34b2d733a9022100b36ce2468cb51f7a3f99b5c2fbe1a65ebf180ffb05262b87aa3936719d04eab6")
}
"#;

#[test]
fn the_ari_id_and_the_missing_key_identifier_agree_on_both_backends() {
    let outcome = assert_same(&Case::file("x509_ari_id", PROGRAM));
    assert_eq!(
        outcome.stdout,
        "serial:00a1b2c3d4e5f60718\nari:eaNXDNFX_JByWJfQHhJuQwOtWPU.AKGyw9Tl9gcY\n\
         serial:01730ffcc812acff69132130ffebc998eb67f9d8\n\
         err:the certificate has no authority key identifier\n",
        "{outcome:#?}"
    );
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
}
