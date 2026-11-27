// SPDX-License-Identifier: Apache-2.0
//! `std.ecdsa.sign` (#828): ECDSA P-256 signing over a `Secret` key, with
//! the deterministic nonce of RFC 6979. Every program here runs through the
//! `oska-run` API on a `FakeHost`, and on the native backend wherever it
//! runs (`common/native.rs`), so the vectors are held on both backends; the
//! other platforms run the same file in CI. That the machine code has no
//! branch or address that depends on the key or the nonce is held by
//! `oska-native`'s constant-time tests.
//!
//! Known answers: RFC 6979 appendix A.2.5 (P-256 with SHA-256, the messages
//! "sample" and "test"), and signatures of other keys (the ends of the
//! range among them) and digests (all zeros, all ones, the order, one) made
//! with OpenSSL's deterministic ECDSA (`cryptography` 50, RFC 6979).
//! Wycheproof's ECDSA set holds signatures to verify, not signing answers,
//! so it holds `verify` (`std_ecdsa`), and its ECDH scalars (the edge
//! cases of a private key) are signed with here, and checked with `verify`.

mod common;

use common::{run_fake, test_fake};
use oska_run::TestStatus;

/// (key, digest, signature `r || s`), all hexadecimal.
const KATS: &[(&str, &str, &str)] = &[
    ("c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721",
     "af2bdbe1aa9b6ec1e2ade1d694f41fc71a831d0268e9891562113d8a62add1bf",
     "efd48b2aacb6a8fd1140dd9cd45e81d69d2c877b56aaf991c34d0ea84eaf3716f7cb1c942d657c41d436c7a1b6e29f65f3e900dbb9aff4064dc4ab2f843acda8"),
    ("c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721",
     "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
     "f1abb023518351cd71d881567b1ea663ed3efcf6c5132b354f28d3b0b7d38367019f4113742a2b14bd25926b49c649155f267e60d3814b4c0cc84250e46f0083"),
    ("c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721",
     "0000000000000000000000000000000000000000000000000000000000000000",
     "68897a78df51058b490c6012251c95921abba96e2e488c8cc998942e440db9b780587fb387363a1df2c9e83c00f8ca990fc0a55b5e470946499b82ca3b552a87"),
    ("c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721",
     "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
     "1f2adbc54b88764c279f689fc9505959fc9e73e80dc20889a4e0be91865de75b9d109b65e2fbfc0ae42ba0b2e5f03670cd458cff4882df6783f3d93d607d1755"),
    ("c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721",
     "ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551",
     "68897a78df51058b490c6012251c95921abba96e2e488c8cc998942e440db9b780587fb387363a1df2c9e83c00f8ca990fc0a55b5e470946499b82ca3b552a87"),
    ("c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721",
     "0000000000000000000000000000000000000000000000000000000000000001",
     "492aa14595fcebaf00d14fe37d582462b0ce38bbe670a245be44ff6fdd0d772d082b82545bb6902dec6fe354a3921e3efc909325731c3cf906c35c4089e3f860"),
    ("0000000000000000000000000000000000000000000000000000000000000001",
     "af2bdbe1aa9b6ec1e2ade1d694f41fc71a831d0268e9891562113d8a62add1bf",
     "0466341174d59e93eb984c2a7c923a80ab99a9e91555bc73ebd8073d4c722121998f2b7bb63082e976215e6ae46344d66d2d4edea67d65d91595f21311df5030"),
    ("0000000000000000000000000000000000000000000000000000000000000001",
     "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
     "7ecc900ec818b3d8cfdc18f45de25dc953e1b9ae1a07a0d9cf8bd3f3e2c4bc86260e32d3a286ac502a62645d23a139398a9fe82f05de8e2e672a051a7c9cff9b"),
    ("0000000000000000000000000000000000000000000000000000000000000001",
     "0000000000000000000000000000000000000000000000000000000000000000",
     "0e6b3bb68d65aa645104de3abb94cbc6f568e2d55ea511d0e6d300e177002159167afc0b518bd63853ccc1b5292e27024c557c8924ca3dec81d59651b9021794"),
    ("0000000000000000000000000000000000000000000000000000000000000001",
     "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
     "bb39491f7fef3e14da8f0431d525575c587c358b05e71e2e5e3c0199bb9ec79853508ccce16e222da12f2a2c012d2240b7f6f8f6d43d5b1a452719a3b6846300"),
    ("0000000000000000000000000000000000000000000000000000000000000001",
     "ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551",
     "0e6b3bb68d65aa645104de3abb94cbc6f568e2d55ea511d0e6d300e177002159167afc0b518bd63853ccc1b5292e27024c557c8924ca3dec81d59651b9021794"),
    ("0000000000000000000000000000000000000000000000000000000000000001",
     "0000000000000000000000000000000000000000000000000000000000000001",
     "4b5c41b83db4cdef7604f3c9cc213cf2dc614962ece259ae18f20472c7b8b73d821dfbae0eee5c57777665eca707fc0d3fb95c8b682213037e98474f00694b50"),
    ("ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632550",
     "af2bdbe1aa9b6ec1e2ade1d694f41fc71a831d0268e9891562113d8a62add1bf",
     "a2e09df104aafc802a50c6684f6db83355a0a2588585a778b311e7e6c7c600ceb783aa569666ddbb05d0a5b9ce18b2757b27e2f6a4b4b1a3e4b4ced21f4174b2"),
    ("ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632550",
     "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
     "2ce08f4caf567510c7507c24e916ed767f67724844e9152a05bd91284b6bc8ed073eefa8066f28e4e357f97b3e118fbaa32656bef04d78636a86c9e1d9288e67"),
    ("ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632550",
     "0000000000000000000000000000000000000000000000000000000000000000",
     "668805e94b02193f59cb7b33dd8ae4195cfb767fd46b58893926cc19497f1dc31d883e8bfba9f070c9fb808695c304d1427c899ae8e988dc067b3b9113aa67f2"),
    ("ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632550",
     "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
     "77b47b05c7efc7af6da228f99727ae7cf6fc3aa80c36663ec4a56b2f7aa32f89bc70ca0c1e8f4530ed6ecf3088a1f47dad8bcf10fd61e1ff727dd77b147810b5"),
    ("ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632550",
     "ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551",
     "668805e94b02193f59cb7b33dd8ae4195cfb767fd46b58893926cc19497f1dc31d883e8bfba9f070c9fb808695c304d1427c899ae8e988dc067b3b9113aa67f2"),
    ("ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632550",
     "0000000000000000000000000000000000000000000000000000000000000001",
     "fef9515d65571f3b535ec2e515683d6915a969c4966c42000a817879dc19032f3e884bc6ee9f708d2c9032bb2eea9ee2fd5c89ef149e0fe02c6d0a2854ef5d23"),
    ("7fffffff800000007fffffffffffffffde737d56d38bcf4279dce5617e3192a8",
     "af2bdbe1aa9b6ec1e2ade1d694f41fc71a831d0268e9891562113d8a62add1bf",
     "51c215d1ffaaeea6b598f49402ddff8ee6a535288e63d78d2c79623fc7e81821016124887a399b339e9a497fef53ae0af0c2446f61350b3caf98cb0248820a95"),
    ("7fffffff800000007fffffffffffffffde737d56d38bcf4279dce5617e3192a8",
     "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
     "6b69694f3e515c94e2d8fa57a8601ca894ad65fab6bfad0af12eb9fbb9314490750dc446d48c8d0586373fa8502f3a7f7cfc1c32074dc008546939f10d876fe9"),
    ("7fffffff800000007fffffffffffffffde737d56d38bcf4279dce5617e3192a8",
     "0000000000000000000000000000000000000000000000000000000000000000",
     "099e39737ae2fd5666d2363ded5d5c4bf739b662fcf962e7ef6b4384228b801f43703159be18165297bbf9cb3d367b4da9084705acbb8c2c5cb044b7fef42f49"),
    ("7fffffff800000007fffffffffffffffde737d56d38bcf4279dce5617e3192a8",
     "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
     "dd6ab132c6817888193eda75005b4afd09b6b400d84d0650cf4fe6f58f1a9ff3b110d8c5737b9d723328e5d516485c1642fb38cb6936e6dc1ebe27be0d5c8fa7"),
    ("7fffffff800000007fffffffffffffffde737d56d38bcf4279dce5617e3192a8",
     "ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551",
     "099e39737ae2fd5666d2363ded5d5c4bf739b662fcf962e7ef6b4384228b801f43703159be18165297bbf9cb3d367b4da9084705acbb8c2c5cb044b7fef42f49"),
    ("7fffffff800000007fffffffffffffffde737d56d38bcf4279dce5617e3192a8",
     "0000000000000000000000000000000000000000000000000000000000000001",
     "05d277fbad53508a24d55e722145b3ff2ae1499f1644e7f909f428f4695005c84e1638be7ed9b4d0cbf5b7902f71ecc5904fa6c8d59d6adf803b25574bf2f9db"),
    ("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
     "af2bdbe1aa9b6ec1e2ade1d694f41fc71a831d0268e9891562113d8a62add1bf",
     "e7db36e5358836028ae34f7533c0681ab2b4f9265b7dd492c19777861449aea5185ba52f3e64599388f39aa7db8517ade20877a6570e561de302f993c6ed6ad2"),
    ("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
     "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
     "7f8431b05c48fa588021f1c460e4e6734263aac5e933b31f50b743f670ca9de815849a7294172f3e8db73363c41cf594dd62c1ab3dd2c9327e093b600ae67da4"),
    ("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
     "0000000000000000000000000000000000000000000000000000000000000000",
     "6fe90426dba605ed4af09452842d3c0cea2939df3da314d56c41469c072c30ffc91a1e8242251dd654a0113d40da69d968f12afecfe2cf72f55a78cfab42c3e6"),
    ("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
     "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
     "aec500aab99402b0c9527f033a6d4232905f25cfc8102eb2ea09a1391dcfe07a4352fbb066ed80905dda485b2bd298e6d5305ca922d88c3739c9dbf5f906f403"),
    ("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
     "ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551",
     "6fe90426dba605ed4af09452842d3c0cea2939df3da314d56c41469c072c30ffc91a1e8242251dd654a0113d40da69d968f12afecfe2cf72f55a78cfab42c3e6"),
    ("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
     "0000000000000000000000000000000000000000000000000000000000000001",
     "d38af7f57ddf0c8aa0c347ed8aa592de2bde4a876c517049253227eac2f2ae24ac632b16ca7078ab6c8ca628f7b60d909b4ca1cfb9bf22949c727d9ee4c3dc20"),
];

