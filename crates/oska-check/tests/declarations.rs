//! Declaration rules (#35): `cap` on capability-bearing types, `main` as
//! the root of authority, capitalization (ADR 0005), recursive types, and
//! `dbg` under `--release`.

mod common;

use common::{assert_errors, assert_fix, assert_ok};

// ---- `cap` -------------------------------------------------------------

#[test]
fn a_struct_holding_a_capability_must_be_declared_cap() {
    assert_errors(
        "struct Logger { out: Stdout }",
        &[("E0210", "Logger")],
    );
}

#[test]
fn the_missing_cap_fix_inserts_cap() {
    assert_fix(
        "pub struct Logger { out: Stdout }",
        "E0210",
        "pub cap struct Logger { out: Stdout }",
    );
}

#[test]
fn an_enum_holding_a_capability_must_be_declared_cap() {
    let src = "enum Sink {\n    Out(Stdout),\n    Nowhere,\n}";
    assert_errors(src, &[("E0210", "Sink")]);
    assert_fix(
        src,
        "E0210",
        "cap enum Sink {\n    Out(Stdout),\n    Nowhere,\n}",
    );
}

#[test]
fn a_cap_struct_may_hold_a_capability() {
    assert_ok("cap struct Logger { out: Stdout }");
    assert_ok("cap enum Sink {\n    Out(Stdout),\n    Nowhere,\n}");
}

#[test]
fn a_field_of_a_cap_type_or_cap_fn_needs_cap() {
    assert_errors(
        "cap struct Logger { out: Stdout }\nstruct App { log: Logger }",
        &[("E0210", "App")],
    );
    assert_errors("struct Hook { run: cap fn(Int) }", &[("E0210", "Hook")]);
}

#[test]
fn a_capability_inside_a_written_type_argument_needs_cap() {
    assert_errors(
        "struct Fanout { outs: List<Stdout> }",
        &[("E0210", "Fanout")],
    );
}

#[test]
fn generic_containers_and_plain_data_need_no_cap() {
    assert_ok("struct Box<T> { value: T }\nfn f(b: Box<Stdout>) { }");
    assert_ok("struct Scores { all: List<Int>, best: Option<Int>, hook: fn(Int) -> Int }");
}
