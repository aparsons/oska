//! Secret integers (ticket #224), run on both backends through the
//! differential harness: wrapping `+ - *` on `Secret<U8>`, `Secret<U32>`
//! and `Secret<U64>` at the edges, with a secret or a public operand;
//! the bitwise operators, shifts and rotations against the public ones;
//! the truth tables of `ct_eq`, `ct_select`, `and`, `or`, `xor` and `not`;
//! `Secret<Bytes>`'s reads, pushes and constant-time comparisons;
//! `ct_select` on buffers of different lengths (`P0042`) and a shift by a
//! public amount out of range (`P0008`); `dbg` printing `<redacted>` in
//! direct and generic code; and the ChaCha20 block function (RFC 8439)
//! and the SHA-256 compression function (FIPS 180-4), written in Oska once
//! over public words and once over secret ones, against their test
//! vectors.

#![cfg(any(
    all(
        any(target_os = "linux", target_os = "macos"),
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(windows, target_arch = "x86_64")
))]

mod differential;

use differential::{assert_same, in_parallel, Case};

/// A secret integer type: its name and its width in bits.
const TYPES: [(&str, u32); 3] = [("U8", 8), ("U32", 32), ("U64", 64)];

/// A program whose `main` checks each `(expression, expected)` in
/// `checks`, each a secret of type `ty` compared, exposed, with the
/// literal `expected`, printing `bad <expression>` for each that does not
/// hold, then `done`. `lets` binds the secret values the expressions use,
/// and each with a `p` before its name to the same public value. The
/// checks are split among functions of a few each, which the backend
/// compiles small.
fn checking(ty: &str, lets: &[(String, u128)], checks: &[(String, u128)]) -> String {
    let mut bindings = String::new();
    for (name, v) in lets {
        bindings.push_str(&format!(
            "    let {name}: Secret<{ty}> = Secret.new({v})\n    let p{name}: {ty} = {v}\n"
        ));
    }
    let mut program = String::new();
    let mut main = String::new();
    for (n, chunk) in checks.chunks(12).enumerate() {
        let mut body = bindings.clone();
        for (expr, expected) in chunk {
            body.push_str(&format!(
                "    let want: {ty} = {expected}\n    \
                 if ({expr}).expose() != want {{\n        let _ = io.println(\"bad {expr}\")\n    }}\n"
            ));
        }
        program.push_str(&format!("fn part{n}(io: Stdout) {{\n{body}}}\n\n"));
        main.push_str(&format!("    part{n}(io)\n"));
    }
    format!("{program}fn main(io: Stdout) {{\n{main}    let _ = io.println(\"done\")\n}}\n")
}

fn assert_all_hold(name: &str, src: &str) {
    let outcome = assert_same(&Case::file(name, src));
    assert_eq!(outcome.stdout, "done\n", "case `{name}`: {outcome:#?}");
    assert_eq!(outcome.exit_code, Some(0), "case `{name}`: {outcome:#?}");
}

/// The edge values of a type `bits` wide.
fn edges(bits: u32) -> Vec<u128> {
    let max = (1u128 << bits) - 1;
    let pattern = 0xA5C3_0F96_5A3C_F069u128 & max;
    vec![
        0,
        1,
        2,
        u128::from(bits - 1),
        max / 2,
        max / 2 + 1,
        max - 1,
        max,
        pattern,
    ]
}

#[test]
fn secret_arithmetic_wraps_at_every_edge() {
    in_parallel(&TYPES, |&(ty, bits)| {
        let values = edges(bits);
        let m = 1u128 << bits;
        let lets: Vec<_> = values
            .iter()
            .enumerate()
            .map(|(i, &v)| (format!("v{i}"), v))
            .collect();
        let mut checks = Vec::new();
        for (i, &a) in values.iter().enumerate() {
            for (j, &b) in values.iter().enumerate() {
                checks.push((format!("v{i} + v{j}"), (a + b) % m));
                checks.push((format!("v{i} - v{j}"), (a + m - b) % m));
                checks.push((format!("v{i} * pv{j}"), a * b % m));
            }
        }
        // A literal operand takes the secret's type and wraps too.
        checks.push(("v7 + 1".to_string(), 0));
        checks.push(("1 - v1 - v1".to_string(), m - 1));
        assert_all_hold(
            &format!("secret_wrapping_{ty}"),
            &checking(ty, &lets, &checks),
        );
    });
}