/// Wycheproof's `ecdh_secp256r1` private scalars with an edge case: the
/// smallest, the largest, and ones with long runs of zero or one bits.
const EDGE_KEYS: &[&str] = &[
    "0000000000000000000000000000000000000000000000000000000000000001",
    "ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632550",
    "00000000ffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    "8000000000000000000000000000000000000000000000000000000000000000",
];

const PRELUDE: &str = r#"use std.ecdsa
use std.p256
use std.sha2

fn hex(text: String) -> Bytes {
    Bytes.from_hex(text).unwrap_or(Bytes.new())
}

fn key(text: String) -> Secret<Bytes> {
    Secret.new(hex(text))
}

// The signature of `digest` under `text`, empty when it signs nothing.
fn sign_bytes(text: String, digest: Bytes) -> Bytes {
    match ecdsa.sign(key(text), digest) {
        Some(sig) => sig.expose()
        None => Bytes.new()
    }
}

// Whether the signature of `signed` under the private key `signer` verifies
// for `checked` under the public key of the private key `verifier`.
fn verifies(signer: String, verifier: String, signed: Bytes, checked: Bytes) -> Bool {
    match p256.public_key(key(verifier)) {
        Some(point) => match ecdsa.PublicKey.from_bytes(ecdsa.Curve.P256, point.expose()) {
            Some(k) => ecdsa.verify(k, checked, sign_bytes(signer, signed))
            None => false
        }
        None => false
    }
}

