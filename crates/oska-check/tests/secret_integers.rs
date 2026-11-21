//! Secret integers (#224): `Secret<U8>`, `Secret<U32>`, `Secret<U64>`,
//! `Secret<Bool>` and `Secret<Bytes>`. Code computes on them without
//! exposing them, and the checker refuses every use whose behaviour would
//! depend on a secret value: a branch (`E0353`), an index, an offset, a
//! length or a hash key (`E0354`), a division, a remainder or a shift or
//! rotation amount (`E0355`), and an ordering (`E0356`). `==` on one is
//! `E0351`, whose fix rewrites it to `ct_eq`.
//!
//! Each refusal is an attack: a program that would leak a secret through
//! timing, a panic or memory access. Without the check each one either
//! checks cleanly or is reported under another code, so each test names
//! the exact code and a fragment of the message.

mod common;

use common::{assert_audit, assert_errors, assert_fix, assert_ok, check};

const TYPES: [&str; 3] = ["U8", "U32", "U64"];

/// Assert that `src` reports exactly one diagnostic, `code` at `spanned`,
/// whose message contains `fragment`.
fn assert_refused(src: &str, code: &str, spanned: &str, fragment: &str) {
    assert_errors(src, &[(code, spanned)]);
    let report = check(src);
    let message = &report.diagnostics[0].message;
    assert!(
        message.contains(fragment),
        "the message of {code} should contain {fragment:?}: {message}"
    );
}

// ---- Allowed forms ----------------------------------------------------------

#[test]
fn secret_new_makes_a_secret_of_each_type() {
    assert_ok(
        "fn keys() {\n\
         \x20   let a: Secret<U8> = Secret.new(7)\n\
         \x20   let b: Secret<U32> = Secret.new(3735928559)\n\
         \x20   let c: Secret<U64> = Secret.new(1)\n\
         \x20   let d: Secret<Bool> = Secret.new(true)\n\
         \x20   let e: Secret<Bytes> = Secret.new(Bytes.zeroed(32))\n\
         }",
    );
}

#[test]
fn secret_new_is_not_an_audit_site_and_expose_is() {
    assert_audit(
        "fn f(x: U32) -> U32 {\n    let k = Secret.new(x)\n    k.expose()\n}",
        &[("expose", "k.expose()")],
    );
}

#[test]
fn expose_on_each_secret_type_is_an_audit_site() {
    assert_audit(
        "fn f(a: Secret<U8>, b: Secret<U32>, c: Secret<U64>, d: Secret<Bool>, e: Secret<Bytes>) {\n\
         \x20   let _ = a.expose()\n\
         \x20   let _ = b.expose()\n\
         \x20   let _ = c.expose()\n\
         \x20   let _ = d.expose()\n\
         \x20   let _ = e.expose()\n\
         }",
        &[
            ("expose", "a.expose()"),
            ("expose", "b.expose()"),
            ("expose", "c.expose()"),
            ("expose", "d.expose()"),
            ("expose", "e.expose()"),
        ],
    );
}

#[test]
fn every_wrapping_and_bitwise_operator_works_on_two_secrets() {
    for ty in TYPES {
        assert_ok(&format!(
            "fn f(a: Secret<{ty}>, b: Secret<{ty}>, n: {ty}) -> Secret<{ty}> {{\n\
             \x20   let sum = a + b\n\
             \x20   let difference = a - b\n\
             \x20   let product = a * b\n\
             \x20   let mixed = (a & b) | (a ^ b)\n\
             \x20   let flipped = ~a\n\
             \x20   let shifted = (a << n) | (b >> n)\n\
             \x20   ((sum ^ difference) ^ (product ^ mixed)) ^ (flipped ^ shifted)\n\
             }}"
        ));
    }
}

#[test]
fn a_public_operand_of_the_same_type_gives_a_secret_result() {
    for ty in TYPES {
        assert_ok(&format!(
            "fn f(k: Secret<{ty}>, p: {ty}) -> Secret<{ty}> {{\n\
             \x20   let a = k ^ 92\n\
             \x20   let b = 54 ^ k\n\
             \x20   let c = p + k\n\
             \x20   let d = k * p\n\
             \x20   (a | b) & (c - d)\n\
             }}"
        ));
    }
}

