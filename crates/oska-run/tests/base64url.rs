// SPDX-License-Identifier: Apache-2.0
//! `std.base64url` (#829, spec #824): unpadded base64url and the raw
//! `R || S` ES256 signature. Every program runs on the interpreter and,
//! where native executables run, natively, and must do the same on both
//! (ADR 0014); `crates/oskac/tests/native_base64url.rs` runs one on
//! every supported platform.

mod common;

use common::assert_runs;

/// Print `call` (an expression of `Result<Bytes, Base64UrlError>`) as hex,
/// or the error.
fn show(body: &str) -> String {
    format!(
        r#"use std.base64url

fn show(r: Result<Bytes, base64url.Base64UrlError>) -> String {{
    match r {{
        Ok(b) => {{
            let hex = b.to_hex()
            "ok:{{hex}}"
        }}
        Err(e) => {{
            let text = e.to_string()
            "err:{{text}}"
        }}
    }}
}}

fn main(io: Stdout) -> Result<(), IoError> {{
{body}
    Ok(())
}}
"#
    )
}

fn decode_lines(texts: &[&str]) -> String {
    let body: String = texts
        .iter()
        .map(|t| format!("    io.println(show(base64url.decode(\"{t}\")))?\n"))
        .collect();
    show(&body)
}

#[test]
fn rfc_4648_vectors_encode() {
    // RFC 4648, section 10, without the padding.
    let pairs = [
        ("", ""),
        ("f", "Zg"),
        ("fo", "Zm8"),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg"),
        ("fooba", "Zm9vYmE"),
        ("foobar", "Zm9vYmFy"),
    ];
    let body: String = pairs
        .iter()
        .map(|(plain, _)| format!("    io.println(base64url.encode(\"{plain}\".to_bytes()))?\n"))
        .collect();
    let expected: String = pairs.iter().map(|(_, enc)| format!("{enc}\n")).collect();
    assert_runs(&show(&body), &[], &expected);
}

#[test]
fn rfc_4648_vectors_decode() {
    let texts = ["", "Zg", "Zm8", "Zm9v", "Zm9vYg", "Zm9vYmE", "Zm9vYmFy"];
    let hex = |s: &str| -> String { s.bytes().map(|b| format!("{b:02x}")).collect() };
    let plain = ["", "f", "fo", "foo", "foob", "fooba", "foobar"];
    let expected: String = plain.iter().map(|p| format!("ok:{}\n", hex(p))).collect();
    assert_runs(&decode_lines(&texts), &[], &expected);
}

#[test]
fn the_url_safe_alphabet_uses_dash_and_underscore() {
    // 0xfb 0xff 0xbf is 111110 111111 111110 111111: `-`, `_`, `-`, `_`
    // where the standard alphabet has `+`, `/`, `+`, `/`.
    let src = show(
        r#"    io.println(base64url.encode(Bytes.from_hex("fbffbf").unwrap_or(Bytes.new())))?
    io.println(base64url.encode(Bytes.from_hex("fbff").unwrap_or(Bytes.new())))?
    io.println(show(base64url.decode("-_-_")))?
"#,
    );
    assert_runs(&src, &[], "-_-_\n-_8\nok:fbffbf\n");
}

#[test]
fn every_length_round_trips() {
    // Bytes 0..=255 in prefixes of every length from 0 to 256, and with
    // the high bits set, encode and decode back to themselves.
    let src = r#"use std.base64url

fn main(io: Stdout) -> Result<(), IoError> {
    var all = Bytes.new()
    var bad = 0
    for i in 0..257 {
        let back = base64url.decode(base64url.encode(all))
        match back {
            Ok(b) => {
                let got = b.to_hex()
                let want = all.to_hex()
                if got != want {
                    bad = bad + 1
                }
            }
            Err(_) => { bad = bad + 1 }
        }
        all.push((i * 7 % 256).to_u8())
    }
    io.println("mismatches: {bad}")?
    Ok(())
}
"#;
    assert_runs(src, &[], "mismatches: 0\n");
}

#[test]
fn padding_is_refused() {
    let src = decode_lines(&["Zg==", "Zm8=", "=", "Zm9v="]);
    assert_runs(
        &src,
        &[],
        "err:base64url text has padding\n".repeat(4).as_str(),
    );
}

#[test]
fn the_standard_alphabet_is_refused() {
    let src = decode_lines(&["+_8", "-/8", "+/+/"]);
    assert_runs(
        &src,
        &[],
        "err:base64url text uses the standard alphabet\n"
            .repeat(3)
            .as_str(),
    );
}

#[test]
fn stray_characters_are_refused() {
    let src = decode_lines(&["Zm 9v", "Zm9v\\n", "Zm9v!", " Zm9v", "Zm.v", "Zm9é"]);
    assert_runs(
        &src,
        &[],
        "err:base64url text has a character outside the alphabet\n"
            .repeat(6)
            .as_str(),
    );
}

#[test]
fn a_length_no_bytes_encode_to_is_refused() {
    let src = decode_lines(&["Z", "Zm9vY", "Zm9vYmFyZ"]);
    assert_runs(
        &src,
        &[],
        "err:base64url text has a length no bytes encode to\n"
            .repeat(3)
            .as_str(),
    );
}

