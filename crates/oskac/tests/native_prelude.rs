// SPDX-License-Identifier: Apache-2.0
//! Every body-less prelude and `std.html` function has a native
//! implementation (spec #125, #140).
//!
//! [`CALLS`] holds, for each body-less function, by the name the checked
//! program calls it by, a program that calls it. The tests require a call
//! for every one but the [`EXEMPT`] (no backend has a runtime for them),
//! that the program really calls that function, that the native backend
//! compiles it for every target, and that it runs the same on both
//! backends. A new
//! body-less function fails here until it has a native implementation and
//! a row.

#![cfg(any(
    all(
        any(target_os = "linux", target_os = "macos"),
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(windows, target_arch = "x86_64")
))]

mod differential;

use std::ops::ControlFlow;

use differential::{assert_same, in_parallel, Case};
use oska_check::program::{walk, Callee, Program, Visit};
use oska_syntax::ast::{Item, ItemKind, Module, TypeKind};

/// The prelude's source, the same file the checker embeds.
const PRELUDE: &str = include_str!("../../oska-check/src/prelude.osk");

/// `std.html`'s source.
const STD_HTML: &str = include_str!("../../oska-check/src/std/html.osk");

/// Why the native test host does not record network attempts yet.
const NATIVE_NET_ATTEMPTS: &str =
    "a native plain test refuses `t.net` as the interpreter does, but \
does not record where it was asked to go: `t.net_attempts` is a later ticket of spec #228";

/// The types, or `Type.method` names, that not every backend implements,
/// with why. A program given a type that has no runtime is refused before
/// it runs, by the interpreter (`E0370`) and the native backend alike.
/// `TestContext.net_attempts` has a runtime on the interpreter only. A
/// `List`, `Option` or `Result` method written in Oska in `std.core`
/// (ADR 0034) is not here: [`required`] skips it by
/// `builtins::is_core_method`, as `oska-run` does, because a call of it is
/// a call of a function of the program, and `oska-run`'s `list_methods` and
/// `option_result` tests run each on both backends.
/// `std.sql` and `std.proc` have no runtime on either backend either: a
/// program importing one is refused (`E0371`), so their body-less
/// functions are not scanned at all.
const EXEMPT: &[(&str, &str)] = &[
    ("TestContext.net_attempts", NATIVE_NET_ATTEMPTS),
    ("Spawn", "process spawning has no runtime yet"),
    ("Sched", "scheduling has no runtime yet"),
];

