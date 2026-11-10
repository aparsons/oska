//! Review fixes for spec #105: no checked program performs an effect inside
//! a contract, and no capability or `Scope` reaches a plain `fn` or an
//! unscoped closure through a type parameter.
//!
//! - A contract's `==`, `!=` and interpolation dispatch to `Eq` and
//!   `Display` impls, so they are judged as calls are (`E0280`).
//! - A type parameter whose purity a body assumes, by using a value of it
//!   in a contract or by capturing one in a plain `fn` closure, cannot be
//!   instantiated with a capability-bearing type; one captured by an
//!   unscoped closure cannot be instantiated with a type that holds a
//!   `Scope` (`E0274`). That covers a trait's `Self` too: a capability-
//!   bearing type cannot implement a trait whose contracts use `self`.
//! - A closure decided unscoped before its captures' types were known is
//!   rejected if one of them turns out to hold a `Scope` (`E0292`).

mod common;

use common::{assert_errors, assert_ok, check};

/// A `cap` struct whose `Eq` and `Display` impls print.
const LOUD: &str = "cap struct L { out: Stdout }

impl Eq for L {
    fn eq(self, other: L) -> Bool {
        let _ = self.out.println(\"effect\")
        true
    }
}

impl Display for L {
    fn to_string(self) -> String {
        let _ = self.out.println(\"effect\")
        \"l\"
    }
}
";

/// `LOUD`, then `rest`.
fn loud(rest: &str) -> String {
    format!("{LOUD}\n{rest}")
}

// ---- Operators and interpolation in contracts (G1) ---------------------------

#[test]
fn equality_on_a_capability_bearing_value_in_a_contract_is_impure() {
    let src = loud("fn f(x: L) -> Int\n    requires x == x\n{\n    1\n}\n");
    assert_errors(&src, &[("E0280", "x == x")]);
}

#[test]
fn inequality_on_a_capability_bearing_value_in_a_contract_is_impure() {
    let src = loud("fn f(x: L) -> Int\n    ensures |r| x != x\n{\n    1\n}\n");
    assert_errors(&src, &[("E0280", "x != x")]);
}

#[test]
fn equality_on_a_container_of_a_capability_in_a_contract_is_impure() {
    let src = loud("fn f(x: Option<L>) -> Int\n    requires x == x\n{\n    1\n}\n");
    assert_errors(&src, &[("E0280", "x == x")]);
}

#[test]
fn interpolating_a_capability_bearing_value_in_a_contract_is_impure() {
    let src = loud("fn f(x: L) -> Int\n    requires \"{x}\".len() > 0\n{\n    1\n}\n");
    assert_errors(&src, &[("E0280", "{x}")]);
}

#[test]
fn a_value_that_turns_out_capability_bearing_after_the_comparison_is_caught() {
    // `xs` has no element type yet where it is compared; the loop's next
    // pass compares a list holding `y`.
    let src = loud(
        "fn f(y: L) -> Int
    requires {
        var xs = []
        var same = true
        for i in 0..2 {
            same = xs == xs
            xs = [y]
        }
        same
    }
{
    1
}
",
    );
    assert_errors(&src, &[("E0280", "xs == xs")]);
}

#[test]
fn equality_and_interpolation_of_plain_data_in_a_contract_are_fine() {
    assert_ok(
        "struct P { n: Int }

impl Display for P {
    fn to_string(self) -> String {
        \"p\"
    }
}

fn f(x: P, s: String) -> Int
    requires x.n == 1 && s != \"\" && \"{x}\".len() > 0
{
    1
}
",
    );
}

// ---- Type parameters used in contracts (G2) ----------------------------------

/// A trait whose impl for `Stdout` prints, and one for `Int` that is pure.
const SPEAK: &str = "trait Speak {
    fn ok(self) -> Bool
}

impl Speak for Stdout {
    fn ok(self) -> Bool {
        let _ = self.println(\"effect\")
        true
    }
}

impl Speak for Int {
    fn ok(self) -> Bool {
        self > 0
    }
}
";

/// `SPEAK`, then `rest`.
fn speak(rest: &str) -> String {
    format!("{SPEAK}\n{rest}")
}

/// The messages of the diagnostics `src` reports with `code`.
fn messages(src: &str, code: &str) -> Vec<String> {
    check(src)
        .diagnostics
        .into_iter()
        .filter(|d| d.code == code)
        .map(|d| d.message)
        .collect()
}

/// `check`, whose contract calls a method on its `T`.
const CHECK: &str = "fn check<T: Speak>(x: T) -> Int\n    requires x.ok()\n{\n    1\n}\n";

#[test]
fn a_generic_whose_contract_uses_its_parameter_rejects_a_capability() {
    let src = speak(&format!(
        "{CHECK}\nfn main(out: Stdout) {{\n    let n = check(out)\n}}\n"
    ));
    assert_errors(&src, &[("E0274", "check")]);
}

#[test]
fn the_message_names_the_type_the_parameter_and_why() {
    let src = speak(&format!(
        "{CHECK}\nfn main(out: Stdout) {{\n    let n = check(out)\n}}\n"
    ));
    let messages = messages(&src, "E0274");
    let [message] = messages.as_slice() else {
        panic!("one E0274: {messages:#?}");
    };
    assert!(message.contains("`Stdout`"), "{message}");
    assert!(message.contains("`T` of `check`"), "{message}");
    assert!(message.contains("contract"), "{message}");
}

#[test]
fn a_generic_whose_contract_uses_its_parameter_accepts_pure_data() {
    assert_ok(&speak(&format!(
        "{CHECK}\nfn main(out: Stdout) {{\n    let n = check(5)\n}}\n"
    )));
}

#[test]
fn a_generic_whose_contract_ignores_its_parameter_accepts_a_capability() {
    assert_ok(
        "fn keep<T>(x: T, n: Int) -> T\n    requires n > 0\n{\n    x\n}\n\
         fn main(out: Stdout) {\n    let o = keep(out, 1)\n}\n",
    );
}

#[test]
fn the_requirement_passes_through_a_generic_caller() {
    let src = speak(&format!(
        "{CHECK}\nfn outer<U: Speak>(x: U) -> Int {{\n    check(x)\n}}\n\
         fn main(out: Stdout) {{\n    let n = outer(out)\n}}\n"
    ));
    assert_errors(&src, &[("E0274", "outer")]);
}

#[test]
fn the_requirement_passes_through_a_caller_declared_first() {
    let src = speak(&format!(
        "fn outer<U: Speak>(x: U) -> Int {{\n    check(x)\n}}\n{CHECK}\n\
         fn main(out: Stdout) {{\n    let n = outer(out)\n}}\n"
    ));
    assert_errors(&src, &[("E0274", "outer")]);
}

#[test]
fn the_requirement_survives_recursion() {
    let src = speak(
        "fn f<T: Speak>(x: T, n: Int) -> Int\n    requires x.ok()\n{\n    \
         if n == 0 { 0 } else { f(x, n - 1) }\n}\n\
         fn main(out: Stdout) {\n    let n = f(out, 1)\n}\n",
    );
    assert_errors(&src, &[("E0274", "f")]);
}

#[test]
fn a_parameter_inside_a_generic_struct_is_covered() {
    let src = speak(
        "struct Box<T> { x: T }\n\
         fn f<T: Speak>(b: Box<T>) -> Int\n    requires b.x.ok()\n{\n    1\n}\n\
         fn main(out: Stdout) {\n    let n = f(Box(x: out))\n}\n",
    );
    assert_errors(&src, &[("E0274", "f")]);
}

#[test]
fn a_parameter_inside_an_option_in_a_contract_is_covered() {
    let src = speak(
        "fn f<T: Speak>(o: Option<T>) -> Int\n    \
         requires match o {\n        Some(x) => x.ok()\n        None => true\n    }\n{\n    1\n}\n\
         fn main(out: Stdout) {\n    let n = f(Some(out))\n}\n",
    );
    assert_errors(&src, &[("E0274", "f")]);
}

#[test]
fn a_list_of_the_parameter_in_a_contract_is_covered() {
    // As `xs.len()` on a `List<Stdout>` would be in a concrete contract.
    let src = "fn f<T>(xs: List<T>) -> Int\n    requires xs.len() > 0\n{\n    1\n}\n\
               fn main(out: Stdout) {\n    let n = f([out])\n}\n";
    assert_errors(src, &[("E0274", "f")]);
}

#[test]
fn a_method_value_in_a_contract_is_covered() {
    let src = speak(
        "fn f<T: Speak>(xs: List<T>) -> Int\n    requires xs.map(Speak.ok).len() > 0\n{\n    1\n}\n\
         fn main(out: Stdout) {\n    let n = f([out])\n}\n",
    );
    assert_errors(&src, &[("E0274", "f")]);
}

#[test]
fn equality_on_a_parameter_in_a_contract_is_covered() {
    let src = loud(
        "fn f<T: Eq>(x: T) -> Int\n    requires x == x\n{\n    1\n}\n\
         fn main(out: Stdout) {\n    let n = f(L(out: out))\n}\n",
    );
    assert_errors(&src, &[("E0274", "f")]);
}

#[test]
fn interpolating_a_parameter_in_a_contract_is_covered() {
    let src = loud(
        "fn f<T: Display>(x: T) -> Int\n    ensures |r| \"{x}\".len() > 0\n{\n    1\n}\n\
         fn main(out: Stdout) {\n    let n = f(L(out: out))\n}\n",
    );
    assert_errors(&src, &[("E0274", "f")]);
}

#[test]
fn a_closure_inside_a_contract_is_covered() {
    let src = speak(
        "fn f<T: Speak>(x: T) -> Int\n    requires (|| x.ok())()\n{\n    1\n}\n\
         fn main(out: Stdout) {\n    let n = f(out)\n}\n",
    );
    assert_errors(&src, &[("E0274", "f")]);
}

#[test]
fn a_generic_methods_own_parameter_is_covered() {
    let src = speak(
        "struct P { n: Int }\n\
         impl P {\n    fn m<T: Speak>(self, x: T) -> Int\n        requires x.ok()\n    {\n        1\n    }\n}\n\
         fn main(out: Stdout) {\n    let n = P(n: 1).m(out)\n}\n",
    );
    assert_errors(&src, &[("E0274", "m")]);
}

#[test]
fn a_generic_impls_parameter_is_covered() {
    let src = speak(
        "struct W<T> { x: T }\n\
         impl<T: Speak> W<T> {\n    fn m(self) -> Int\n        requires self.x.ok()\n    {\n        1\n    }\n}\n\
         fn main(out: Stdout) {\n    let n = W(x: out).m()\n}\n",
    );
    assert_errors(&src, &[("E0274", "m")]);
}

/// A trait whose contract calls a method on `self`.
const SHAPE: &str = "trait Shape {
    fn scale(self, k: Int) -> Int
        requires self.fine()
    fn fine(self) -> Bool
}
";

#[test]
fn a_capability_cannot_implement_a_trait_whose_contract_uses_self() {
    let src = format!(
        "{SHAPE}\nimpl Shape for Stdout {{\n    fn scale(self, k: Int) -> Int {{ k }}\n    \
         fn fine(self) -> Bool {{\n        let _ = self.println(\"effect\")\n        true\n    }}\n}}\n"
    );
    assert_errors(&src, &[("E0274", "Stdout")]);
}

#[test]
fn pure_data_may_implement_a_trait_whose_contract_uses_self() {
    assert_ok(&format!(
        "{SHAPE}\nimpl Shape for Int {{\n    fn scale(self, k: Int) -> Int {{ k }}\n    \
         fn fine(self) -> Bool {{ true }}\n}}\n"
    ));
}

#[test]
fn a_capability_may_implement_a_trait_whose_contract_ignores_self() {
    assert_ok(
        "trait Grow {\n    fn grow(self, k: Int) -> Int\n        requires k > 0\n}\n\
         impl Grow for Stdout {\n    fn grow(self, k: Int) -> Int { k }\n}\n",
    );
}

/// `SPEAK` and `SHAPE`, with `Shape` implemented for any `W<T>`.
fn wrapped(main: &str) -> String {
    speak(&format!(
        "{SHAPE}\nstruct W<T> {{ x: T }}\n\
         impl<T: Speak> Shape for W<T> {{\n    fn scale(self, k: Int) -> Int {{ k }}\n    \
         fn fine(self) -> Bool {{ self.x.ok() }}\n}}\n\
         fn use_shape<S: Shape>(s: S) -> Int {{\n    s.scale(2)\n}}\n{main}"
    ))
}

#[test]
fn a_generic_impl_of_such_a_trait_rejects_a_capability_through_a_bound() {
    let src = wrapped("fn main(out: Stdout) {\n    let n = use_shape(W(x: out))\n}\n");
    assert_errors(&src, &[("E0274", "use_shape(W(x: out))")]);
}

#[test]
fn a_generic_impl_of_such_a_trait_rejects_a_capability_called_directly() {
    let src = wrapped("fn main(out: Stdout) {\n    let n = W(x: out).scale(2)\n}\n");
    assert_errors(&src, &[("E0274", "scale")]);
}

#[test]
fn a_generic_impl_of_such_a_trait_accepts_pure_data() {
    assert_ok(&wrapped(
        "fn main(out: Stdout) {\n    let n = use_shape(W(x: 5))\n    let m = W(x: 5).scale(2)\n}\n",
    ));
}

// ---- Type parameters captured by plain closures (G3) -------------------------

#[test]
fn a_plain_fn_capturing_a_parameter_rejects_a_capability() {
    let src = speak(
        "fn wrap<T: Speak>(x: T) -> fn() -> Bool {\n    || x.ok()\n}\n\
         fn pure_fn(f: fn() -> Bool) -> Bool {\n    f()\n}\n\
         fn main(out: Stdout) {\n    let g = wrap(out)\n    let b = pure_fn(g)\n}\n",
    );
    assert_errors(&src, &[("E0274", "wrap")]);
    let messages = messages(&src, "E0274");
    assert!(messages[0].contains("plain `fn`"), "{messages:#?}");
}

#[test]
fn a_plain_fn_capturing_a_parameter_accepts_pure_data() {
    assert_ok(&speak(
        "fn wrap<T: Speak>(x: T) -> fn() -> Bool {\n    || x.ok()\n}\n\
         fn main(out: Stdout) {\n    let g = wrap(5)\n}\n",
    ));
}

#[test]
fn a_closure_passed_as_a_plain_fn_rejects_a_capability() {
    let src = speak(
        "fn run(f: fn() -> Bool) -> Bool {\n    f()\n}\n\
         fn g<T: Speak>(x: T) -> Bool {\n    run(|| x.ok())\n}\n\
         fn main(out: Stdout) {\n    let b = g(out)\n}\n",
    );
    assert_errors(&src, &[("E0274", "g")]);
}

#[test]
fn a_closure_passed_to_a_prelude_function_rejects_a_capability() {
    let src = speak(
        "fn g<T: Speak>(x: T, ns: List<Int>) -> List<Bool> {\n    ns.map(|n| x.ok())\n}\n\
         fn main(out: Stdout) {\n    let b = g(out, [1])\n}\n",
    );
    assert_errors(&src, &[("E0274", "g")]);
}

#[test]
fn a_closure_used_only_as_a_cap_fn_accepts_a_capability() {
    assert_ok(&speak(
        "fn run_cap(f: cap fn() -> Bool) -> Bool {\n    f()\n}\n\
         fn g<T: Speak>(x: T) -> Bool {\n    run_cap(|| x.ok())\n}\n\
         fn h<T: Speak>(x: T) -> cap fn() -> Bool {\n    || x.ok()\n}\n\
         fn main(out: Stdout) {\n    let b = g(out)\n    let c = h(out)\n}\n",
    ));
}

#[test]
fn a_closure_inside_a_plain_closure_is_covered() {
    let src = speak(
        "fn wrap<T: Speak>(x: T) -> fn() -> Bool {\n    let a = || x.ok()\n    || a()\n}\n\
         fn main(out: Stdout) {\n    let g = wrap(out)\n}\n",
    );
    assert_errors(&src, &[("E0274", "wrap")]);
}

#[test]
fn a_closure_handed_to_a_generic_that_keeps_it_in_a_plain_fn_is_covered() {
    let src = speak(
        "fn hold<F>(f: F) -> fn() -> F {\n    || f\n}\n\
         fn wrap<T: Speak>(x: T) -> Int {\n    let h = hold(|| x.ok())\n    1\n}\n\
         fn main(out: Stdout) {\n    let n = wrap(out)\n}\n",
    );
    assert_errors(&src, &[("E0274", "wrap")]);
}

#[test]
fn a_parameter_inside_an_option_captured_by_a_plain_fn_is_covered() {
    let src = speak(
        "fn wrap<T: Speak>(o: Option<T>) -> fn() -> Bool {\n    \
         || match o {\n        Some(x) => x.ok()\n        None => true\n    }\n}\n\
         fn main(out: Stdout) {\n    let g = wrap(Some(out))\n}\n",
    );
    assert_errors(&src, &[("E0274", "wrap")]);
}

#[test]
fn a_parameter_captured_before_its_type_is_known_is_covered() {
    let src = speak(
        "fn wrap<T: Speak>(x: T) -> fn() -> Int {\n    var xs = []\n    \
         let g = || xs.len()\n    xs.push(x)\n    g\n}\n\
         fn main(out: Stdout) {\n    let g = wrap(out)\n}\n",
    );
    assert_errors(&src, &[("E0274", "wrap")]);
}

#[test]
fn a_trait_default_method_capturing_self_in_a_plain_fn_is_covered() {
    let src = "trait Later {\n    fn ok(self) -> Bool\n    \
               fn later(self) -> fn() -> Bool {\n        || self.ok()\n    }\n}\n\
               impl Later for Stdout {\n    fn ok(self) -> Bool { true }\n}\n";
    assert_errors(src, &[("E0274", "Stdout")]);
}

// ---- Scope through type parameters and late types (G4) -----------------------

/// A trait whose impl for `Scope` spawns.
const GO: &str = "trait Go {
    fn go(self) -> Int
}