#[test]
fn secret_bitwise_operators_and_rotations_match_the_public_ones() {
    in_parallel(&TYPES, |&(ty, bits)| {
        let values = edges(bits);
        let max = (1u128 << bits) - 1;
        let lets: Vec<_> = values
            .iter()
            .enumerate()
            .map(|(i, &v)| (format!("v{i}"), v))
            .collect();
        let mut checks = Vec::new();
        for (i, &a) in values.iter().enumerate() {
            checks.push((format!("~v{i}"), !a & max));
            for (j, &b) in values.iter().enumerate() {
                checks.push((format!("v{i} & v{j}"), a & b));
                checks.push((format!("v{i} | pv{j}"), a | b));
                checks.push((format!("pv{i} ^ v{j}"), a ^ b));
            }
            for n in [0, 1, bits / 2, bits - 1] {
                let rotl = ((a << n) | (a >> ((bits - n) % bits))) & max;
                let rotr = ((a >> n) | (a << ((bits - n) % bits))) & max;
                checks.push((format!("v{i}.rotl({n})"), rotl));
                checks.push((format!("v{i}.rotr({n})"), rotr));
                checks.push((format!("v{i} << {n}"), (a << n) & max));
                checks.push((format!("v{i} >> {n}"), a >> n));
            }
        }
        assert_all_hold(&format!("secret_bits_{ty}"), &checking(ty, &lets, &checks));
    });
}

#[test]
fn secret_conversions_widen_and_wrap() {
    let src = "fn main(io: Stdout) {\n    \
               let a: Secret<U8> = Secret.new(200)\n    \
               let b: Secret<U32> = Secret.new(4294967295)\n    \
               let c: Secret<U64> = Secret.new(18446744073709551615)\n    \
               let _ = io.println(a.to_u32().expose().to_int().to_string())\n    \
               let _ = io.println(a.to_u64().expose().to_int().to_string())\n    \
               let _ = io.println(b.to_u64().expose().to_int().to_string())\n    \
               let _ = io.println(b.wrapping_to_u8().expose().to_int().to_string())\n    \
               let _ = io.println(c.wrapping_to_u8().expose().to_int().to_string())\n    \
               let _ = io.println(c.wrapping_to_u32().expose().to_int().to_string())\n}\n";
    let outcome = assert_same(&Case::file("secret_conversions", src));
    assert_eq!(
        outcome.stdout, "200\n200\n4294967295\n255\n255\n4294967295\n",
        "{outcome:#?}"
    );
}

/// `Bool`'s text.
const SHOWN: &str = "fn shown(b: Secret<Bool>) -> String {\n    \
                     if b.expose() { \"t\" } else { \"f\" }\n}\n";

#[test]
fn ct_eq_and_ct_select_truth_tables() {
    let mut body = String::new();
    for ty in ["U8", "U32", "U64"] {
        for (a, b) in [(7, 7), (7, 9), (0, 255)] {
            body.push_str(&format!(
                "    let a: Secret<{ty}> = Secret.new({a})\n    \
                 let b: Secret<{ty}> = Secret.new({b})\n    \
                 let same = a.ct_eq(b)\n    \
                 let picked = same.ct_select(a, b).expose().to_int()\n    \
                 let other = same.not().ct_select(a, b).expose().to_int()\n    \
                 let s = shown(same)\n    \
                 let _ = io.println(\"{ty} {a} {b} {{s}} {{picked}} {{other}}\")\n"
            ));
        }
    }
    let src = format!("{SHOWN}\nfn main(io: Stdout) {{\n{body}}}\n");
    let outcome = assert_same(&Case::file("secret_ct_eq_select", &src));
    let mut expected = String::new();
    for ty in ["U8", "U32", "U64"] {
        expected.push_str(&format!(
            "{ty} 7 7 t 7 7\n{ty} 7 9 f 9 7\n{ty} 0 255 f 255 0\n"
        ));
    }
    assert_eq!(outcome.stdout, expected, "{outcome:#?}");
}

