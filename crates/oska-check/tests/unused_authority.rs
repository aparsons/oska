//! Unused authority (ticket #368, ADR 0016): the release check rejects a
//! capability-bearing parameter that its function never uses, directly or
//! by passing it on. An ordinary check says nothing.

mod common;

use common::assert_ok;
use oska_check::{check_module_with, CheckOptions};
use oska_syntax::Applicability;

/// `(code, spanned text)` for every diagnostic of the release check.
fn release_errors(src: &str) -> Vec<(&'static str, &str)> {
    check_module_with(src, CheckOptions { release: true })
        .diagnostics
        .iter()
        .map(|d| (d.code, &src[d.span.start..d.span.end]))
        .collect()
}

/// Assert that the release check accepts `src`.
fn assert_release_ok(src: &str) {
    assert_eq!(release_errors(src), [], "{src}");
}

/// The source after applying the fix of the first `E0296`, and how safe
/// that fix is.
fn fixed(src: &str) -> (String, Applicability) {
    let report = check_module_with(src, CheckOptions { release: true });
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|d| d.code == "E0296")
        .unwrap_or_else(|| panic!("{src:?} should report E0296: {:#?}", report.diagnostics));
    let fix = diagnostic.fix.as_ref().expect("E0296 should carry a fix");
    let mut edits = fix.edits.clone();
    edits.sort_by_key(|e| std::cmp::Reverse(e.span.start));
    let mut out = src.to_string();
    for edit in edits {
        out.replace_range(edit.span.start..edit.span.end, &edit.replacement);
    }
    (out, fix.applicability)
}

// ---- Release check only -------------------------------------------------------------

#[test]
fn an_unused_capability_parameter_is_unused_authority_in_the_release_check() {
    assert_eq!(
        release_errors("fn main(out: Stdout, net: Net) {\n    let _ = out.println(\"hi\")\n}"),
        [("E0296", "net: Net")]
    );
}

#[test]
fn an_ordinary_check_does_not_report_unused_authority() {
    assert_ok("fn main(out: Stdout, net: Net) {\n    let _ = out.println(\"hi\")\n}");
}

#[test]
fn a_capability_used_by_main_is_not_unused_authority() {
    assert_release_ok("fn main(out: Stdout) {\n    let _ = out.println(\"hi\")\n}");
}

#[test]
fn a_parameter_that_is_not_capability_bearing_is_not_judged() {
    assert_release_ok("fn double(n: Int, label: String) -> Int {\n    n * 2\n}\n\nfn main() {}");
}

// ---- Any read is a use --------------------------------------------------------------

#[test]
fn passing_a_capability_on_is_a_use() {
    assert_release_ok(
        "fn greet(out: Stdout) {\n    let _ = out.println(\"hi\")\n}\n\n\
         fn main(out: Stdout) {\n    greet(out)\n}",
    );
}

#[test]
fn attenuating_a_capability_is_a_use() {
    assert_release_ok(
        "fn docs(fs: Fs) -> DirRead {\n    fs.read_only().restrict_to(\"docs\")\n}\n\n\
         fn main() {}",
    );
}

#[test]
fn storing_a_capability_in_a_value_is_a_use() {
    assert_release_ok(
        "cap struct Logger { out: Stdout }\n\n\
         fn logger(out: Stdout) -> Logger {\n    let l = Logger(out: out)\n    l\n}\n\n\
         fn main() {}",
    );
}

#[test]
fn returning_a_capability_is_a_use() {
    assert_release_ok("fn pass(out: Stdout) -> Stdout {\n    out\n}\n\nfn main() {}");
}

#[test]
fn capturing_a_capability_in_a_closure_is_a_use() {
    assert_release_ok(
        "fn later(out: Stdout) -> cap fn() {\n    || {\n        let _ = out.println(\"hi\")\n    }\n}\n\n\
         fn main() {}",
    );
}

// ---- What is capability-bearing -----------------------------------------------------

#[test]
fn an_unused_cap_struct_parameter_is_unused_authority() {
    assert_eq!(
        release_errors(
            "cap struct Logger { out: Stdout }\n\n\
             fn work(log: Logger, n: Int) -> Int {\n    n\n}\n\nfn main() {}"
        ),
        [("E0296", "log: Logger")]
    );
}

#[test]
fn an_unused_cap_fn_parameter_is_unused_authority() {
    assert_eq!(
        release_errors("fn work(f: cap fn(), n: Int) -> Int {\n    n\n}\n\nfn main() {}"),
        [("E0296", "f: cap fn()")]
    );
}

#[test]
fn an_unused_plain_fn_parameter_is_not_judged() {
    assert_release_ok("fn work(f: fn(), n: Int) -> Int {\n    n\n}\n\nfn main() {}");
}

#[test]
fn a_capability_carried_through_a_type_argument_is_unused_authority() {
    assert_eq!(
        release_errors("fn work(outs: List<Stdout>, n: Int) -> Int {\n    n\n}\n\nfn main() {}"),
        [("E0296", "outs: List<Stdout>")]
    );
}

#[test]
fn an_unused_parameter_of_a_bounded_type_parameter_is_unused_authority() {
    assert_eq!(
        release_errors("fn work<T: Display>(x: T, n: Int) -> Int {\n    n\n}\n\nfn main() {}"),
        [("E0296", "x: T")]
    );
}

