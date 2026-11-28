// SPDX-License-Identifier: Apache-2.0
//! The checker's rules for `SortedMap` and `SortedSet` (#806, spec #888,
//! ADR 0033): both are prelude names (`E0202` for a user type of that
//! name), a key or item type needs `Ord` (`Float` is `E0265`, a `Secret` is
//! `E0356`, the codes of the `Ord` refusal of `sorted`), a bound is refused
//! where the collection is named, and a mutating call needs a `var`
//! (`E0230`).

mod common;

use common::{assert_errors, assert_modules_errors, assert_modules_ok, assert_ok, check};

/// A program that makes a sorted set of `item`, then `body`.
fn set_program(item: &str, decls: &str, body: &str) -> String {
    format!("{decls}\nfn main() {{\n    var s: SortedSet<{item}> = SortedSet.new()\n{body}\n}}")
}

fn map_program(key: &str, decls: &str, body: &str) -> String {
    format!("{decls}\nfn main() {{\n    var m: SortedMap<{key}, Int> = SortedMap.new()\n{body}\n}}")
}

fn codes(src: &str) -> Vec<&'static str> {
    check(src).diagnostics.iter().map(|d| d.code).collect()
}

#[test]
fn both_check_with_no_import() {
    assert_ok(&set_program(
        "String",
        "",
        "    let _a = s.add(\"a\")\n    let n = s.len()\n    let f = s.first()\n    let l = s.last()\n    let r = s.remove(\"a\")\n    let l2 = s.to_list()",
    ));
    assert_ok(&map_program(
        "String",
        "",
        "    let _a = m.put(\"a\", 1)\n    let v = m.get(\"a\")\n    let r = m.remove(\"a\")\n    let f = m.first()\n    let l = m.last()\n    let c = m.contains(\"a\")\n    let es = m.to_list()",
    ));
}

#[test]
fn both_check_in_a_package_module_with_no_import() {
    assert_modules_ok(&[
        (
            "main",
            "use pkg.util\n\nfn main() {\n    let n = util.count(SortedSet.new(), SortedMap.new())\n}\n",
        ),
        (
            "util",
            "pub fn count(s: SortedSet<Int>, m: SortedMap<String, Int>) -> Int {\n    s.len() + m.len()\n}\n",
        ),
    ]);
}

#[test]
fn use_std_sorted_still_works() {
    assert_ok("use std.sorted\n\nfn main() {\n    let s: SortedSet<Int> = SortedSet.new()\n}\n");
}

#[test]
fn a_user_type_with_a_sorted_name_is_refused_as_a_prelude_name() {
    for name in ["SortedMap", "SortedSet", "SortedEntry"] {
        assert_errors(
            &format!("struct {name} {{ n: Int }}\n\nfn main() {{}}\n"),
            &[("E0202", name)],
        );
    }
}

#[test]
fn a_package_module_cannot_declare_them_either() {
    assert_modules_errors(
        &[
            ("main", "fn main() {}\n"),
            ("util", "pub enum SortedSet {\n    A,\n}\n"),
        ],
        &[("src/util.osk", "E0202", "SortedSet")],
    );
}

#[test]
fn every_ord_key_type_passes() {
    for item in [
        "Int",
        "I8",
        "U8",
        "U64",
        "Bool",
        "String",
        "Option<Int>",
        "List<String>",
        "List<Option<Int>>",
    ] {
        assert_ok(&set_program(item, "", "    let n = s.len()"));
        assert_ok(&map_program(item, "", "    let n = m.len()"));
    }
}

#[test]
fn a_user_type_with_impl_ord_is_a_key() {
    let decls = "struct V { n: Int }\n\
                 impl Ord for V {\n    fn cmp(self, other: V) -> Ordering { self.n.cmp(other.n) }\n}\n";
    assert_ok(&set_program(
        "V",
        decls,
        "    let _a = s.add(V(n: 1))\n    let f = s.first()",
    ));
    assert_ok(&map_program(
        "V",
        decls,
        "    let _a = m.put(V(n: 1), 2)\n    let g = m.get(V(n: 1))",
    ));
}

#[test]
fn a_type_without_ord_is_refused_where_the_collection_is_named() {
    let decls = "struct P { x: Int }\n";
    let src = set_program("P", decls, "    let _a = s.add(P(x: 1))");
    assert_errors(
        &src,
        &[("E0265", "SortedSet.new()"), ("E0265", "s.add(P(x: 1))")],
    );
    let src = map_program("P", decls, "");
    assert_errors(&src, &[("E0265", "SortedMap.new()")]);
}