#[test]
fn a_public_operand_of_another_type_is_a_mismatch() {
    assert_errors(
        "fn f(k: Secret<U32>, p: U8) -> Secret<U32> {\n    k ^ p\n}",
        &[("E0220", "p")],
    );
}

#[test]
fn secret_operators_are_defined_for_the_secret_integer_types_only() {
    assert_refused(
        "fn f(k: Secret<String>, j: Secret<String>) {\n    let _ = k + j\n}",
        "E0223",
        "k + j",
        "secret integers",
    );
}

#[test]
fn rotations_and_conversions_keep_the_value_secret() {
    assert_ok(
        "fn f(a: Secret<U8>, b: Secret<U32>, c: Secret<U64>) -> Secret<U64> {\n\
         \x20   let x: Secret<U32> = a.to_u32().rotl(3)\n\
         \x20   let y: Secret<U64> = a.to_u64().rotr(1)\n\
         \x20   let z: Secret<U8> = b.wrapping_to_u8().rotl(1)\n\
         \x20   let w: Secret<U32> = c.wrapping_to_u32().rotr(7)\n\
         \x20   let v: Secret<U8> = c.wrapping_to_u8()\n\
         \x20   (x ^ w ^ b).to_u64() ^ y ^ z.to_u64() ^ v.to_u64() ^ c.rotl(13)\n\
         }",
    );
}

#[test]
fn a_secret_integer_has_no_checked_narrowing() {
    assert_refused(
        "fn f(k: Secret<U32>) {\n    let _ = k.to_u8()\n}",
        "E0228",
        "to_u8",
        "no method",
    );
}

#[test]
fn ct_eq_gives_a_secret_bool_and_ct_select_picks_without_a_branch() {
    for ty in TYPES {
        assert_ok(&format!(
            "fn f(a: Secret<{ty}>, b: Secret<{ty}>) -> Secret<{ty}> {{\n\
             \x20   let same: Secret<Bool> = a.ct_eq(b)\n\
             \x20   same.ct_select(a, b)\n\
             }}"
        ));
    }
    assert_ok(
        "fn f(c: Secret<Bool>, a: Secret<Bytes>, b: Secret<Bytes>, x: Secret<Bool>) -> Secret<Bytes> {\n\
         \x20   let _: Secret<Bool> = c.ct_select(x, x.not())\n\
         \x20   c.ct_select(a, b)\n\
         }",
    );
}

#[test]
fn ct_select_refuses_a_type_it_cannot_pick_without_a_branch() {
    assert_refused(
        "fn f(c: Secret<Bool>, a: Secret<String>, b: Secret<String>) {\n\
         \x20   let _ = c.ct_select(a, b)\n\
         }",
        "E0223",
        "c.ct_select(a, b)",
        "`ct_select`",
    );
}

#[test]
fn secret_bools_combine_without_short_circuiting() {
    assert_ok(
        "fn f(a: Secret<Bool>, b: Secret<Bool>) -> Secret<Bool> {\n\
         \x20   a.and(b).or(a.xor(b)).not()\n\
         }",
    );
}

#[test]
fn secret_bytes_read_at_public_offsets_into_secret_integers() {
    assert_ok(
        "fn f(key: Secret<Bytes>, i: Int) -> Secret<U64> {\n\
         \x20   let n: Int = key.len()\n\
         \x20   let b: Secret<U8> = key.at(i)\n\
         \x20   let part: Secret<Bytes> = key.slice(0, n - 1)\n\
         \x20   let w: Secret<U32> = part.read_u32_be(0) ^ part.read_u32_le(4)\n\
         \x20   key.read_u64_be(0) ^ key.read_u64_le(8) ^ w.to_u64() ^ b.to_u64()\n\
         }",
    );
}

