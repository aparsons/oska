// SPDX-License-Identifier: Apache-2.0
//! `Map` and `Entry` are prelude names (#591, ADR 0028): a program uses
//! them with no `use`, and, like every prelude name, a program cannot
//! declare its own `Map` or `Entry` (`E0202`).

mod common;

use common::{assert_errors, assert_modules_errors, assert_modules_ok, assert_ok};

const USES: &str = "fn main() {\n    var m: Map<String, Int> = Map.new()\n    \
                    m.put(\"a\", 1)\n    let e = Entry(key: \"b\", value: 2)\n    m.put(e.key, e.value)\n}\n";

#[test]
fn map_and_entry_check_with_no_import() {
    assert_ok(USES);
}

#[test]
fn map_and_entry_check_in_a_package_module_with_no_import() {
    assert_modules_ok(&[
        (
            "main",
            "use pkg.util\n\nfn main() {\n    let n = util.count(Map.new())\n}\n",
        ),
        (
            "util",
            "pub fn count(m: Map<Int, Int>) -> Int {\n    m.len()\n}\n",
        ),
    ]);
}

#[test]
fn a_user_struct_named_map_is_refused_as_a_prelude_name() {
    assert_errors(
        "struct Map { n: Int }\n\nfn main() {\n    let m = Map(n: 1)\n}\n",
        &[("E0202", "Map")],
    );
}

#[test]
fn a_user_struct_named_entry_is_refused_as_a_prelude_name() {
    assert_errors(
        "struct Entry { n: Int }\n\nfn main() {\n    let e = Entry(n: 1)\n}\n",
        &[("E0202", "Entry")],
    );
}

#[test]
fn a_user_struct_named_entry_is_refused_even_when_nothing_else_mentions_a_map() {
    assert_errors(
        "struct Entry { n: Int }\n\nfn main() {}\n",
        &[("E0202", "Entry")],
    );
}

#[test]
fn a_package_module_cannot_declare_map_either() {
    assert_modules_errors(
        &[
            ("main", "fn main() {}\n"),
            ("util", "pub enum Map {\n    A,\n}\n"),
        ],
        &[("src/util.osk", "E0202", "Map")],
    );
}

#[test]
fn a_user_function_named_map_is_fine() {
    assert_ok("fn map(n: Int) -> Int {\n    n\n}\n\nfn main() {\n    let n = map(1)\n}\n");
}