#[test]
fn secret_bool_truth_tables() {
    let mut body = String::new();
    for a in [false, true] {
        for b in [false, true] {
            body.push_str(&format!(
                "    let a = Secret.new({a})\n    let b = Secret.new({b})\n    \
                 let and = shown(a.and(b))\n    let or = shown(a.or(b))\n    \
                 let xor = shown(a.xor(b))\n    let not = shown(a.not())\n    \
                 let pick = shown(a.ct_select(b, a))\n    \
                 let _ = io.println(\"{{and}}{{or}}{{xor}}{{not}}{{pick}}\")\n"
            ));
        }
    }
    let src = format!("{SHOWN}\nfn main(io: Stdout) {{\n{body}}}\n");
    let outcome = assert_same(&Case::file("secret_bool_tables", &src));
    // and, or, xor, not a, a.ct_select(b, a)
    assert_eq!(
        outcome.stdout, "ffftf\nftttf\nfttff\nttfft\n",
        "{outcome:#?}"
    );
}

#[test]
fn ct_select_and_its_logic_evaluate_every_operand() {
    let src = "fn noisy(io: Stdout, name: String, b: Bool) -> Secret<Bool> {\n    \
               let _ = io.println(name)\n    Secret.new(b)\n}\n\n\
               fn main(io: Stdout) {\n    \
               let t = Secret.new(true)\n    \
               let f = Secret.new(false)\n    \
               let _ = f.and(noisy(io, \"and\", true))\n    \
               let _ = t.or(noisy(io, \"or\", true))\n    \
               let _ = t.ct_select(noisy(io, \"x\", true), noisy(io, \"y\", false))\n}\n";
    let outcome = assert_same(&Case::file("secret_select_evaluates_all", src));
    assert_eq!(outcome.stdout, "and\nor\nx\ny\n", "{outcome:#?}");
}

#[test]
fn secret_bytes_read_push_and_compare() {
    let src = "fn main(io: Stdout) {\n    \
               let plain = Bytes.from_hex(\"00112233445566778899aabbccddeeff\").unwrap_or(Bytes.new())\n    \
               var key = Secret.new(plain)\n    \
               let n = key.len()\n    \
               let _ = io.println(n.to_string())\n    \
               let _ = io.println(key.at(15).expose().to_int().to_string())\n    \
               let _ = io.println(key.read_u32_be(4).expose().to_int().to_string())\n    \
               let _ = io.println(key.read_u32_le(4).expose().to_int().to_string())\n    \
               let _ = io.println(key.read_u64_be(0).expose().to_int().to_string())\n    \
               let _ = io.println(key.read_u64_le(0).expose().to_int().to_string())\n    \
               let _ = io.println(key.slice(2, 5).expose().to_hex())\n    \
               key.push(Secret.new(1))\n    \
               key.push_u32_be(Secret.new(2))\n    \
               key.push_u32_le(Secret.new(3))\n    \
               key.push_u64_be(Secret.new(4))\n    \
               key.push_u64_le(Secret.new(5))\n    \
               key.append(Secret.new(Bytes.from_hex(\"beef\").unwrap_or(Bytes.new())))\n    \
               let grown = key.expose().slice(16, 43).to_hex()\n    \
               let _ = io.println(grown)\n    \
               let same = Secret.new(plain).constant_time_eq(plain)\n    \
               let differs = Secret.new(plain).constant_time_eq(plain.slice(0, 15))\n    \
               let twin = Secret.new(plain).constant_time_eq_secret(Secret.new(plain))\n    \
               let other = Secret.new(plain).constant_time_eq_secret(key)\n    \
               if same && !differs && twin && !other {\n        \
               let _ = io.println(\"compared\")\n    }\n    \
               let original = plain.to_hex()\n    \
               let _ = io.println(original)\n}\n";
    let outcome = assert_same(&Case::file("secret_bytes", src));
    assert_eq!(
        outcome.stdout,
        "16\n255\n1146447479\n2003195204\n4822678189205111\n8603657889541918976\n\
         223344\n01000000020300000000000000000000040500000000000000beef\ncompared\n\
         00112233445566778899aabbccddeeff\n",
        "{outcome:#?}"
    );
}