#[test]
fn secret_bytes_grow_with_secret_integers() {
    assert_ok(
        "fn f(a: Secret<U8>, b: Secret<U32>, c: Secret<U64>, more: Secret<Bytes>) -> Secret<Bytes> {\n\
         \x20   var key = Secret.new(Bytes.new())\n\
         \x20   key.push(a)\n\
         \x20   key.push_u32_be(b)\n\
         \x20   key.push_u32_le(b)\n\
         \x20   key.push_u64_be(c)\n\
         \x20   key.push_u64_le(c)\n\
         \x20   key.append(more)\n\
         \x20   key\n\
         }",
    );
}

#[test]
fn secret_bytes_compare_in_constant_time() {
    assert_ok(
        "fn f(key: Secret<Bytes>, mac: Bytes, other: Secret<Bytes>) -> Bool {\n\
         \x20   key.constant_time_eq(mac) && key.constant_time_eq_secret(other)\n\
         }",
    );
}

#[test]
fn a_secret_list_may_be_iterated_because_its_length_is_public() {
    assert_ok(
        "fn f(keys: List<Secret<U32>>) -> Secret<U32> {\n\
         \x20   var acc: Secret<U32> = Secret.new(0)\n\
         \x20   for k in keys {\n\
         \x20       acc = acc ^ k\n\
         \x20   }\n\
         \x20   acc\n\
         }",
    );
}

// ---- E0353: a secret decides a branch --------------------------------------

/// A function over the secret `c: Secret<Bool>` and `k: Secret<U32>`
/// whose body is `body`.
fn over_secrets(body: &str) -> String {
    format!("fn f(c: Secret<Bool>, k: Secret<U32>, n: Int) -> Int {{\n    {body}\n}}")
}

#[test]
fn attack_a_secret_if_condition_is_e0353() {
    assert_refused(
        &over_secrets("if c { 1 } else { 0 }"),
        "E0353",
        "c",
        "`if` condition",
    );
}

#[test]
fn attack_a_secret_while_condition_is_e0353() {
    assert_refused(
        &over_secrets("while c { }\n    0"),
        "E0353",
        "c",
        "`while` condition",
    );
}

#[test]
fn attack_a_secret_match_scrutinee_is_e0353() {
    assert_refused(
        &over_secrets("match k { _ => 0 }"),
        "E0353",
        "k",
        "`match` scrutinee",
    );
}

#[test]
fn attack_a_secret_match_guard_is_e0353() {
    assert_refused(
        &over_secrets("match n {\n        _ if c => 1\n        _ => 0\n    }"),
        "E0353",
        "c",
        "`match` guard",
    );
}

#[test]
fn attack_a_secret_range_bound_is_e0353() {
    assert_refused(
        "fn f(k: Secret<U32>) {\n    for i in 0..k { }\n}",
        "E0353",
        "k",
        "range bound",
    );
}

#[test]
fn attack_a_secret_operand_of_and_or_is_e0353_with_a_fix() {
    let src = "fn f(a: Secret<Bool>, b: Secret<Bool>) -> Secret<Bool> {\n    a && b\n}";
    assert_refused(src, "E0353", "a && b", "`&&`");
    assert_fix(
        src,
        "E0353",
        "fn f(a: Secret<Bool>, b: Secret<Bool>) -> Secret<Bool> {\n    a.and(b)\n}",
    );
    let src = "fn f(a: Secret<Bool>, b: Secret<Bool>) -> Secret<Bool> {\n    a || b\n}";
    assert_refused(src, "E0353", "a || b", "`||`");
    assert_fix(
        src,
        "E0353",
        "fn f(a: Secret<Bool>, b: Secret<Bool>) -> Secret<Bool> {\n    a.or(b)\n}",
    );
}

#[test]
fn attack_a_secret_mixed_with_a_public_bool_in_and_is_e0353() {
    assert_refused(
        "fn f(a: Secret<Bool>, b: Bool) -> Bool {\n    b && a\n}",
        "E0353",
        "b && a",
        "`&&`",
    );
}

