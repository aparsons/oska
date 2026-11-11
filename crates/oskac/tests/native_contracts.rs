//! Native contracts (ticket #134): `requires` and `ensures` evaluated on
//! every call as the interpreter evaluates them, run on both backends
//! through the differential harness (seam 1). The programs are the
//! interpreter's contract suites' (`oska-run/tests/preconditions.rs`,
//! `postconditions.rs`, `trait_contracts.rs`, `contract_review_fixes.rs`)
//! that need nothing the native backend lacks; the rest are refused
//! cleanly (see the last test).

#![cfg(all(target_arch = "aarch64", target_os = "linux"))]

mod differential;

use differential::{assert_same, Case};

/// Run `src` on both backends and require the same outcome, a clean run
/// that printed `stdout`.
fn assert_prints(name: &str, src: &str, stdout: &str) {
    let outcome = assert_same(&Case::file(name, src));
    assert_eq!(outcome.stdout, stdout, "case `{name}`: {outcome:#?}");
    assert_eq!(outcome.exit_code, Some(0), "case `{name}`: {outcome:#?}");
}

/// Run `src` on both backends and require the same outcome, a panic with
/// `code` whose report contains `message` and whose stdout is `stdout`.
fn assert_panics(name: &str, src: &str, code: &str, message: &str, stdout: &str) {
    let outcome = assert_same(&Case::file(name, src));
    assert_eq!(outcome.exit_code, Some(101), "case `{name}`: {outcome:#?}");
    assert!(
        outcome
            .stderr
            .contains(&format!("error[{code}]: {message}")),
        "case `{name}`: {outcome:#?}"
    );
    assert_eq!(outcome.stdout, stdout, "case `{name}`: {outcome:#?}");
}

const DOUBLE: &str = "fn double(x: Int) -> Int
    requires x > 0
{
    x * 2
}
";

const FORGETFUL: &str = "fn absolute(x: Int) -> Int
    ensures |r| r >= 0
{
    x
}
";

// ---- requires -----------------------------------------------------------------

#[test]
fn a_false_requires_panics_with_p0006() {
    let src = format!("{DOUBLE}\nfn main() {{\n    let y = double(-1)\n}}\n");
    assert_panics(
        "requires_false",
        &src,
        "P0006",
        "precondition violated: `double` requires `x > 0`",
        "",
    );
}

#[test]
fn passing_contracts_let_the_program_run() {
    let src = format!(
        "{DOUBLE}\nfn main(io: Stdout) {{\n    let four = double(2)\n    let _ = io.println(\"{{four}}\")\n}}\n"
    );
    assert_prints("requires_passes", &src, "4\n");
}

#[test]
fn the_first_false_clause_in_the_order_written_is_quoted() {
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
    assert_panics(
        "requires_order",
        src,
        "P0006",
        "precondition violated: `Account.withdraw` requires `amount <= self.balance`",
        "",
    );
}

#[test]
fn arguments_are_evaluated_first_and_the_body_runs_only_when_every_clause_holds() {
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
    assert_panics(
        "requires_order_of_evaluation",
        src,
        "P0006",
        "precondition violated: `report` requires `n > 0`",
        "argument 3\nbody 3\nargument -1\n",
    );
}

