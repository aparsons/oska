//! `std` modules and sink literal tags (#51): the prelude's `std.*`
//! modules, `use` resolving them, and a sink literal's tag naming the
//! literal type its imported module exports.

mod common;

use common::{assert_errors, assert_fix, assert_ok};

// ---- `use` ------------------------------------------------------------------------

#[test]
fn a_use_of_a_declared_std_module_resolves() {
    assert_ok("use std.sql");
}

#[test]
fn a_use_of_an_unknown_std_module_suggests_the_closest_one() {
    assert_errors("use std.sqll", &[("E0295", "std.sqll")]);
    assert_fix("use std.sqll", "E0295", "use std.sql");
}

#[test]
fn an_imported_module_exports_its_types() {
    assert_ok("use std.sql\nfn f(q: SqlLiteral) { }");
}

#[test]
fn a_module_type_is_unknown_without_its_use() {
    assert_errors("fn f(q: SqlLiteral) { }", &[("E0200", "SqlLiteral")]);
}
