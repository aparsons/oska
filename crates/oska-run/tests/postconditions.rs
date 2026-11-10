//! Postconditions enforced on every return path (#107, spec #105): a
//! function's `ensures` clauses are evaluated whenever it returns, in every
//! build, with `r` bound to the value being returned, and a false one stops
//! the program with `P0007`, "postcondition violated", spanning the clause
//! and naming the return that broke it.

mod common;

use common::{assert_panics, assert_runs, run_fake, test_fake};
use oska_run::TestStatus;

/// `absolute`, whose result must not be negative, with a body that forgets
/// the negative case when it falls off the end.
const FORGETFUL: &str = "fn absolute(x: Int) -> Int
    ensures |r| r >= 0
{
    x
}
";

/// The message of the panic `src` stops with.
fn panic_message(src: &str) -> String {
    run_fake(src, &[]).panic.expect("a panic").message
}

#[test]
fn a_false_ensures_panics_with_p0007_spanning_the_clause() {
    let src = format!("{FORGETFUL}\nfn main() {{\n    let y = absolute(-1)\n}}\n");
    assert_panics(&src, "P0007", "ensures |r| r >= 0");
}

#[test]
fn falling_off_the_end_names_the_value_the_body_ends_with() {
    let src = format!("{FORGETFUL}\nfn main() {{\n    let y = absolute(-1)\n}}\n");
    assert_eq!(
        panic_message(&src),
        "postcondition violated: `absolute` ensures `r >= 0`, \
         broken by the return at line 4, column 5"
    );
}

#[test]
fn a_body_that_ends_with_a_statement_names_its_closing_brace() {
    let src = "fn note(x: Int)
    ensures |r| r == ()
    ensures |r| false
{
    let y = x
}

fn main() {
    note(0)
}
";
    assert_panics(src, "P0007", "ensures |r| false");
    assert_eq!(
        panic_message(src),
        "postcondition violated: `note` ensures `false`, \
         broken by the return at line 6, column 1"
    );
}

#[test]
fn an_early_return_is_checked_and_named() {
    let src = "fn clamp(x: Int) -> Int
    ensures |r| r >= 0
{
    if x < 0 {
        return x
    }
    x
}

fn main() {
    let y = clamp(-3)
}
";
    assert_panics(src, "P0007", "ensures |r| r >= 0");
    assert_eq!(
        panic_message(src),
        "postcondition violated: `clamp` ensures `r >= 0`, \
         broken by the return at line 5, column 9"
    );
}

