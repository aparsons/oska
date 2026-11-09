//! Contracts and test bodies (#41): `requires` and `ensures` clauses are
//! type-checked and may call only pure functions; `test` bodies are
//! checked with the built-in `t`.

mod common;

use common::{assert_errors, assert_fix, assert_ok};

// ---- Contract conditions -----------------------------------------------------

#[test]
fn a_requires_clause_must_be_bool() {
    assert_ok("fn f(amount: Int) -> Int\n    requires amount > 0\n{\n    amount\n}");
    assert_errors(
        "fn f(amount: Int) -> Int\n    requires amount + 1\n{\n    amount\n}",
        &[("E0281", "amount + 1")],
    );
}

#[test]
fn an_ensures_clause_must_be_bool() {
    assert_errors(
        "fn f(amount: Int) -> Int\n    ensures |r| r\n{\n    amount\n}",
        &[("E0281", "r")],
    );
}

#[test]
fn ensures_binds_the_return_type() {
    assert_ok("fn f(s: String) -> String\n    ensures |r| r.len() >= s.len()\n{\n    s\n}");
    assert_errors(
        "fn f(amount: Int) -> Int\n    ensures |r| r.len() > 0\n{\n    amount\n}",
        &[("E0228", "len")],
    );
}

#[test]
fn the_ensures_binding_is_scoped_to_its_clause() {
    assert_errors(
        "fn f(amount: Int) -> Int\n    requires amount > 0\n    ensures |r| r > 0\n{\n    r\n}",
        &[("E0221", "r")],
    );
}

#[test]
fn a_contract_sees_parameters_but_not_body_locals() {
    assert_ok("fn f(a: Int, b: Int) -> Int\n    requires a < b\n{\n    b - a\n}");
    assert_errors(
        "fn f(a: Int) -> Int\n    requires total > 0\n{\n    let total = a\n    total\n}",
        &[("E0221", "total")],
    );
}

// ---- Contracts call only pure functions ----------------------------------------

#[test]
fn a_contract_may_call_a_pure_function() {
    assert_ok(
        "fn positive(n: Int) -> Bool { n > 0 }\nfn f(n: Int) -> Int\n    requires positive(n)\n{\n    n\n}",
    );
}

#[test]
fn a_contract_that_calls_a_function_taking_a_capability_is_an_error() {
    assert_errors(
        "fn ready(io: Stdout) -> Bool { true }\nfn f(io: Stdout) -> Int\n    requires ready(io)\n{\n    1\n}",
        &[("E0280", "ready(io)")],
    );
}

#[test]
fn a_contract_that_calls_a_capability_method_is_an_error() {
    assert_errors(
        "fn f(io: Stdout) -> Int\n    requires io.println(\"checking\") == Ok(())\n{\n    1\n}",
        &[("E0280", "io.println(\"checking\")")],
    );
    assert_errors(
        "fn f(io: Stdout) -> Int\n    requires Stdout.println(io, \"x\") == Ok(())\n{\n    1\n}",
        &[("E0280", "Stdout.println(io, \"x\")")],
    );
}

#[test]
fn a_contract_that_calls_a_method_of_a_cap_struct_is_an_error() {
    assert_errors(
        "cap struct Logger { out: Stdout }\nimpl Logger {\n    fn ready(self) -> Bool { true }\n}\nfn f(log: Logger) -> Int\n    requires log.ready()\n{\n    1\n}",
        &[("E0280", "log.ready()")],
    );
}

#[test]
fn a_contract_may_call_a_plain_closure_but_not_a_cap_fn() {
    assert_ok("fn f(n: Int, ok: fn(Int) -> Bool) -> Int\n    requires ok(n)\n{\n    n\n}");
    assert_errors(
        "fn f(n: Int, ok: cap fn(Int) -> Bool) -> Int\n    requires ok(n)\n{\n    n\n}",
        &[("E0280", "ok(n)")],
    );
}

#[test]
fn a_body_may_still_call_functions_taking_capabilities() {
    assert_ok(
        "fn ready(io: Stdout) -> Bool { true }\nfn f(io: Stdout) -> Bool\n    requires true\n{\n    ready(io)\n}",
    );
}

#[test]
fn a_contract_on_a_body_less_trait_method_is_checked() {
    assert_errors(
        "trait Sized {\n    fn size(self) -> Int\n        ensures |r| r\n}",
        &[("E0281", "r")],
    );
}

// ---- Test bodies ---------------------------------------------------------------

#[test]
fn a_test_body_is_type_checked() {
    assert_errors(
        "test \"adds\" {\n    let x: Int = \"one\"\n}",
        &[("E0220", "\"one\"")],
    );
}

#[test]
fn the_design_notes_test_checks() {
    assert_ok(
        "struct Account { pub id: Int, balance: Int }\nimpl Account {\n    pub fn deposit(var self, amount: Int)\n        requires amount > 0\n    {\n        self.balance = self.balance + amount\n    }\n}\ntest \"deposit adds to the balance\" {\n    var a = Account(id: 1, balance: 0)\n    a.deposit(amount: 5)\n    t.assert_eq(a.balance, 5)\n}",
    );
}

#[test]
fn assertions_check_their_arguments() {
    assert_ok("test \"asserts\" {\n    t.assert(1 < 2)\n    t.assert_ne(\"a\", \"b\")\n    t.fail(\"unreachable\")\n}");
    assert_errors(
        "test \"asserts\" {\n    t.assert_eq(1, \"one\")\n}",
        &[("E0220", "\"one\"")],
    );
    assert_errors("test \"asserts\" {\n    t.assert(1)\n}", &[("E0220", "1")]);
}

#[test]
fn t_holds_the_stand_in_capabilities() {
    assert_ok(
        "fn greet(io: Stdout) -> Result<(), IoError> {\n    io.println(\"hi\")\n}\nfn tick(clock: Clock, random: Random, fs: FsRead) {}\ntest \"greets\" {\n    let _ = greet(t.stdout)\n    tick(t.clock, t.random, t.fs.read_only())\n}",
    );
}

#[test]
fn t_has_no_other_members() {
    assert_fix(
        "test \"net\" {\n    let n = t.fz\n}",
        "E0227",
        "test \"net\" {\n    let n = t.fs\n}",
    );
    assert_errors(
        "test \"net\" {\n    t.assert_equal(1, 1)\n}",
        &[("E0228", "assert_equal")],
    );
}

#[test]
fn an_integration_test_uses_the_same_t() {
    assert_ok("#[integration]\ntest \"prints\" {\n    let _ = t.stdout.println(\"hi\")\n}");
}

#[test]
fn t_exists_only_in_tests() {
    assert_errors("fn f() {\n    t.assert(true)\n}", &[("E0221", "t")]);
}

#[test]
fn a_test_has_no_capabilities_besides_t() {
    assert_errors(
        "test \"forges\" {\n    let _ = io.println(\"hi\")\n}",
        &[("E0221", "io")],
    );
}

#[test]
fn a_method_contract_sees_self() {
    assert_ok(
        "struct Account { balance: Int }\nimpl Account {\n    fn withdraw(self, amount: Int) -> Int\n        requires amount <= self.balance\n    {\n        self.balance - amount\n    }\n}",
    );
}