#[test]
fn attack_a_secret_operand_of_not_is_e0353_with_a_fix() {
    let src = "fn f(a: Secret<Bool>) -> Secret<Bool> {\n    !a\n}";
    assert_refused(src, "E0353", "!a", "`!`");
    assert_fix(
        src,
        "E0353",
        "fn f(a: Secret<Bool>) -> Secret<Bool> {\n    a.not()\n}",
    );
}

#[test]
fn attack_a_secret_contract_clause_is_e0353() {
    assert_refused(
        "fn f(a: Secret<U32>, b: Secret<U32>) -> Int\n    requires a.ct_eq(b)\n{\n    0\n}",
        "E0353",
        "a.ct_eq(b)",
        "`requires` condition",
    );
}

// ---- E0354: a secret decides a memory address ------------------------------

#[test]
fn attack_a_secret_list_index_is_e0354() {
    assert_refused(
        "fn f(table: List<U8>, k: Secret<U32>) -> U8 {\n    table.at(k)\n}",
        "E0354",
        "k",
        "index, an offset or a length",
    );
}

#[test]
fn attack_a_secret_bytes_offset_is_e0354() {
    assert_refused(
        "fn f(key: Secret<Bytes>, k: Secret<U32>) -> Secret<U32> {\n    key.read_u32_be(k)\n}",
        "E0354",
        "k",
        "index, an offset or a length",
    );
}

#[test]
fn attack_a_secret_slice_bound_is_e0354() {
    assert_refused(
        "fn f(data: Bytes, k: Secret<U64>) -> Bytes {\n    data.slice(0, k)\n}",
        "E0354",
        "k",
        "index, an offset or a length",
    );
}

#[test]
fn attack_a_secret_length_is_e0354() {
    assert_refused(
        "fn f(k: Secret<U8>) -> Bytes {\n    Bytes.zeroed(k)\n}",
        "E0354",
        "k",
        "index, an offset or a length",
    );
}

#[test]
fn attack_a_secret_hash_key_is_e0354() {
    assert_refused(
        "fn key<T: Hash>(x: T) -> Int {\n    x.hash()\n}\n\
         fn f(k: Secret<Bytes>) -> Int {\n    key(k)\n}",
        "E0354",
        "key(k)",
        "`Hash`",
    );
    assert_refused(
        "fn f(k: Secret<Bytes>) -> Int {\n    k.hash()\n}",
        "E0354",
        "hash",
        "`Hash`",
    );
}

// ---- E0355: a secret decides a variable-time instruction or a panic --------

#[test]
fn attack_a_secret_division_is_e0355() {
    assert_refused(
        "fn f(k: Secret<U32>) -> Secret<U32> {\n    k / 3\n}",
        "E0355",
        "k / 3",
        "`/`",
    );
    assert_refused(
        "fn f(k: Secret<U32>, p: U32) -> Secret<U32> {\n    p / k\n}",
        "E0355",
        "p / k",
        "`/`",
    );
}

#[test]
fn attack_a_secret_remainder_is_e0355() {
    assert_refused(
        "fn f(k: Secret<U64>) -> Secret<U64> {\n    k % 7\n}",
        "E0355",
        "k % 7",
        "`%`",
    );
}

#[test]
fn attack_a_secret_shift_amount_is_e0355() {
    assert_refused(
        "fn f(k: Secret<U32>, p: U32) -> Secret<U32> {\n    p << k\n}",
        "E0355",
        "p << k",
        "shift or rotation amount",
    );
    assert_refused(
        "fn f(k: Secret<U32>, j: Secret<U32>) -> Secret<U32> {\n    j >> k\n}",
        "E0355",
        "j >> k",
        "shift or rotation amount",
    );
}

#[test]
fn attack_a_secret_rotation_amount_is_e0355() {
    assert_refused(
        "fn f(k: Secret<U32>, n: Secret<U8>) -> Secret<U32> {\n    k.rotl(n)\n}",
        "E0355",
        "n",
        "shift or rotation amount",
    );
}

// ---- E0356: a secret ordering ----------------------------------------------