#[test]
fn a_contract_that_panics_reports_its_own_panic() {
    let src = "fn grow(x: Int) -> Int
    requires x * 2 > 0
{
    x
}

fn main() {
    let big = grow(9223372036854775807)
}
";
    assert_panics("requires_overflow", src, "P0001", "", "");
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
    assert_panics(
        "requires_nested",
        src,
        "P0006",
        "precondition violated: `half` requires `n % 2 == 0`",
        "",
    );
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

fn main(io: Stdout) {
    var counter = Counter(count: 0)
    counter.add(2)
    let _ = io.println(\"{counter.count}\")
    counter.add(-1)
}
";
    assert_panics(
        "requires_mutating",
        src,
        "P0006",
        "precondition violated: `Counter.add` requires `n >= 0`",
        "2\n",
    );
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
    assert_panics(
        "requires_underscore",
        src,
        "P0006",
        "precondition violated: `pick` requires `n > 0`",
        "",
    );
}

#[test]
fn requires_sees_text_and_struct_arguments() {
    let src = "struct Range2 { lo: Int, hi: Int }

fn width(r: Range2, label: String) -> Int
    requires r.lo <= r.hi
    requires label.len() > 0
{
    r.hi - r.lo
}

fn main(io: Stdout) {
    let a = width(Range2(lo: 1, hi: 4), \"a\")
    let _ = io.println(\"{a}\")
    let b = width(Range2(lo: 1, hi: 4), \"\")
}
";
    assert_panics(
        "requires_values",
        src,
        "P0006",
        "precondition violated: `width` requires `label.len() > 0`",
        "3\n",
    );
}

// ---- ensures --------------------------------------------------------------------

#[test]
fn a_false_ensures_panics_with_p0007_naming_the_return() {
    let src = format!("{FORGETFUL}\nfn main() {{\n    let y = absolute(-1)\n}}\n");
    assert_panics(
        "ensures_false",
        &src,
        "P0007",
        "postcondition violated: `absolute` ensures `r >= 0`, \
         broken by the return at line 4, column 5",
        "",
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
    assert_panics(
        "ensures_unit_body",
        src,
        "P0007",
        "postcondition violated: `note` ensures `false`, \
         broken by the return at line 6, column 1",
        "",
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
    assert_panics(
        "ensures_early_return",
        src,
        "P0007",
        "postcondition violated: `clamp` ensures `r >= 0`, \
         broken by the return at line 5, column 9",
        "",
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
    assert_prints("ensures_early_ok", src, "0 4\n");
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
    assert_prints("ensures_bare_return", src, "done\n");
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
    assert_panics(
        "ensures_order",
        src,
        "P0007",
        "postcondition violated: `f` ensures `r > 100`, broken by the return at line 6, column 5",
        "",
    );
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
    assert_panics(
        "ensures_parameters",
        src,
        "P0007",
        "postcondition violated: `pick` ensures `n > 0`",
        "1\n",
    );
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
    assert_prints("ensures_no_binding", src, "1\n");
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
    assert_panics(
        "ensures_after_body",
        src,
        "P0007",
        "postcondition violated: `shown` ensures `r > 10`",
        "body 20\nbody 3\n",
    );
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

fn main(io: Stdout) {
    var counter = Counter(count: 0)
    counter.add(2)
    let _ = io.println(\"{counter.count}\")
    counter.add(-5)
}
";
    assert_panics(
        "ensures_receiver",
        src,
        "P0007",
        "postcondition violated: `Counter.add` ensures `self.count > 0`",
        "2\n",
    );
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
    assert_panics(
        "ensures_method_result",
        src,
        "P0007",
        "postcondition violated: `Account.after` ensures `r >= 0`, \
         broken by the return at line 7, column 9",
        "",
    );
}

#[test]
fn r_may_be_a_struct_or_text() {
    let src = "struct Pair { a: Int, b: Int }

fn ordered(a: Int, b: Int) -> Pair
    ensures |r| r.a <= r.b
{
    Pair(a: a, b: b)
}

fn label(n: Int) -> String
    ensures |r| r.len() > 1
{
    \"{n}\"
}

fn main(io: Stdout) {
    let p = ordered(1, 2)
    let _ = io.println(\"{p.a}{p.b}\")
    let s = label(12)
    let _ = io.println(s)
    let t = label(5)
}
";
    assert_panics(
        "ensures_counted_result",
        src,
        "P0007",
        "postcondition violated: `label` ensures `r.len() > 1`",
        "12\n12\n",
    );
}

#[test]
fn a_return_from_inside_a_loop_with_locals_is_checked() {
    let src = "fn find(limit: Int, name: String) -> Int
    ensures |r| r < 5
{
    let label = \"{name}!\"
    var i = 0
    while i < 100 {
        let tag = \"{label}{i}\"
        if i == limit {
            return i
        }
        i = i + 1
    }
    0
}

fn main(io: Stdout) {
    let a = find(3, \"x\")
    let _ = io.println(\"{a}\")
    let b = find(7, \"x\")
}
";
    assert_panics(
        "ensures_in_loop",
        src,
        "P0007",
        "postcondition violated: `find` ensures `r < 5`, broken by the return at line 9, column 13",
        "3\n",
    );
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
    assert_panics(
        "ensures_recursion",
        src,
        "P0007",
        "postcondition violated: `countdown` ensures `r == 0`, \
         broken by the return at line 4, column 5",
        "",
    );
}

#[test]
fn a_contract_calling_a_function_with_contracts_checks_those_too_after_the_body() {
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
    assert_panics(
        "ensures_nested",
        src,
        "P0007",
        "postcondition violated: `shrink` ensures `r < n`, \
         broken by the return at line 4, column 5",
        "",
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
    assert_panics(
        "ensures_breaks_requires",
        src,
        "P0006",
        "precondition violated: `half` requires `n % 2 == 0`",
        "",
    );
}

#[test]
fn an_ensures_that_panics_reports_its_own_panic_unchanged() {
    let src = "fn grow(x: Int) -> Int
    ensures |r| r * 2 > 0
{
    x
}

fn main() {
    let big = grow(9223372036854775807)
}
";
    assert_panics("ensures_overflow", src, "P0001", "", "");
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
    assert_panics("ensures_body_panics", src, "P0004", "", "");
}

#[test]
fn the_column_of_the_breaking_return_counts_characters() {
    let src = "fn f(s: String) -> Int
    ensures |r| r > 0
{
    if s == \"ééééé\" { return -1 }
    1
}
fn main(out: Stdout) {
    let n = f(\"ééééé\")
}
";
    assert_panics(
        "ensures_column",
        src,
        "P0007",
        "postcondition violated: `f` ensures `r > 0`, broken by the return at line 4, column 23",
        "",
    );
}

// ---- trait contracts -----------------------------------------------------------------

/// `Shape`, whose `scale` needs a positive factor and whose result must
/// not shrink, with two impls: `Square` honours the trait and `Cheat`
/// breaks it. The caller's `main` is appended.
fn shapes(main: &str) -> String {
    format!(
        "trait Shape {{
    fn scale(self, k: Int) -> Int
        requires k > 0
        ensures |r| r >= k
    fn size(self) -> Int
}}

struct Square {{ side: Int }}
struct Cheat {{ side: Int }}

impl Shape for Square {{
    fn scale(self, factor: Int) -> Int {{ self.side * factor }}
    fn size(self) -> Int {{ self.side }}
}}

impl Shape for Cheat {{
    fn scale(self, _: Int) -> Int {{ 0 }}
    fn size(self) -> Int {{ self.side }}
}}

{main}
"
    )
}

#[test]
fn a_direct_method_call_checks_the_traits_requires() {
    let src = shapes("fn main() {\n    let n = Square(side: 2).scale(-1)\n}");
    assert_panics(
        "trait_requires_direct",
        &src,
        "P0006",
        "precondition violated: `<Square as Shape>.scale` requires `k > 0`",
        "",
    );
}

#[test]
fn an_impl_that_names_a_parameter_differently_is_still_checked() {
    let src = shapes("fn main() {\n    let n = Cheat(side: 2).scale(-3)\n}");
    assert_panics(
        "trait_requires_renamed",
        &src,
        "P0006",
        "precondition violated: `<Cheat as Shape>.scale` requires `k > 0`",
        "",
    );
}

#[test]
fn a_direct_method_call_checks_the_traits_ensures() {
    let src = shapes("fn main() {\n    let n = Cheat(side: 2).scale(3)\n}");
    assert_panics(
        "trait_ensures_direct",
        &src,
        "P0007",
        "postcondition violated: `<Cheat as Shape>.scale` ensures `r >= k`, \
         broken by the return at line 17, column 37",
        "",
    );
}

#[test]
fn an_impl_that_honours_the_contract_runs() {
    let src = shapes(
        "fn main(io: Stdout) {
    let b = Square(side: 2).scale(4)
    let _ = io.println(\"{b}\")
}",
    );
    assert_prints("trait_honoured", &src, "8\n");
}

#[test]
fn a_default_method_keeps_its_contract_in_every_impl_that_inherits_it() {
    let src = "trait Halve {
    fn half(self, n: Int) -> Int
        requires n % 2 == 0
    { n / 2 }
}

struct A { x: Int }
struct B { x: Int }
impl Halve for A {}
impl Halve for B {}

fn main() {
    let ok = A(x: 0).half(4)
    let bad = B(x: 0).half(3)
}
";
    assert_panics(
        "trait_default",
        src,
        "P0006",
        "precondition violated: `<B as Halve>.half` requires `n % 2 == 0`",
        "",
    );
}

#[test]
fn an_overriding_impl_of_a_default_method_is_held_to_the_trait_contract() {
    let src = "trait Halve {
    fn half(self, n: Int) -> Int
        requires n % 2 == 0
    { n / 2 }
}

struct A { x: Int }
impl Halve for A {
    fn half(self, n: Int) -> Int { n }
}

fn main() {
    let bad = A(x: 0).half(3)
}
";
    assert_panics(
        "trait_override",
        src,
        "P0006",
        "precondition violated: `<A as Halve>.half` requires `n % 2 == 0`",
        "",
    );
}

#[test]
fn a_trait_method_without_a_contract_is_unchecked() {
    let src = "trait Twice {
    fn twice(self, n: Int) -> Int
}

struct A { x: Int }
impl Twice for A {
    fn twice(self, n: Int) -> Int { n * 2 }
}

fn main(io: Stdout) {
    let n = A(x: 0).twice(-2)
    let _ = io.println(\"{n}\")
}
";
    assert_prints("trait_unchecked", src, "-4\n");
}

#[test]
fn an_inherent_impl_method_keeps_its_contract() {
    let src = "struct Account { balance: Int }

impl Account {
    fn withdraw(self, amount: Int) -> Int
        requires amount <= self.balance
    {
        self.balance - amount
    }
}

fn main() {
    let left = Account(balance: 5).withdraw(10)
}
";
    assert_panics(
        "inherent_requires",
        src,
        "P0006",
        "precondition violated: `Account.withdraw` requires `amount <= self.balance`",
        "",
    );
}

#[test]
fn generic_functions_check_their_contracts_per_instance() {
    let src = "fn first<T>(a: T, b: T, n: Int) -> T
    requires n > 0
    ensures |r| n < 100
{
    a
}

fn main(io: Stdout) {
    let x = first(\"s\", \"t\", 1)
    let y = first(4, 5, 2)
    let _ = io.println(\"{x}{y}\")
    let z = first(4, 5, 200)
}
";
    assert_panics(
        "generic_contracts",
        src,
        "P0007",
        "postcondition violated: `first` ensures `n < 100`",
        "s4\n",
    );
}

// ---- contracts through traits and `?` (#133) -----------------------------------------

#[test]
fn a_call_through_a_trait_bound_checks_the_impls_requires() {
    let src = shapes(
        "fn through<S: Shape>(s: S, k: Int) -> Int {
    s.scale(k)
}

fn main(io: Stdout) {
    let a = through(Square(side: 2), 3)
    let _ = io.println(\"{a}\")
    let b = through(Square(side: 2), 0)
}",
    );
    assert_panics(
        "trait_requires_through_bound",
        &src,
        "P0006",
        "precondition violated: `<Square as Shape>.scale` requires `k > 0`",
        "6\n",
    );
}

#[test]
fn a_call_through_a_trait_bound_checks_the_impls_ensures() {
    let src = shapes(
        "fn through<S: Shape>(s: S, k: Int) -> Int {
    s.scale(k)
}

fn main() {
    let n = through(Cheat(side: 2), 3)
}",
    );
    assert_panics(
        "trait_ensures_through_bound",
        &src,
        "P0007",
        "postcondition violated: `<Cheat as Shape>.scale` ensures `r >= k`",
        "",
    );
}

#[test]
fn a_qualified_trait_call_checks_the_impls_requires() {
    let src = shapes("fn main() {\n    let n = Shape.scale(Square(side: 2), 0)\n}");
    assert_panics(
        "trait_requires_qualified",
        &src,
        "P0006",
        "precondition violated: `<Square as Shape>.scale` requires `k > 0`",
        "",
    );
}

#[test]
fn a_clause_may_call_a_trait_method() {
    let src = "trait Sized {
    fn size(self) -> Int
    fn grow(self, k: Int) -> Int
        ensures |r| r >= self.size()
}

struct Good { n: Int }
struct Bad { n: Int }

impl Sized for Good {
    fn size(self) -> Int { self.n }
    fn grow(self, k: Int) -> Int { self.n + k }
}

impl Sized for Bad {
    fn size(self) -> Int { self.n }
    fn grow(self, k: Int) -> Int { self.n - k }
}

fn main(io: Stdout) {
    let a = Good(n: 2).grow(3)
    let _ = io.println(\"{a}\")
    let b = Bad(n: 2).grow(3)
}
";
    assert_panics(
        "clause_calls_trait_method",
        src,
        "P0007",
        "postcondition violated: `<Bad as Sized>.grow` ensures `r >= self.size()`",
        "5\n",
    );
}

#[test]
fn an_early_return_through_question_mark_is_checked_by_ensures() {
    let src = "fn parse(text: String) -> Result<Int, String> {
    match text.parse_int() {
        Some(n) => Ok(n)
        None => Err(\"bad {text}\")
    }
}

fn double(text: String) -> Result<Int, String>
    ensures |r| r.is_ok()
{
    let n = parse(text)?
    Ok(n * 2)
}

fn main(io: Stdout) {
    let a = double(\"4\").unwrap_or(0)
    let _ = io.println(\"{a}\")
    let r = double(\"x\")
}
";
    assert_panics(
        "question_mark_ensures",
        src,
        "P0007",
        "postcondition violated: `double` ensures `r.is_ok()`, \
         broken by the return at line 11, column 13",
        "8\n",
    );
}

// ---- what is not native yet ---------------------------------------------------------

/// Contract programs in the interpreter's suites that need a feature of a
/// later ticket are refused with `E0390` for that feature, not for the
/// contract, and exit 2.
#[test]
fn contract_programs_that_need_later_features_are_refused_cleanly() {
    let cases = [
        (
            "closure_in_contract",
            "fn count(xs: List<Int>) -> Int\n    requires xs.map(|x| { return x > 0 }) == [true, true]\n{\n    xs.len()\n}\n\nfn main(io: Stdout) {\n    let n = count([1, 2])\n}\n",
        ),
    ];
    for (name, src) in cases {
        let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("native_contracts_refusals")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a directory");
        std::fs::write(dir.join("main.osk"), src).expect("the source");
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_oskac"))
            .args(["build", "main.osk"])
            .current_dir(&dir)
            .output()
            .expect("oskac should run");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "{name}: {stderr}");
        assert!(stderr.contains("error[E0390]"), "{name}: {stderr}");
        assert!(
            !stderr.contains("`requires` or `ensures`"),
            "{name} must be refused for its feature, not its contract: {stderr}"
        );
    }
}
