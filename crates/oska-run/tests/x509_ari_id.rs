// SPDX-License-Identifier: Apache-2.0
//! `std.x509` Authority Key Identifier, serial and the RFC 9773 renewal
//! information id (#832, spec #824). Every program runs through the
//! `oska-run` API on a `FakeHost`, and on the native backend wherever it
//! runs (`common/native.rs`); `crates/oskac/tests/native_x509.rs` runs the
//! same program on every supported platform.
//!
//! The corpus is a leaf made by OpenSSL (serial with the high bit set, so
//! its DER content has a leading zero byte), a self-signed certificate
//! with no extensions, and certificates built here with the exact key
//! identifier and serial of the example in RFC 9773, section 4.1, and with
//! an Authority Key Identifier that holds no key identifier.

mod common;

use common::assert_runs;

/// An OpenSSL leaf (ECDSA P-256) with serial `a1b2c3d4e5f60718` and an
/// Authority Key Identifier of `79a3570c...58f5`.
const LEAF: &str = "308201763082011ba003020102020900a1b2c3d4e5f60718300a06082a8648ce3d04030230123110300e06035504030c0754657374204341301e170d3236313132373133333833315a170d3237303232353133333833315a30143112301006035504030c096c6561662e746573743059301306072a8648ce3d020106082a8648ce3d030107034200041bea25d960a2c6e777f520251378747f2b1a74dcd72eac954d98539b4164657b20caccdd7e1a797a33087790792033f671b4584d05b6c9c52d6a2095f59d4d00a3583056301f0603551d2304183016801479a3570cd157fc90725897d01e126e4303ad58f530140603551d11040d300b82096c6561662e74657374301d0603551d0e04160414f9c45928475e93a0164d1faed61ccfb8a2135e27300a06082a8648ce3d0403020349003046022100b33351b6c48c6704ab7a5bfa2d9415183967392f146fafa406fe82e50cc8cabd022100a56ff58ec609e83a3211928c29f65ff21f0eed24d8cf4fb543b3420b4deedbd2";

/// A self-signed certificate with no extensions at all.
const NO_AKI: &str = "308201583081fea003020102021401730ffcc812acff69132130ffebc998eb67f9d8300a06082a8648ce3d04030230123110300e06035504030c0754657374204341301e170d3236313132373133333833345a170d3336313132343133333833345a30123110300e06035504030c07546573742043413059301306072a8648ce3d020106082a8648ce3d030107034200041a046cbb158c505fcb9b321bd0e7a697b73c3a3eb4362f6ba295ae53178e8b5e14e816a55721961f0db05d60ef9fdfc3e3f87446613e4997eafdc900edaeee84a3323030301d0603551d0e0416041479a3570cd157fc90725897d01e126e4303ad58f5300f0603551d130101ff040530030101ff300a06082a8648ce3d0403020349003046022100d48b7986b051b2720fa7980d19d8b69f7786ba91ff93b720a336ac34b2d733a9022100b36ce2468cb51f7a3f99b5c2fbe1a65ebf180ffb05262b87aa3936719d04eab6";