#[test]
fn an_unused_parameter_of_an_inherent_method_is_unused_authority() {
    assert_eq!(
        release_errors(
            "struct Counter { n: Int }\n\n\
             impl Counter {\n    fn get(self, clock: Clock) -> Int {\n        self.n\n    }\n}\n\n\
             fn main() {}"
        ),
        [("E0296", "clock: Clock")]
    );
}

// ---- Exemptions, and none for `_` -------------------------------------------------

#[test]
fn a_method_whose_signature_a_trait_fixes_is_exempt() {
    assert_release_ok(
        "trait Sink {\n    fn put(self, out: Stdout, n: Int) -> Int\n}\n\n\
         struct Quiet {}\n\n\
         impl Sink for Quiet {\n    fn put(self, out: Stdout, n: Int) -> Int {\n        n\n    }\n}\n\n\
         fn main() {}",
    );
}

#[test]
fn a_traits_default_method_is_exempt() {
    assert_release_ok(
        "trait Sink {\n    fn put(self, out: Stdout, n: Int) -> Int {\n        n\n    }\n}\n\n\
         fn main() {}",
    );
}

#[test]
fn an_underscore_name_does_not_silence_unused_authority() {
    assert_eq!(
        release_errors("fn main(_net: Net) {}"),
        [("E0296", "_net: Net")]
    );
}

// ---- The fix ------------------------------------------------------------------------

#[test]
fn on_main_the_fix_removes_the_parameter_and_is_machine() {
    assert_eq!(
        fixed("fn main(out: Stdout, net: Net) {\n    let _ = out.println(\"hi\")\n}"),
        (
            "fn main(out: Stdout) {\n    let _ = out.println(\"hi\")\n}".to_string(),
            Applicability::Machine
        )
    );
}

#[test]
fn the_fix_removes_a_first_parameter_with_its_comma() {
    assert_eq!(
        fixed("fn main(net: Net, out: Stdout) {\n    let _ = out.println(\"hi\")\n}").0,
        "fn main(out: Stdout) {\n    let _ = out.println(\"hi\")\n}"
    );
}

#[test]
fn the_fix_empties_the_parentheses_of_an_only_parameter() {
    assert_eq!(fixed("fn main(\n    net: Net,\n) {}").0, "fn main() {}");
}

#[test]
fn the_fix_keeps_the_receiver_of_a_method() {
    assert_eq!(
        fixed(
            "struct Counter { n: Int }\n\n\
             impl Counter {\n    fn get(self, clock: Clock) -> Int {\n        self.n\n    }\n}\n\n\
             fn main() {}"
        ),
        (
            "struct Counter { n: Int }\n\n\
             impl Counter {\n    fn get(self) -> Int {\n        self.n\n    }\n}\n\n\
             fn main() {}"
                .to_string(),
            Applicability::Maybe
        )
    );
}

#[test]
fn off_main_the_fix_is_maybe_because_callers_change_too() {
    assert_eq!(
        fixed("fn work(net: Net, n: Int) -> Int {\n    n\n}\n\nfn main() {}"),
        (
            "fn work(n: Int) -> Int {\n    n\n}\n\nfn main() {}".to_string(),
            Applicability::Maybe
        )
    );
}

// ---- Reads that are not a use -------------------------------------------------------

#[test]
fn a_read_of_a_let_that_shadows_the_parameter_is_not_a_use() {
    assert_eq!(
        release_errors(
            "fn work(net: Net, n: Int) -> Int {\n    let net = n\n    net + 1\n}\n\nfn main() {}"
        ),
        [("E0296", "net: Net")]
    );
}

#[test]
fn a_read_of_a_closure_parameter_that_shadows_the_parameter_is_not_a_use() {
    assert_eq!(
        release_errors(
            "fn work(net: Net, n: Int) -> Int {\n    let f = |net: Int| net + 1\n    f(n)\n}\n\n\
             fn main() {}"
        ),
        [("E0296", "net: Net")]
    );
}

#[test]
fn a_read_before_a_shadowing_let_is_a_use() {
    assert_release_ok(
        "fn work(out: Stdout, n: Int) -> Int {\n    let _ = out.println(\"hi\")\n    let out = n\n    out\n}\n\n\
         fn main() {}",
    );
}

#[test]
fn a_shadowing_let_inside_a_block_ends_with_the_block() {
    assert_release_ok(
        "fn work(out: Stdout, n: Int) -> Int {\n    if n > 0 {\n        let out = n\n    }\n    \
         let _ = out.println(\"hi\")\n    n\n}\n\nfn main() {}",
    );
}

#[test]
fn discarding_the_parameter_with_let_underscore_is_not_a_use() {
    assert_eq!(
        release_errors("fn main(net: Net) {\n    let _ = net\n}"),
        [("E0296", "net: Net")]
    );
}

#[test]
fn a_read_in_a_contract_is_a_use() {
    // A contract may read a parameter of a bounded type parameter: `E0274`
    // keeps its type argument pure there. Removing the parameter would
    // break the contract, so the read is a use. (Reading a capability in a
    // contract is `E0280` anyway.)
    assert_release_ok(
        "trait Speak {\n    fn ok(self) -> Bool\n}\n\n\
         impl Speak for Int {\n    fn ok(self) -> Bool {\n        self > 0\n    }\n}\n\n\
         fn check<T: Speak>(x: T) -> Int\n    requires x.ok()\n{\n    1\n}\n\n\
         fn main() {\n    let n = check(5)\n}",
    );
}
