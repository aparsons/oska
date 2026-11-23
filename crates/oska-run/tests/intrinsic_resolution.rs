// SPDX-License-Identifier: Apache-2.0
//! Intrinsics resolved once (#502): the interpreter finds each body-less
//! prelude function without scanning every table on each call, and still
//! reaches the same implementation by every name a program can call it by.

mod common;

use std::ops::ControlFlow;

use common::assert_runs;
use oska_check::program::{walk, Callee, Program, Visit};
use oska_run::FakeHost;

#[test]
fn an_intrinsic_called_on_every_iteration_gives_the_same_result_each_time() {
    let src = "fn main(io: Stdout) -> Result<(), IoError> {\n    \
               var total = 0\n    \
               for i in 0..1000 {\n        \
               total = total + i.to_string().len()\n    \
               }\n    \
               io.println(\"{total}\")\n}\n";
    // 10 one-digit, 90 two-digit and 900 three-digit numbers.
    assert_runs(src, &[], "2890\n");
}

#[test]
fn a_std_function_is_reached_whether_or_not_the_program_declares_the_same_name() {
    let calls = "fn main(io: Stdout) -> Result<(), IoError> {\n    \
                 for _ in 0..2 {\n        \
                 io.println(html.text(\"<a&b>\").render())?\n    \
                 }\n    \
                 Ok(())\n}\n";
    let expected = "&lt;a&amp;b&gt;\n".repeat(2);
    assert_runs(&format!("use std.html\n{calls}"), &[], &expected);
    assert_runs(
        &format!("use std.html\nfn text(n: Int) -> Int {{\n    n\n}}\n{calls}"),
        &[],
        &expected,
    );
}

#[test]
fn a_trait_method_in_generic_code_reaches_the_prelude_intrinsic_or_the_programs_own_implementation()
{
    let src = "struct Point { x: Int, y: Int }\n\
               impl Display for Point {\n    \
               fn to_string(self) -> String {\n        \
               let x = self.x\n        \
               let y = self.y\n        \
               \"({x}, {y})\"\n    \
               }\n}\n\
               fn show<T: Display>(value: T) -> Untrusted<String> {\n    \
               \"<{value}>\"\n}\n\
               fn main(io: Stdout) -> Result<(), IoError> {\n    \
               for _ in 0..3 {\n        \
               io.println(show(7))?\n        \
               io.println(show(Point(x: 1, y: 2)))?\n    \
               }\n    \
               Ok(())\n}\n";
    assert_runs(src, &[], &"<7>\n<(1, 2)>\n".repeat(3));
}

/// The intrinsic call sites in `program`'s `main`, by name, and whether
/// each has been resolved.
fn intrinsic_sites(program: &Program) -> Vec<(String, bool)> {
    let mut sites = Vec::new();
    let _ = walk::<()>(&program.functions["main"].body, &mut |visit| {
        if let Visit::Callee(Callee::Intrinsic(name), _) = visit {
            sites.push((name.to_string(), name.resolved().is_some()));
        }
        ControlFlow::Continue(())
    });
    sites
}

#[test]
fn running_a_program_resolves_each_intrinsic_call_site_it_reaches_and_keeps_it() {
    let src = "fn main(io: Stdout) -> Result<(), IoError> {\n    \
               var total = 0\n    \
               for i in 0..3 {\n        \
               total = total + i.to_string().len()\n    \
               }\n    \
               if total > 100 {\n        \
               io.println(\"never\")?\n    \
               }\n    \
               io.println(\"{total}\")\n}\n";
    let report = oska_check::check_module(src);
    let program = report.program.expect("the program checks");
    let before = intrinsic_sites(&program);
    assert!(before.iter().all(|(_, resolved)| !resolved), "{before:?}");

    let outcome = oska_run::run_program(&program, &mut FakeHost::new(), &["main.osk".into()]);
    assert_eq!(outcome.stdout, "3\n");

    let after = intrinsic_sites(&program);
    let resolved = |name: &str| {
        after
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, r)| *r)
            .collect::<Vec<_>>()
    };
    // `i.to_string()` and the `{total}` hole.
    assert_eq!(resolved("Int.to_string"), [true, true], "{after:?}");
    assert_eq!(resolved("String.len"), [true], "{after:?}");
    // The first `println` is never reached, so it is never resolved.
    assert_eq!(resolved("Stdout.println"), [false, true], "{after:?}");
}