#[test]
fn a_float_key_is_refused_with_the_ord_code_and_says_why() {
    let src = set_program("Float", "", "    let _a = s.add(1.5)");
    assert_eq!(codes(&src), ["E0265", "E0265"]);
    let text = check(&src).diagnostics[0].message.clone();
    assert!(text.contains("`Float: Ord`"), "{text}");
    assert!(text.contains("NaN"), "{text}");
    let src = map_program("Float", "", "    let _a = m.put(1.5, 2)");
    assert_eq!(codes(&src), ["E0265", "E0265"]);
}

#[test]
fn a_float_key_gets_the_same_code_as_sorted_on_a_list_of_floats() {
    let src = "fn main() {\n    let xs: List<Float> = []\n    let ys = xs.sorted()\n}";
    assert_errors(src, &[("E0265", "xs.sorted()")]);
}

#[test]
fn a_secret_key_is_refused_with_the_code_of_the_ord_refusal() {
    for item in ["Secret<Bytes>", "Secret<U32>", "Secret<Bool>"] {
        assert_eq!(codes(&set_program(item, "", "")), ["E0356"], "{item}");
        assert_eq!(codes(&map_program(item, "", "")), ["E0356"], "{item}");
    }
    let src = "fn main() {\n    let xs: List<Secret<U32>> = []\n    let ys = xs.sorted()\n}";
    assert_eq!(codes(src), ["E0356"]);
}

#[test]
fn a_secret_value_is_allowed() {
    assert_ok(
        "fn f(v: Secret<Bytes>) -> Int {\n    var m: SortedMap<Int, Secret<Bytes>> = SortedMap.new()\n    let _a = m.put(1, v)\n    m.len()\n}\nfn main() {}",
    );
}

#[test]
fn of_is_refused_for_a_float_key_too() {
    let src = "fn main() {\n    let s = SortedSet.of([1.5])\n}";
    assert!(codes(src).contains(&"E0265"), "{:?}", codes(src));
    let src = "fn main() {\n    let m = SortedMap.of([SortedEntry(key: 1.5, value: 2)])\n}";
    assert!(codes(src).contains(&"E0265"), "{:?}", codes(src));
}

#[test]
fn mutating_calls_need_a_var() {
    for call in ["s.add(2)", "s.remove(1)"] {
        let src = format!(
            "fn main() {{\n    let s: SortedSet<Int> = SortedSet.new()\n    let _x = {call}\n}}"
        );
        assert_errors(&src, &[("E0230", "s")]);
    }
    for call in ["m.put(1, 2)", "m.remove(1)"] {
        let src = format!("fn main() {{\n    let m: SortedMap<Int, Int> = SortedMap.new()\n    let _x = {call}\n}}");
        assert_errors(&src, &[("E0230", "m")]);
    }
    let src = "fn main() {\n    var s: SortedSet<Int> = SortedSet.new()\n    let _a = s.add(2)\n    let _b = s.remove(2)\n    var m: SortedMap<Int, Int> = SortedMap.new()\n    let _c = m.put(1, 2)\n    let _d = m.remove(1)\n}";
    assert_ok(src);
}

#[test]
fn reading_methods_need_no_var() {
    assert_ok(
        "fn main() {\n    let s: SortedSet<Int> = SortedSet.new()\n    let m: SortedMap<Int, Int> = SortedMap.new()\n    let a = s.contains(1)\n    let b = m.get(1)\n    let c = m.first()\n    let d = s.last()\n    let e = s.to_list()\n}",
    );
}

#[test]
fn a_sorted_collection_is_not_iterable_but_its_list_is() {
    assert_errors(
        "fn main() {\n    let s: SortedSet<Int> = SortedSet.new()\n    for x in s {\n        let y = x\n    }\n}",
        &[("E0235", "s")],
    );
    assert_ok(
        "fn main() {\n    let s: SortedSet<Int> = SortedSet.new()\n    for x in s.to_list() {\n        let y = x\n    }\n}",
    );
}

#[test]
fn map_equality_needs_value_eq_and_set_equality_checks() {
    assert_ok(
        "fn main() {\n    let a: SortedSet<Int> = SortedSet.new()\n    let b: SortedSet<Int> = SortedSet.new()\n    let m: SortedMap<Int, String> = SortedMap.new()\n    let n: SortedMap<Int, String> = SortedMap.new()\n    let e = a == b\n    let f = m == n\n}",
    );
}

#[test]
fn a_generic_function_over_a_key_type_is_told_the_bound() {
    let src = "fn count<T>(item: T) -> Int {\n    var s: SortedSet<T> = SortedSet.new()\n    let _a = s.add(item)\n    s.len()\n}\nfn main() {}";
    let report = check(src);
    assert!(!report.diagnostics.is_empty());
    assert!(
        report.diagnostics.iter().all(|d| d.code == "E0265"),
        "{:#?}",
        report.diagnostics
    );
}
