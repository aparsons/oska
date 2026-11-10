//! Preconditions enforced at runtime (#106, spec #105): a function's
//! `requires` clauses are evaluated on every call, in every build, and a
//! false one stops the program with `P0006`, "precondition violated",
//! spanning the call that broke it.

mod common;

use common::{assert_panics, assert_refused, assert_runs, run_fake, test_fake};
use oska_run::TestStatus;

/// `double`, which requires a positive argument.
const DOUBLE: &str = "fn double(x: Int) -> Int
    requires x > 0
{
    x * 2
}
";

#[test]
fn a_false_requires_panics_with_p0006_spanning_the_call() {
    let src = format!("{DOUBLE}\nfn main() {{\n    let y = double(-1)\n}}\n");
    assert_panics(&src, "P0006", "double(-1)");
}

#[test]
fn the_panic_quotes_the_first_false_clause_in_the_order_written() {
    let src = "struct Account { balance: Int }

impl Account {
    fn withdraw(self, amount: Int) -> Account
        requires amount > 0
        requires amount <= self.balance
    {
        Account(balance: self.balance - amount)
    }
}

fn main() {
    let account = Account(balance: 10)
    let after = account.withdraw(25)
}
";
    assert_panics(src, "P0006", "account.withdraw(25)");
    let panic = run_fake(src, &[]).panic.expect("a panic");
    assert_eq!(
        panic.message,
        "precondition violated: `Account.withdraw` requires `amount <= self.balance`"
    );
}

#[test]
fn the_arguments_are_evaluated_first_and_the_body_runs_only_when_every_clause_holds() {
    let src = "fn shown(io: Stdout, n: Int) -> Int {
    let _ = io.println(\"argument {n}\")
    n
}

fn report(io: Stdout, n: Int)
    requires n > 0
{
    let _ = io.println(\"body {n}\")
}

fn main(io: Stdout) {
    report(io, shown(io, 3))
    report(io, shown(io, -1))
    let _ = io.println(\"unreachable\")
}
";
    assert_panics(src, "P0006", "report(io, shown(io, -1))");
    assert_eq!(
        run_fake(src, &[]).stdout,
        "argument 3\nbody 3\nargument -1\n"
    );
}

#[test]
fn passing_contracts_let_the_program_run_to_the_end() {
    let src = format!(
        "{DOUBLE}
fn main(io: Stdout) {{
    let four = double(2)
    let _ = io.println(\"{{four}}\")
}}
"
    );
    assert_runs(&src, &[], "4\n");
}

#[test]
fn a_contract_that_panics_reports_its_own_panic_unchanged() {
    let src = "fn grow(x: Int) -> Int
    requires x * 2 > 0
{
    x
}

fn main() {
    let big = grow(9223372036854775807)
}
";
    assert_panics(src, "P0001", "x * 2");
}

#[test]
fn a_contract_calling_a_function_with_contracts_checks_those_too() {
    let src = "fn half(n: Int) -> Int
    requires n % 2 == 0
{
    n / 2
}

fn quarter(n: Int) -> Int
    requires half(n) % 2 == 0
{
    n / 4
}

fn main() {
    let q = quarter(3)
}
";
    assert_panics(src, "P0006", "half(n)");
    let panic = run_fake(src, &[]).panic.expect("a panic");
    assert_eq!(
        panic.message,
        "precondition violated: `half` requires `n % 2 == 0`"
    );
}

#[test]
fn a_call_through_a_function_value_spans_that_call() {
    let src = format!(
        "{DOUBLE}
fn main() {{
    let f = double
    let y = f(-1)
}}
"
    );
    assert_panics(&src, "P0006", "f(-1)");
}

#[test]
fn a_mutating_method_call_spans_that_call() {
    let src = "struct Counter { count: Int }

impl Counter {
    fn add(var self, n: Int)
        requires n >= 0
    {
        self.count = self.count + n
    }
}

fn main() {
    var counter = Counter(count: 0)
    counter.add(2)
    counter.add(-1)
}
";
    assert_panics(src, "P0006", "counter.add(-1)");
}

#[test]
fn a_parameter_named_underscore_takes_no_slot_in_the_contract() {
    let src = "fn pick(_: Int, n: Int) -> Int
    requires n > 0
{
    n
}

fn main() {
    let ok = pick(-5, 1)
    let bad = pick(5, -1)
}
";
    assert_panics(src, "P0006", "pick(5, -1)");
}

#[test]
fn a_question_mark_or_return_leaving_a_contract_is_refused_before_the_program_starts() {
    let question_mark = "fn maybe(n: Int) -> Option<Int> {
    Some(n)
}

fn f(n: Int) -> Option<Int>
    requires maybe(n)? > 0
{
    Some(n)
}

fn main() {
    let x = f(1)
}
";
    assert_refused(question_mark, "E0380");
    let early_return = "fn g(n: Int) -> Int
    requires { return 1 }
{
    n
}

fn main() {
    let x = g(1)
}
";
    assert_refused(early_return, "E0380");
}

#[test]
fn a_closure_in_a_contract_may_return_from_itself() {
    let src = "fn count(xs: List<Int>) -> Int
    requires xs.map(|x| { return x > 0 }) == [true, true]
{
    xs.len()
}

fn main(io: Stdout) {
    let n = count([1, 2])
    let _ = io.println(\"{n}\")
}
";
    assert_runs(src, &[], "2\n");
}

#[test]
fn under_oskac_test_a_violation_fails_only_the_test_that_hit_it() {
    let src = format!(
        "{DOUBLE}
test \"breaks the contract\" {{
    let y = double(-1)
}}

test \"keeps the contract\" {{
    t.assert_eq(double(2), 4)
}}
"
    );
    let report = test_fake(&src);
    assert!(report.diagnostics.is_empty(), "{report:#?}");
    let statuses: Vec<(&str, TestStatus)> = report
        .tests
        .iter()
        .map(|t| (t.name.as_str(), t.status))
        .collect();
    assert_eq!(
        statuses,
        [
            ("breaks the contract", TestStatus::Failed),
            ("keeps the contract", TestStatus::Passed),
        ]
    );
    let failure = report.tests[0].failure.as_ref().expect("a failure");
    assert_eq!(failure.code, "P0006");
    assert_eq!(&src[failure.span.start..failure.span.end], "double(-1)");
}