#[test]
fn unused_bits_that_are_not_zero_are_refused() {
    // `Zh` has the 'f' bits and then a set unused bit; `Zm9` likewise.
    let src = decode_lines(&["Zh", "Zm9", "Zm9vYh"]);
    assert_runs(
        &src,
        &[],
        "err:base64url text has unused bits that are not zero\n"
            .repeat(3)
            .as_str(),
    );
}

#[test]
fn a_raw_signature_pads_each_half_to_32_bytes() {
    let r = "01".repeat(32);
    let s = "02".repeat(32);
    let src = show(&format!(
        r#"    let r = Bytes.from_hex("{r}").unwrap_or(Bytes.new())
    let s = Bytes.from_hex("{s}").unwrap_or(Bytes.new())
    io.println(show(base64url.es256_raw(r, s)))?
"#
    ));
    assert_runs(&src, &[], &format!("ok:{r}{s}\n"));
}

#[test]
fn a_half_with_leading_zeros_keeps_its_32_bytes() {
    // r has two leading zero bytes (30 significant), s has one (31), and
    // a short s of one byte is padded from the left.
    let r30 = "ab".repeat(30);
    let s31 = "cd".repeat(31);
    let src = show(&format!(
        r#"    let r = Bytes.from_hex("0000{r30}").unwrap_or(Bytes.new())
    let s = Bytes.from_hex("00{s31}").unwrap_or(Bytes.new())
    io.println(show(base64url.es256_raw(r, s)))?
    let one = Bytes.from_hex("07").unwrap_or(Bytes.new())
    io.println(show(base64url.es256_raw(one, Bytes.new())))?
"#
    ));
    let expected = format!(
        "ok:0000{r30}00{s31}\nok:{}07{}\n",
        "00".repeat(31),
        "00".repeat(32)
    );
    assert_runs(&src, &[], &expected);
}

#[test]
fn a_der_sign_byte_is_dropped_and_a_half_over_32_bytes_is_refused() {
    // 33 bytes with a leading 0x00 (as DER writes a top-bit-set integer)
    // fit; 33 significant bytes do not.
    let r = format!("00{}", "ff".repeat(32));
    let long = format!("01{}", "ff".repeat(32));
    let src = show(&format!(
        r#"    let r = Bytes.from_hex("{r}").unwrap_or(Bytes.new())
    let s = Bytes.from_hex("05").unwrap_or(Bytes.new())
    io.println(show(base64url.es256_raw(r, s)))?
    let long = Bytes.from_hex("{long}").unwrap_or(Bytes.new())
    io.println(show(base64url.es256_raw(long, s)))?
    io.println(show(base64url.es256_raw(s, long)))?
"#
    ));
    let expected = format!(
        "ok:{}{}05\nerr:the ES256 signature is malformed\nerr:the ES256 signature is malformed\n",
        "ff".repeat(32),
        "00".repeat(31)
    );
    assert_runs(&src, &[], &expected);
}

#[test]
fn a_der_signature_becomes_the_raw_form_and_a_loose_one_is_refused() {
    // SEQUENCE { INTEGER 0x0080 (r = 128), INTEGER 0x05 (s = 5) }.
    let src = show(
        r#"    let ok = Bytes.from_hex("300702020080020105").unwrap_or(Bytes.new())
    io.println(show(base64url.es256_raw_from_der(ok)))?
    // A non-minimal integer (a needless leading zero).
    let loose = Bytes.from_hex("300702020001020105").unwrap_or(Bytes.new())
    io.println(show(base64url.es256_raw_from_der(loose)))?
    // Trailing bytes after the sequence.
    let trailing = Bytes.from_hex("30070202008002010500").unwrap_or(Bytes.new())
    io.println(show(base64url.es256_raw_from_der(trailing)))?
    // A negative integer.
    let negative = Bytes.from_hex("3006020180020105").unwrap_or(Bytes.new())
    io.println(show(base64url.es256_raw_from_der(negative)))?
    io.println(show(base64url.es256_raw_from_der(Bytes.new())))?
"#,
    );
    let bad = "err:the ES256 signature is malformed\n";
    let expected = format!(
        "ok:{}80{}05\n{}",
        "00".repeat(31),
        "00".repeat(31),
        bad.repeat(4)
    );
    assert_runs(&src, &[], &expected);
}

#[test]
fn a_jws_signing_input_from_rfc_7515_encodes_as_the_rfc_shows() {
    // RFC 7515, appendix A.1: the protected header `{"typ":"JWT",\r\n
    // "alg":"HS256"}` is `eyJ0eXAiOiJKV1QiLA0KICJhbGciOiJIUzI1NiJ9`.
    let header = "{\"typ\":\"JWT\",\r\n \"alg\":\"HS256\"}";
    let hex: String = header.bytes().map(|b| format!("{b:02x}")).collect();
    let src = show(&format!(
        r#"    let header = Bytes.from_hex("{hex}").unwrap_or(Bytes.new())
    io.println(base64url.encode(header))?
    io.println(show(base64url.decode("eyJ0eXAiOiJKV1QiLA0KICJhbGciOiJIUzI1NiJ9")))?
"#
    ));
    assert_runs(
        &src,
        &[],
        &format!("eyJ0eXAiOiJKV1QiLA0KICJhbGciOiJIUzI1NiJ9\nok:{hex}\n"),
    );
}