impl Go for Scope {
    fn go(self) -> Int {
        let t = self.spawn(|| 1)
        7
    }
}
";

#[test]
fn an_unscoped_closure_capturing_a_parameter_rejects_a_scope() {
    let src = format!(
        "{GO}\nfn wrap<T: Go>(x: T) -> fn() -> Int {{\n    || x.go()\n}}\n\
         fn main(out: Stdout) {{\n    let g = scope |s| {{\n        let f = wrap(s)\n        f\n    }}\n}}\n"
    );
    assert_errors(&src, &[("E0274", "wrap")]);
    let messages = messages(&src, "E0274");
    assert!(messages[0].contains("`Scope`"), "{messages:#?}");
}

/// The source text each diagnostic of `src` with `code` spans.
fn spans_of<'a>(src: &'a str, code: &str) -> Vec<&'a str> {
    check(src)
        .diagnostics
        .iter()
        .filter(|d| d.code == code)
        .map(|d| &src[d.span.start..d.span.end])
        .collect()
}

#[test]
fn a_closure_capturing_a_list_typed_only_later_cannot_escape_its_scope() {
    // The reviewer's s3: on the loop's second pass, `g` captures `[s]`.
    let src = format!(
        "{GO}\nfn helper<T: Go>(xs: List<T>) -> Int {{\n    var n = 0\n    \
         for x in xs {{\n        n = n + x.go()\n    }}\n    n\n}}\n\
         fn main(out: Stdout) {{\n    let g = scope |s| {{\n        var xs = []\n        \
         var g = || 0\n        for i in [1, 2] {{\n            g = || helper(xs)\n            \
         xs.push(s)\n        }}\n        g\n    }}\n    let n = g()\n}}\n"
    );
    let spans = spans_of(&src, "E0292");
    assert!(spans.contains(&"|| helper(xs)"), "{spans:#?}");
}

#[test]
fn a_closure_capturing_an_empty_list_that_later_holds_the_handle_is_rejected() {
    let src = "fn main(out: Stdout) {\n    let g = scope |s| {\n        var xs = []\n        \
               let g = || xs.len()\n        xs.push(s)\n        g\n    }\n}\n";
    let spans = spans_of(src, "E0292");
    assert!(spans.contains(&"|| xs.len()"), "{spans:#?}");
}

#[test]
fn a_closure_capturing_a_list_typed_later_with_plain_data_is_fine() {
    assert_ok(
        "fn f() -> fn() -> Int {\n    var xs = []\n    let g = || xs.len()\n    xs.push(1)\n    g\n}\n",
    );
}