/// How a [`CALLS`] program is run.
#[derive(Clone, Copy)]
enum Run {
    /// Its `main`, with nothing on stdin.
    Main,
    /// Its `main`, with this on stdin and in `$NAME`.
    WithInput(&'static str),
    /// Its `test` blocks: only `TestContext` methods.
    Tests,
}

/// One row of [`CALLS`]: the function's name, how its program runs, and the
/// program.
type Row = (&'static str, Run, &'static str);

#[path = "native_prelude/rows_capabilities.rs"]
mod rows_capabilities;

#[path = "native_prelude/rows_runtime.rs"]
mod rows_runtime;

#[path = "native_prelude/rows_integers.rs"]
mod rows_integers;

#[path = "native_prelude/rows_floats_bytes.rs"]
mod rows_floats_bytes;

/// The row tables, in order, joined into [`CALLS`].
const PARTS: &[&[Row]] = &[
    rows_capabilities::ROWS,
    rows_runtime::ROWS,
    rows_integers::ROWS,
    rows_floats_bytes::ROWS,
];

const CALL_COUNT: usize = {
    let mut total = 0;
    let mut part = 0;
    while part < PARTS.len() {
        total += PARTS[part].len();
        part += 1;
    }
    total
};

const fn joined() -> [Row; CALL_COUNT] {
    let mut all = [PARTS[0][0]; CALL_COUNT];
    let mut at = 0;
    let mut part = 0;
    while part < PARTS.len() {
        let mut row = 0;
        while row < PARTS[part].len() {
            all[at] = PARTS[part][row];
            at += 1;
            row += 1;
        }
        part += 1;
    }
    all
}

/// For each body-less function, a program that calls it, and how to run
/// it. `main` prints what each call gives, so the differential run checks
/// the value too. The rows live in `native_prelude/`.
const CALLS: &[Row] = &joined();

/// The qualified name the checked program calls `item`'s function by, if
/// it has no body.
fn bodiless(owner: Option<&str>, item: &Item) -> Option<String> {
    let ItemKind::Fn(f) = &item.kind else {
        return None;
    };
    if f.body.is_some() {
        return None;
    }
    Some(match owner {
        Some(owner) => format!("{owner}.{}", f.name.name),
        None => f.name.name.clone(),
    })
}

/// The type name methods on `target` are called by: trust is
/// transparent, so `Untrusted<String>`'s are `String`'s.
fn call_owner(target: &oska_syntax::ast::Type) -> Option<String> {
    let TypeKind::Path { segments, args } = &target.kind else {
        return None;
    };
    let head = &segments.last()?.name;
    match (head.as_str(), args.as_slice()) {
        ("Untrusted", [inner]) => call_owner(inner),
        _ => Some(head.clone()),
    }
}

/// Every body-less function `module` declares, by qualified name.
fn bodiless_functions(module: &Module) -> Vec<String> {
    let mut names = Vec::new();
    for item in &module.items {
        match &item.kind {
            ItemKind::Fn(_) => names.extend(bodiless(None, item)),
            ItemKind::Impl(decl) => {
                let Some(owner) = call_owner(&decl.target) else {
                    continue;
                };
                for method in &decl.methods {
                    names.extend(bodiless(Some(&owner), method));
                }
            }
            _ => {}
        }
    }
    names
}

/// Every body-less function of the prelude and `std.html` that a backend
/// must implement.
fn required() -> Vec<String> {
    let mut names = Vec::new();
    for src in [PRELUDE, STD_HTML] {
        let module = oska_syntax::parse_prelude(src).expect("the source should parse");
        names.extend(bodiless_functions(&module));
    }
    names.retain(|name| {
        let owner = name.split('.').next().unwrap_or("");
        let (ty, method) = name.split_once('.').unwrap_or((name, ""));
        !EXEMPT
            .iter()
            .any(|(e, _)| *e == owner || *e == name.as_str())
            && !oska_check::builtins::is_core_method(ty, method)
    });
    names
}

#[test]
fn every_bodiless_function_has_a_call() {
    let names = required();
    assert!(
        names.len() > 60 && names.iter().any(|n| n == "Stdout.println"),
        "the scan should find the prelude's functions: {names:?}"
    );
    let missing: Vec<&String> = names
        .iter()
        .filter(|n| !CALLS.iter().any(|(name, _, _)| name == n))
        .collect();
    assert!(
        missing.is_empty(),
        "body-less functions with no call here, so no proof of a native implementation: {missing:?}"
    );
    for (name, _, _) in CALLS {
        assert!(names.iter().any(|n| n == name), "`{name}` is not body-less");
    }
}

/// The whole program of a [`CALLS`] row: its source and `shown`, which
/// words a `Bool` for printing (`Bool` is not `Display`).
fn source(src: &str) -> String {
    format!("{src}\n\nfn shown(b: Bool) -> String {{\n    if b {{ \"true\" }} else {{ \"false\" }}\n}}\n")
}

/// The checked program of a [`CALLS`] row, which must check.
fn checked(name: &str, src: &str) -> Program {
    let report = oska_check::check_module(src);
    match report.program {
        Some(program) if report.diagnostics.is_empty() => program,
        _ => panic!(
            "the call of `{name}` should check: {:#?}",
            report.diagnostics
        ),
    }
}

/// Whether `program` calls the intrinsic `name` anywhere.
fn calls(program: &Program, name: &str) -> bool {
    let qualified = format!("html.{name}");
    let bodies = program
        .functions
        .values()
        .map(|f| &f.body)
        .chain(program.tests.iter().map(|t| &t.body));
    for body in bodies {
        let found = walk(body, &mut |visit| match visit {
            Visit::Callee(Callee::Intrinsic(callee), _)
                if *callee == name || *callee == qualified =>
            {
                ControlFlow::Break(())
            }
            _ => ControlFlow::Continue(()),
        });
        if found.is_break() {
            return true;
        }
    }
    false
}

#[test]
fn every_call_calls_its_function_and_builds_natively() {
    in_parallel(CALLS, |(name, run, src)| {
        let program = checked(name, &source(src));
        assert!(
            calls(&program, name),
            "the program for `{name}` never calls it"
        );
        for target in oska_native::Target::ALL {
            let built = match run {
                Run::Tests => oska_native::build_tests(&program, target),
                Run::Main | Run::WithInput(_) => {
                    oska_native::build(&program, target, &oska_native::Unplaced)
                }
            };
            if let Err(refusal) = built {
                panic!(
                    "`{name}` has no native implementation for {}: {refusal:#?}",
                    target.name()
                );
            }
        }
    });
}

#[test]
fn every_call_runs_the_same_on_both_backends() {
    in_parallel(CALLS, |(name, run, src)| {
        let case = Case::file(&format!("prelude-{name}"), &source(src)).with_args(&["given"]);
        let case = match run {
            Run::Tests => return,
            Run::Main => case,
            Run::WithInput(stdin) => case.with_stdin(stdin).with_env("NAME", "from env"),
        };
        assert_same(&case);
    });
}