// The hexadecimal signature of `digest` under `text`, or "none".
fn signature(text: String, digest: Bytes) -> String {
    match ecdsa.sign(key(text), digest) {
        Some(sig) => sig.expose().to_hex()
        None => "none"
    }
}
"#;

/// Run the `test` blocks of `body` after the shared helpers and assert that
/// every one passed.
fn passes(body: &str) {
    let src = format!("{PRELUDE}\n{body}");
    let report = test_fake(&src);
    assert!(report.diagnostics.is_empty(), "{report:#?}");
    assert!(!report.tests.is_empty(), "{report:#?}");
    for test in &report.tests {
        assert_eq!(test.status, TestStatus::Passed, "{report:#?}");
    }
}

/// The diagnostics' codes of a program that `main` runs, which must not
/// check.
fn refused(body: &str) -> Vec<String> {
    let src = format!("{PRELUDE}\n{body}");
    let outcome = run_fake(&src, &[]);
    assert!(
        !outcome.diagnostics.is_empty(),
        "should be refused: {outcome:#?}"
    );
    assert_eq!(outcome.stdout, "", "a refused program prints nothing");
    outcome
        .diagnostics
        .iter()
        .map(|d| d.code.to_string())
        .collect()
}

// ---- Known answers ----------------------------------------------------------

