// SPDX-License-Identifier: Apache-2.0
//! `Deque` is a prelude name (#804, ADR 0032): a program uses it with no
//! `use`, and cannot declare its own (`E0202`).

mod common;

use common::{assert_errors, assert_ok};

#[test]
fn deque_checks_with_no_import() {
    assert_ok(
        "fn main() {\n    var d: Deque<Int> = Deque.new()\n    d.push_back(1)\n    d.push_front(0)\n    \
         let a = d.pop_back()\n    let n = d.len()\n}\n",
    );
}

#[test]
fn a_user_struct_named_deque_is_refused_as_a_prelude_name() {
    assert_errors(
        "struct Deque { n: Int }\n\nfn main() {\n    let d = Deque(n: 1)\n}\n",
        &[("E0202", "Deque")],
    );
}

#[test]
fn a_push_needs_a_var() {
    assert_errors(
        "fn main() {\n    let d: Deque<Int> = Deque.new()\n    d.push_back(1)\n}\n",
        &[("E0230", "d")],
    );
}
