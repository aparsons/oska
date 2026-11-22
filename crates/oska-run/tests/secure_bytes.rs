//! `Random.secure_bytes(n)` (#240): `n` secret bytes. On the test host they
//! are fixed, the generator's words little-endian from `FAKE_SEED`, so a
//! recorded handshake replays byte for byte, and the interpreter and the
//! native backend agree on them. A bad length panics as `Bytes.zeroed`'s.

mod common;

use common::{assert_panics, assert_runs, test_fake};
use oska_run::{TestStatus, FAKE_SEED};

/// The first `len` bytes the test host's generator gives, as hex.
fn fixed_hex(len: usize) -> String {
    let mut state = FAKE_SEED;
    let mut bytes = Vec::new();
    while bytes.len() < len {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        bytes.extend_from_slice(&(z ^ (z >> 31)).to_le_bytes());
    }
    bytes.truncate(len);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn a_run_gives_the_length_asked_for_in_bytes_that_vary() {
    // A run has no fixed bytes on a real OS; the plain test below holds
    // the test host's.
    let src = "fn main(io: Stdout, random: Random) {\n\
               \x20   let a = random.secure_bytes(300)\n\
               \x20   let b = random.secure_bytes(300)\n\
               \x20   let n = a.len()\n\
               \x20   let _ = io.println(\"{n}\")\n\
               \x20   let same = a.constant_time_eq_secret(b)\n\
               \x20   let _ = io.println(if same { \"same\" } else { \"varied\" })\n\
               }\n";
    assert_runs(src, &[], "300\nvaried\n");
}

#[test]
fn a_plain_test_gets_the_same_fixed_bytes_every_run() {
    let src = format!(
        "test \"fixed\" {{\n\
         \x20   let key = t.random.secure_bytes(8)\n\
         \x20   t.assert_eq(key.expose().to_hex(), \"{}\")\n\
         \x20   let more = t.random.secure_bytes(8)\n\
         \x20   t.assert(!key.constant_time_eq_secret(more))\n\
         }}\n",
        fixed_hex(8)
    );
    let report = test_fake(&src);
    assert!(report.diagnostics.is_empty(), "{report:#?}");
    assert_eq!(report.tests[0].status, TestStatus::Passed, "{report:#?}");
}

#[test]
fn zero_bytes_is_empty() {
    let src = "fn main(io: Stdout, random: Random) {\n\
               \x20   let n = random.secure_bytes(0).len()\n\
               \x20   let _ = io.println(\"{n}\")\n\
               }\n";
    assert_runs(src, &[], "0\n");
}

#[test]
fn a_negative_length_panics_as_zeroed_does() {
    let src = "fn main(random: Random) {\n    let _ = random.secure_bytes(-1)\n}\n";
    assert_panics(src, "P0042", "random.secure_bytes(-1)");
}

#[test]
fn a_length_past_the_limit_panics_as_zeroed_does() {
    let src = "fn main(random: Random) {\n    let _ = random.secure_bytes(268435457)\n}\n";
    assert_panics(src, "P0043", "random.secure_bytes(268435457)");
}