#[test]
fn rfc_6979_p256_sha256_sample_and_test() {
    passes(
        r#"test "RFC 6979 A.2.5, SHA-256, sample and test" {
    let x = "c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721"
    t.assert_eq(signature(x, sha2.sha256("sample".to_bytes())), "efd48b2aacb6a8fd1140dd9cd45e81d69d2c877b56aaf991c34d0ea84eaf3716f7cb1c942d657c41d436c7a1b6e29f65f3e900dbb9aff4064dc4ab2f843acda8")
    t.assert_eq(signature(x, sha2.sha256("test".to_bytes())), "f1abb023518351cd71d881567b1ea663ed3efcf6c5132b354f28d3b0b7d38367019f4113742a2b14bd25926b49c649155f267e60d3814b4c0cc84250e46f0083")
}
"#,
    );
}

#[test]
fn known_answers_of_more_keys_and_digests() {
    let mut body = String::from("test \"known answers\" {\n");
    for (key, digest, signature) in KATS {
        body.push_str(&format!(
            "    t.assert_eq(signature(\"{key}\", hex(\"{digest}\")), \"{signature}\")\n"
        ));
    }
    body.push_str("}\n");
    passes(&body);
}

// ---- Verifying ----------------------------------------------------------------

#[test]
fn verify_accepts_what_sign_makes_for_digests_of_every_length() {
    let mut body = String::from(
        "fn pattern(length: Int) -> Bytes {\n    var out = Bytes.new()\n    for i in 0..length {\n        out.push(((i * 7 + 3) % 256).to_u8())\n    }\n    out\n}\n\ntest \"round trip\" {\n",
    );
    for key in EDGE_KEYS {
        body.push_str(&format!(
            "    for length in [0, 5, 32, 48, 100] {{\n\
             \x20       let digest = pattern(length)\n\
             \x20       t.assert(verifies(\"{key}\", \"{key}\", digest, digest))\n\
             \x20   }}\n"
        ));
    }
    body.push_str("}\n");
    passes(&body);
}

#[test]
fn a_signature_is_not_valid_for_another_digest_or_key() {
    passes(
        r#"test "only its own key and digest" {
    let x = "c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721"
    let digest = sha2.sha256("sample".to_bytes())
    let two = "0000000000000000000000000000000000000000000000000000000000000002"
    t.assert(verifies(x, x, digest, digest))
    t.assert(!verifies(x, x, digest, sha2.sha256("test".to_bytes())))
    t.assert(!verifies(x, two, digest, digest))
}
"#,
    );
}

// ---- Determinism and the nonce -------------------------------------------------

#[test]
fn the_same_key_and_digest_give_the_same_signature() {
    passes(
        r#"test "deterministic" {
    let x = "7fffffff800000007fffffffffffffffde737d56d38bcf4279dce5617e3192a8"
    let digest = sha2.sha256("again".to_bytes())
    let first = signature(x, digest)
    t.assert_eq(first, signature(x, digest))
    t.assert_eq(first, signature(x, digest))
    t.assert(first != "none")
}
"#,
    );
}