#[test]
fn an_early_return_that_keeps_the_promise_passes() {
    let src = "fn clamp(x: Int) -> Int
    ensures |r| r >= 0
{
    if x < 0 {
        return 0
    }
    x
}

fn main(io: Stdout) {
    let low = clamp(-3)
    let high = clamp(4)
    let _ = io.println(\"{low} {high}\")
}
";
    assert_runs(src, &[], "0 4\n");
}

#[test]
fn a_bare_return_binds_r_to_unit() {
    let src = "fn stop(x: Int)
    ensures |r| r == ()
{
    return
}

fn main(io: Stdout) {
    stop(1)
    let _ = io.println(\"done\")
}
";
    assert_runs(src, &[], "done\n");
}

#[test]
fn question_mark_checks_the_err_it_returns() {
    let src = "fn parse(text: String) -> Result<Int, String> {
    Err(\"bad {text}\")
}

fn double(text: String) -> Result<Int, String>
    ensures |r| r.is_ok()
{
    let n = parse(text)?
    Ok(n * 2)
}

fn main() {
    let r = double(\"x\")
}
";
    assert_panics(src, "P0007", "ensures |r| r.is_ok()");
    assert_eq!(
        panic_message(src),
        "postcondition violated: `double` ensures `r.is_ok()`, \
         broken by the return at line 8, column 13"
    );
}

#[test]
fn r_is_the_err_a_question_mark_returns_so_a_clause_can_state_it() {
    let src = "fn parse(text: String) -> Result<Int, String> {
    Err(\"bad {text}\")
}

fn double(text: String) -> Result<Int, String>
    ensures |r| r.is_ok() || r == Err(\"bad {text}\")
{
    let n = parse(text)?
    Ok(n * 2)
}

fn main(io: Stdout) {
    let r = double(\"x\")
    if r.is_ok() {
        let _ = io.println(\"ok\")
    } else {
        let _ = io.println(\"err\")
    }
}
";
    assert_runs(src, &[], "err\n");
}

#[test]
fn question_mark_on_a_none_returns_none_to_the_clauses() {
    let src = "fn first(xs: List<Int>) -> Option<Int> {
    xs.get(0)
}

fn twice(xs: List<Int>) -> Option<Int>
    ensures |r| r.is_some()
{
    let n = first(xs)?
    Some(n * 2)
}

fn main() {
    let r = twice([])
}
";
    assert_panics(src, "P0007", "ensures |r| r.is_some()");
}

#[test]
fn a_question_mark_that_continues_does_not_return_early() {
    let src = "fn parse(text: String) -> Result<Int, String> {
    Ok(text.len())
}

fn double(text: String) -> Result<Int, String>
    ensures |r| r.is_ok()
{
    let n = parse(text)?
    Ok(n * 2)
}

fn main(io: Stdout) {
    let r = double(\"abc\")
    if r.is_ok() {
        let _ = io.println(\"ok\")
    } else {
        let _ = io.println(\"err\")
    }
}
";
    assert_runs(src, &[], "ok\n");
}

#[test]
fn the_clauses_run_in_order_and_the_first_false_one_is_reported() {
    let src = "fn f(x: Int) -> Int
    ensures |r| r > 0
    ensures |r| r > 100
    ensures |r| r * 9223372036854775807 > 0
{
    x
}

fn main() {
    let y = f(5)
}
";
    assert_panics(src, "P0007", "ensures |r| r > 100");
}

#[test]
fn a_clause_sees_the_parameters_and_may_ignore_the_binding() {
    let src = "fn pick(_: Int, n: Int) -> Int
    ensures |_| n > 0
    ensures |r| r == n
{
    n
}

fn main(io: Stdout) {
    let first = pick(-5, 1)
    let _ = io.println(\"{first}\")
    let bad = pick(5, -1)
}
";
    assert_panics(src, "P0007", "ensures |_| n > 0");
}

#[test]
fn a_clause_that_ignores_the_binding_still_sees_the_parameters_after_it() {
    let src = "fn pick(a: Int, b: Int) -> Int
    ensures |_| b > a
{
    a
}

fn main(io: Stdout) {
    let first = pick(1, 2)
    let _ = io.println(\"{first}\")
}
";
    assert_runs(src, &[], "1\n");
}

#[test]
fn requires_is_checked_before_the_body_and_ensures_after_it() {
    let src = "fn shown(io: Stdout, n: Int) -> Int
    requires n > 0
    ensures |r| r > 10
{
    let _ = io.println(\"body {n}\")
    n
}

fn main(io: Stdout) {
    let _ = shown(io, 20)
    let _ = shown(io, 3)
    let _ = io.println(\"unreachable\")
}
";
    assert_panics(src, "P0007", "ensures |r| r > 10");
    assert_eq!(run_fake(src, &[]).stdout, "body 20\nbody 3\n");
}

#[test]
fn a_method_checks_its_ensures_against_the_receiver_it_ends_with() {
    let src = "struct Counter { count: Int }

impl Counter {
    fn add(var self, n: Int)
        ensures |_| self.count > 0
    {
        self.count = self.count + n
    }
}

fn main() {
    var counter = Counter(count: 0)
    counter.add(2)
    counter.add(-5)
}
";
    assert_panics(src, "P0007", "ensures |_| self.count > 0");
}

#[test]
fn a_method_binds_r_to_what_it_returns() {
    let src = "struct Account { balance: Int }

impl Account {
    fn after(self, amount: Int) -> Int
        ensures |r| r >= 0
    {
        self.balance - amount
    }
}

fn main() {
    let account = Account(balance: 10)
    let left = account.after(25)
}
";
    assert_panics(src, "P0007", "ensures |r| r >= 0");
    assert_eq!(
        panic_message(src),
        "postcondition violated: `Account.after` ensures `r >= 0`, \
         broken by the return at line 7, column 9"
    );
}

#[test]
fn a_return_inside_a_closure_leaves_the_closure_not_the_function() {
    let src = "fn positives(xs: List<Int>) -> List<Int>
    ensures |r| r.len() == xs.len()
{
    xs.map(|x| {
        if x < 0 {
            return 0 - x
        }
        x
    })
}

fn main(io: Stdout) {
    let xs = positives([1, -2])
    let n = xs.len()
    let _ = io.println(\"{n}\")
}
";
    assert_runs(src, &[], "2\n");
}

#[test]
fn every_level_of_a_recursion_is_checked() {
    let src = "fn countdown(n: Int) -> Int
    ensures |r| r == 0
{
    if n == 0 {
        0
    } else if n == 2 {
        7
    } else {
        countdown(n - 1)
    }
}

fn main() {
    let y = countdown(3)
}
";
    assert_panics(src, "P0007", "ensures |r| r == 0");
    assert_eq!(
        panic_message(src),
        "postcondition violated: `countdown` ensures `r == 0`, \
         broken by the return at line 4, column 5"
    );
}

#[test]
fn a_contract_calling_a_function_with_contracts_checks_those_too() {
    let src = "fn shrink(n: Int) -> Int
    ensures |r| r < n
{
    n
}

fn small(n: Int) -> Int
    ensures |r| shrink(r) > 0
{
    n
}

fn main() {
    let y = small(3)
}
";
    assert_panics(src, "P0007", "ensures |r| r < n");
    assert_eq!(
        panic_message(src),
        "postcondition violated: `shrink` ensures `r < n`, \
         broken by the return at line 4, column 5"
    );
}

#[test]
fn a_contract_may_break_a_precondition_of_what_it_calls() {
    let src = "fn half(n: Int) -> Int
    requires n % 2 == 0
{
    n / 2
}

fn odd(n: Int) -> Int
    ensures |r| half(r) > 0
{
    n
}

fn main() {
    let y = odd(3)
}
";
    assert_panics(src, "P0006", "half(r)");
}

#[test]
fn a_contract_that_panics_reports_its_own_panic_unchanged() {
    let src = "fn grow(x: Int) -> Int
    ensures |r| r * 2 > 0
{
    x
}

fn main() {
    let big = grow(9223372036854775807)
}
";
    assert_panics(src, "P0001", "r * 2");
}

#[test]
fn a_body_that_panics_is_not_hidden_by_its_contract() {
    let src = "fn f(x: Int) -> Int
    ensures |r| r > 0
{
    x / 0
}

fn main() {
    let y = f(1)
}
";
    assert_panics(src, "P0004", "x / 0");
}

#[test]
fn under_oskac_test_a_violation_fails_only_the_test_that_hit_it() {
    let src = format!(
        "{FORGETFUL}
test \"breaks the contract\" {{
    let y = absolute(-1)
}}

test \"keeps the contract\" {{
    t.assert_eq(absolute(2), 2)
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
    assert_eq!(failure.code, "P0007");
    assert_eq!(
        &src[failure.span.start..failure.span.end],
        "ensures |r| r >= 0"
    );
}