#[test]
fn attack_each_secret_ordering_is_e0356() {
    for op in ["<", "<=", ">", ">="] {
        let src = format!("fn f(k: Secret<U32>, p: U32) -> Bool {{\n    k {op} p\n}}");
        assert_refused(&src, "E0356", &format!("k {op} p"), &format!("`{op}`"));
    }
}

// ---- E0351: `==` on a secret, with the fix to `ct_eq` ----------------------

#[test]
fn attack_eq_on_secret_integers_is_e0351_fixed_to_ct_eq() {
    let src = "fn f(a: Secret<U32>, b: Secret<U32>) -> Bool {\n    a == b\n}";
    assert_errors(src, &[("E0351", "a == b")]);
    assert_fix(
        src,
        "E0351",
        "fn f(a: Secret<U32>, b: Secret<U32>) -> Bool {\n    a.ct_eq(b)\n}",
    );
    assert_fix(
        "fn f(a: Secret<U8>, b: Secret<U8>) -> Bool {\n    a != b\n}",
        "E0351",
        "fn f(a: Secret<U8>, b: Secret<U8>) -> Bool {\n    a.ct_eq(b).not()\n}",
    );
}

#[test]
fn attack_eq_on_secret_bytes_is_e0351_fixed_to_constant_time_eq_secret() {
    assert_fix(
        "fn f(a: Secret<Bytes>, b: Secret<Bytes>) -> Bool {\n    a == b\n}",
        "E0351",
        "fn f(a: Secret<Bytes>, b: Secret<Bytes>) -> Bool {\n    a.constant_time_eq_secret(b)\n}",
    );
    assert_fix(
        "fn f(a: Secret<Bytes>, b: Bytes) -> Bool {\n    a != b\n}",
        "E0351",
        "fn f(a: Secret<Bytes>, b: Bytes) -> Bool {\n    !a.constant_time_eq(b)\n}",
    );
}

#[test]
fn a_secret_integer_is_neither_displayed_nor_interpolated() {
    assert_errors(
        "fn f(k: Secret<U32>) -> String {\n    \"{k}\"\n}",
        &[("E0350", "{k}")],
    );
}

// ---- Every route: generics, closures, struct fields, lists -----------------

#[test]
fn attack_a_secret_through_a_generic_function_is_still_refused() {
    assert_refused(
        "fn first<T>(xs: List<T>) -> T {\n    xs.at(0)\n}\n\
         fn f(cs: List<Secret<Bool>>) -> Int {\n    if first(cs) { 1 } else { 0 }\n}",
        "E0353",
        "first(cs)",
        "`if` condition",
    );
}

#[test]
fn attack_a_secret_in_a_closure_is_still_refused() {
    assert_refused(
        "fn f(k: Secret<U32>) -> Secret<U32> {\n\
         \x20   let halve = |x: Secret<U32>| x / 2\n\
         \x20   halve(k)\n\
         }",
        "E0355",
        "x / 2",
        "`/`",
    );
}

#[test]
fn attack_a_secret_struct_field_is_still_refused() {
    assert_refused(
        "struct Key { flag: Secret<Bool>, word: Secret<U32> }\n\
         fn f(key: Key, table: List<U8>) -> U8 {\n    table.at(key.word)\n}",
        "E0354",
        "key.word",
        "index, an offset or a length",
    );
}

#[test]
fn attack_a_secret_list_element_is_still_refused() {
    assert_refused(
        "fn f(keys: List<Secret<U32>>, p: U32) -> Bool {\n    keys.at(0) < p\n}",
        "E0356",
        "keys.at(0) < p",
        "`<`",
    );
}

#[test]
fn a_secret_through_an_eq_bound_is_e0351() {
    assert_errors(
        "fn same<T: Eq>(a: T, b: T) -> Bool {\n    a == b\n}\n\
         fn f(a: Secret<U32>, b: Secret<U32>) -> Bool {\n    same(a, b)\n}",
        &[("E0351", "same(a, b)")],
    );
}