/// The attack: a nonce that repeats under one key, for different messages,
/// gives the key away. The nonce `r` is the x coordinate of `[k]G`, so a
/// repeated `k` is a repeated `r`. Twelve digests that differ in one
/// byte, under one key, give all different `r`; and the same digest under two keys gives different
/// `r` too.
#[test]
fn different_digests_never_share_a_nonce() {
    passes(
        r#"fn r_of(text: String, digest: Bytes) -> String {
    signature(text, digest).slice(0, 64)
}

test "no repeated r" {
    let x = "c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721"
    var seen = ""
    for i in 0..12 {
        var digest = Bytes.zeroed(32)
        digest.set(31, (i % 256).to_u8())
        digest.set(30, (i / 256).to_u8())
        let r = r_of(x, digest)
        t.assert(!seen.contains(r))
        seen = "{seen},{r}"
    }
    let digest = hex("af2bdbe1aa9b6ec1e2ade1d694f41fc71a831d0268e9891562113d8a62add1bf")
    t.assert(r_of(x, digest) != r_of("0000000000000000000000000000000000000000000000000000000000000002", digest))
}
"#,
    );
}

#[test]
fn there_is_no_way_to_pass_a_nonce() {
    // The signature of `sign` has two parameters: a third is an error.
    let codes = refused(
        r#"fn main(io: Stdout) {
    let nonce = key("0000000000000000000000000000000000000000000000000000000000000007")
    let _ = ecdsa.sign(key("0000000000000000000000000000000000000000000000000000000000000001"), Bytes.zeroed(32), nonce)
}
"#,
    );
    assert!(!codes.is_empty());
}

// ---- Keys that are refused -------------------------------------------------------

#[test]
fn a_key_that_is_not_in_range_signs_nothing() {
    passes(
        r#"test "bad keys" {
    let digest = Bytes.zeroed(32)
    t.assert_eq(signature("", digest), "none")
    t.assert_eq(signature("01", digest), "none")
    t.assert_eq(signature("00000000000000000000000000000000000000000000000000000000000000", digest), "none")
    t.assert_eq(signature("000000000000000000000000000000000000000000000000000000000000000001", digest), "none")
    t.assert_eq(signature("0000000000000000000000000000000000000000000000000000000000000000", digest), "none")
    t.assert_eq(signature("ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551", digest), "none")
    t.assert_eq(signature("ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632552", digest), "none")
    t.assert_eq(signature("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff", digest), "none")
    t.assert(signature("0000000000000000000000000000000000000000000000000000000000000001", digest) != "none")
}
"#,
    );
}

// ---- The key is a secret -----------------------------------------------------------

#[test]
fn the_key_never_prints_interpolates_or_compares() {
    let printed = refused(
        r#"fn main(io: Stdout) {
    let k = key("0000000000000000000000000000000000000000000000000000000000000001")
    let _ = io.println(k)
}
"#,
    );
    assert!(!printed.is_empty());
    let interpolated = refused(
        r#"fn main(io: Stdout) {
    let k = key("0000000000000000000000000000000000000000000000000000000000000001")
    let _ = io.println("key: {k}")
}
"#,
    );
    assert!(!interpolated.is_empty());
    let compared = refused(
        r#"fn main(io: Stdout) {
    let k = key("0000000000000000000000000000000000000000000000000000000000000001")
    let same = k == key("0000000000000000000000000000000000000000000000000000000000000001")
    let _ = io.println(if same { "same" } else { "different" })
}
"#,
    );
    assert!(!compared.is_empty());
    // And so is the signature: a caller exposes it where it needs it.
    let signature_printed = refused(
        r#"fn main(io: Stdout) {
    let sig = ecdsa.sign(key("0000000000000000000000000000000000000000000000000000000000000001"), Bytes.zeroed(32))
    let _ = io.println(sig)
}
"#,
    );
    assert!(!signature_printed.is_empty());
}