#[test]
fn ct_select_picks_between_secret_buffers() {
    let src = "fn main(io: Stdout) {\n    \
               let a = Secret.new(Bytes.from_hex(\"0102\").unwrap_or(Bytes.new()))\n    \
               let b = Secret.new(Bytes.from_hex(\"f0f0\").unwrap_or(Bytes.new()))\n    \
               let yes = Secret.new(true)\n    \
               let x = yes.ct_select(a, b).expose().to_hex()\n    \
               let y = yes.not().ct_select(a, b).expose().to_hex()\n    \
               let _ = io.println(\"{x} {y}\")\n    \
               let _ = yes.ct_select(a, Secret.new(Bytes.zeroed(3)))\n}\n";
    let outcome = assert_same(&Case::file("secret_select_bytes", src));
    assert_eq!(outcome.stdout, "0102 f0f0\n", "{outcome:#?}");
    assert!(
        outcome.stderr.contains(
            "error[P0042]: cannot pick between buffers of 2 and 3 bytes without revealing which"
        ),
        "{outcome:#?}"
    );
    assert_eq!(outcome.exit_code, Some(101), "{outcome:#?}");
}

#[test]
fn a_secret_shift_by_a_public_amount_out_of_range_panics() {
    let src = "fn main(io: Stdout) {\n    \
               let k: Secret<U32> = Secret.new(1)\n    \
               let n: U32 = 32\n    \
               let _ = k << n\n}\n";
    let outcome = assert_same(&Case::file("secret_shift_range", src));
    assert!(outcome.stderr.contains("error[P0008]"), "{outcome:#?}");
    assert_eq!(outcome.exit_code, Some(101), "{outcome:#?}");
}

#[test]
fn dbg_of_a_secret_is_redacted_in_direct_and_generic_code() {
    let src = "fn show<T>(x: T) -> T {\n    dbg(x)\n}\n\n\
               fn main(io: Stdout) {\n    \
               let a: Secret<U8> = Secret.new(1)\n    \
               let b: Secret<U32> = Secret.new(2)\n    \
               let c: Secret<U64> = Secret.new(3)\n    \
               let d = Secret.new(true)\n    \
               let e = Secret.new(Bytes.zeroed(2))\n    \
               let _ = dbg(a)\n    let _ = dbg(b)\n    let _ = dbg(c)\n    \
               let _ = dbg(d)\n    let _ = dbg(e)\n    \
               let _ = show(b)\n    let _ = show(e)\n    let _ = show([a])\n}\n";
    let outcome = assert_same(&Case::file("secret_dbg", src));
    assert_eq!(
        outcome.stderr,
        "[dbg] <redacted>\n".repeat(7) + "[dbg] [<redacted>]\n",
        "{outcome:#?}"
    );
}

/// The expected output of each crypto fixture: RFC 8439's quarter round
/// (section 2.1.1) and block (section 2.3.2) test vectors, and the
/// SHA-256 digests of "abc" and "" (FIPS 180-4's example, and the empty
/// message).
const CHACHA20: &str = "ea2a92f4\ncb1cf8ce\n4581472e\n5881c4bb\n\
                        10f1e7e4d13b5915500fdd1fa32071c4c7d1f4c733c068030422aa9ac3d46c4e\
                        d2826446079faa0914c2d705d98b02a2b5129cd1de164eb9cbd083e8a2503c4e\n";
const SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\n\
                      e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\n";

#[test]
fn chacha20_and_sha256_match_their_test_vectors_over_public_and_secret_words() {
    let fixtures = [
        (
            "chacha20_public",
            include_str!("fixtures/secret_crypto/chacha20_public.osk"),
            CHACHA20,
        ),
        (
            "chacha20_secret",
            include_str!("fixtures/secret_crypto/chacha20_secret.osk"),
            CHACHA20,
        ),
        (
            "sha256_public",
            include_str!("fixtures/secret_crypto/sha256_public.osk"),
            SHA256,
        ),
        (
            "sha256_secret",
            include_str!("fixtures/secret_crypto/sha256_secret.osk"),
            SHA256,
        ),
    ];
    in_parallel(&fixtures, |&(name, src, expected)| {
        let outcome = assert_same(&Case::file(name, src));
        assert_eq!(outcome.stdout, expected, "`{name}`: {outcome:#?}");
        assert_eq!(outcome.exit_code, Some(0), "`{name}`: {outcome:#?}");
    });
}