/// DER: `tag`, the length, and `content`.
fn tlv(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    if content.len() < 128 {
        out.push(content.len() as u8);
    } else if content.len() < 256 {
        out.extend([0x81, content.len() as u8]);
    } else {
        out.extend([0x82, (content.len() >> 8) as u8, content.len() as u8]);
    }
    out.extend(content);
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A v3 certificate with `serial` (DER content bytes) and the extensions
/// `extensions` (each one a complete `Extension` SEQUENCE). Its signature
/// is not a real one: parsing does not check it.
fn certificate(serial: &[u8], extensions: &[Vec<u8>]) -> String {
    let ecdsa_sha256 = tlv(
        0x30,
        &[0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02],
    );
    let name = |cn: &str| {
        let attribute = tlv(
            0x30,
            &[vec![0x06, 0x03, 0x55, 0x04, 0x03], tlv(0x0c, cn.as_bytes())].concat(),
        );
        tlv(0x30, &tlv(0x31, &attribute))
    };
    let time = |text: &str| tlv(0x17, text.as_bytes());
    let validity = tlv(
        0x30,
        &[time("260101000000Z"), time("270101000000Z")].concat(),
    );
    // The P-256 generator as the subject key.
    let mut point = vec![0x00, 0x04];
    point.extend((0..64u8).map(|i| i.wrapping_mul(7).wrapping_add(1)));
    let spki = tlv(
        0x30,
        &[
            tlv(
                0x30,
                &[
                    vec![0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01],
                    vec![0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07],
                ]
                .concat(),
            ),
            tlv(0x03, &point),
        ]
        .concat(),
    );
    let mut tbs = vec![];
    tbs.extend(tlv(0xa0, &tlv(0x02, &[2])));
    tbs.extend(tlv(0x02, serial));
    tbs.extend(&ecdsa_sha256);
    tbs.extend(name("Issuer"));
    tbs.extend(validity);
    tbs.extend(name("Subject"));
    tbs.extend(spki);
    if !extensions.is_empty() {
        tbs.extend(tlv(0xa3, &tlv(0x30, &extensions.concat())));
    }
    let mut body = tlv(0x30, &tbs);
    body.extend(&ecdsa_sha256);
    body.extend(tlv(0x03, &[0x00, 0x30, 0x03, 0x02, 0x01, 0x01]));
    hex(&tlv(0x30, &body))
}

/// An Authority Key Identifier extension whose SEQUENCE holds `fields`.
fn aki(fields: &[u8]) -> Vec<u8> {
    tlv(
        0x30,
        &[
            vec![0x06, 0x03, 0x55, 0x1d, 0x23],
            tlv(0x04, &tlv(0x30, fields)),
        ]
        .concat(),
    )
}

/// The key identifier of the example in RFC 9773, section 4.1.
const RFC_KEY_ID: [u8; 20] = [
    0x69, 0x88, 0x5b, 0x6b, 0x87, 0x46, 0x40, 0x41, 0xe1, 0xb3, 0x7b, 0x84, 0x7b, 0xa0, 0xae, 0x2c,
    0xde, 0x01, 0xc8, 0xd4,
];

/// The serial of that example, `00:87:65:43:21`.
const RFC_SERIAL: [u8; 5] = [0x00, 0x87, 0x65, 0x43, 0x21];

/// A program that prints, for each hex certificate, the key identifier or
/// the error, the serial, and the ARI id or the error.
fn program(certs: &[&str]) -> String {
    let calls: String = certs
        .iter()
        .map(|c| format!("    report(io, \"{c}\")?\n"))
        .collect();
    format!(
        r#"use std.x509

fn hex(text: String) -> Untrusted<Bytes> {{
    Bytes.from_hex(text).unwrap_or(Bytes.new())
}}

fn report(io: Stdout, text: String) -> Result<(), IoError> {{
    match x509.Certificate.parse(hex(text)) {{
        Ok(cert) => {{
            match cert.authority_key_id() {{
                Ok(id) => {{
                    let h = id.to_hex()
                    io.println("aki:{{h}}")?
                }}
                Err(e) => {{
                    let m = e.to_string()
                    io.println("aki-err:{{m}}")?
                }}
            }}
            let s = cert.serial().to_hex()
            io.println("serial:{{s}}")?
            match cert.ari_id() {{
                Ok(id) => io.println("ari:{{id}}")
                Err(e) => {{
                    let m = e.to_string()
                    io.println("ari-err:{{m}}")
                }}
            }}
        }}
        Err(e) => {{
            let m = e.to_string()
            io.println("parse-err:{{m}}")
        }}
    }}
}}

fn main(io: Stdout) -> Result<(), IoError> {{
{calls}    Ok(())
}}
"#
    )
}

#[test]
fn an_openssl_leaf_gives_its_key_identifier_serial_and_ari_id() {
    // base64url of 79a3570c... and of 00a1b2c3d4e5f60718 (the leading zero
    // is part of the DER integer, as RFC 9773 says).
    assert_runs(
        &program(&[LEAF]),
        &[],
        "aki:79a3570cd157fc90725897d01e126e4303ad58f5\n\
         serial:00a1b2c3d4e5f60718\n\
         ari:eaNXDNFX_JByWJfQHhJuQwOtWPU.AKGyw9Tl9gcY\n",
    );
}

#[test]
fn the_rfc_9773_example_id_matches() {
    let cert = certificate(&RFC_SERIAL, &[aki(&tlv(0x80, &RFC_KEY_ID))]);
    assert_runs(
        &program(&[&cert]),
        &[],
        &format!(
            "aki:{}\nserial:0087654321\nari:aYhba4dGQEHhs3uEe6CuLN4ByNQ.AIdlQyE\n",
            hex(&RFC_KEY_ID)
        ),
    );
}

#[test]
fn a_certificate_with_no_authority_key_identifier_is_an_error_not_a_default() {
    let no_extension = certificate(&RFC_SERIAL, &[]);
    let other_extension = certificate(
        &RFC_SERIAL,
        &[tlv(
            0x30,
            &[
                vec![0x06, 0x03, 0x55, 0x1d, 0x13],
                tlv(0x04, &tlv(0x30, &[])),
            ]
            .concat(),
        )],
    );
    // An Authority Key Identifier that names no key (only a serial).
    let without_key_id = certificate(&RFC_SERIAL, &[aki(&tlv(0x82, &[1]))]);
    let missing = "the certificate has no authority key identifier";
    let own = |serial: &str| format!("aki-err:{missing}\nserial:{serial}\nari-err:{missing}\n");
    assert_runs(
        &program(&[&no_extension, &other_extension, &without_key_id, NO_AKI]),
        &[],
        &format!(
            "{}{}{}{}",
            own("0087654321"),
            own("0087654321"),
            own("0087654321"),
            own("01730ffcc812acff69132130ffebc998eb67f9d8")
        ),
    );
}
