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
fn an_imported_module_exports_its_types_qualified() {
    assert_ok("use std.sql\nfn f(q: sql.SqlLiteral) { }");
}

#[test]
fn a_brace_group_may_name_what_the_module_exports() {
    assert_ok("use std.sql.{SqlLiteral}\nfn f(q: SqlLiteral) { }");
}

#[test]
fn a_brace_group_name_the_module_does_not_export_is_an_error() {
    let src = "use std.sql.{SqlLitreal}";
    assert_errors(src, &[("E0312", "SqlLitreal")]);
    assert_fix(src, "E0312", "use std.sql.{SqlLiteral}");
}

#[test]
fn a_module_type_is_unknown_without_its_use() {
    assert_errors("fn f(q: SqlLiteral) { }", &[("E0200", "SqlLiteral")]);
}

// ---- Sink literal tags ------------------------------------------------------------

#[test]
fn a_sink_literal_has_the_type_its_imported_module_exports() {
    assert_ok("use std.sql\nfn q() -> sql.SqlLiteral {\n    sql\"SELECT 1\"\n}");
}

#[test]
fn a_sink_literal_is_not_a_string() {
    assert_errors(
        "use std.sql\nfn q() -> String {\n    sql\"SELECT 1\"\n}",
        &[("E0220", "sql\"SELECT 1\"")],
    );
}

#[test]
fn a_sink_literal_needs_its_module_imported() {
    let src = "fn q() {\n    let s = sql\"SELECT 1\"\n}";
    assert_errors(src, &[("E0310", "sql")]);
    assert_fix(
        src,
        "E0310",
        "use std.sql\nfn q() {\n    let s = sql\"SELECT 1\"\n}",
    );
}

#[test]
fn an_unknown_sink_tag_suggests_the_closest_one() {
    let src = "use std.sql\nfn q() -> sql.SqlLiteral {\n    sqll\"SELECT 1\"\n}";
    assert_errors(src, &[("E0311", "sqll")]);
    assert_fix(
        src,
        "E0311",
        "use std.sql\nfn q() -> sql.SqlLiteral {\n    sql\"SELECT 1\"\n}",
    );
}

#[test]
fn an_unknown_sink_tag_with_nothing_close_has_no_fix() {
    let src = "fn q() {\n    let s = shell\"ls\"\n}";
    assert_errors(src, &[("E0311", "shell")]);
    let report = common::check(src);
    assert!(report.diagnostics[0].fix.is_none());
}
