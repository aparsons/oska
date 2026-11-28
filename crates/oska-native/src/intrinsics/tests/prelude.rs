// SPDX-License-Identifier: Apache-2.0
//! Prelude and runtime functions.

use super::*;

/// The `std.rt` function a prelude intrinsic calls, if it calls one.
fn runtime_function(implementation: Prelude) -> Option<&'static str> {
    match implementation {
        Prelude::Runtime(f)
        | Prelude::WriteLine(f)
        | Prelude::Checked(f, _)
        | Prelude::Mutating(f, _) => Some(f),
        Prelude::ByteRead { secret: false, .. } => Some("bytes_read"),
        Prelude::ByteRead { secret: true, .. } => Some("bytes_read_secret"),
        Prelude::Confirm => Some("approver_confirm"),
        Prelude::Identity | Prelude::Field(_) => None,
    }
}

/// The `std.rt` functions that write the program's own text to
/// stdout or stderr: every other write is of a panic's report, or of
/// bytes already counted (a flush).
const OUTPUT_SINKS: [&str; 2] = ["stdout_put_line", "print_error_line"];

/// The `std.rt` functions that write only the runtime's own words:
/// `exit` reports a failed flush of stdout in its own wording.
const OWN_REPORTS: [&str; 1] = ["exit"];

/// Each `std.rt` function by name, with the names of the `std.rt`
/// functions its body calls, read from the source.
fn rt_calls() -> std::collections::BTreeMap<String, Vec<String>> {
    let mut bodies: Vec<(String, String)> = Vec::new();
    for line in oska_check::STD_RT_SOURCE.lines() {
        let head = line
            .strip_prefix("pub fn ")
            .or_else(|| line.strip_prefix("fn "));
        match head.and_then(|h| h.split_once('(')) {
            Some((name, _)) => bodies.push((name.to_string(), String::new())),
            None => {
                if let Some((_, body)) = bodies.last_mut() {
                    body.push_str(line);
                    body.push('\n');
                }
            }
        }
    }
    let names: Vec<String> = bodies.iter().map(|(n, _)| n.clone()).collect();
    bodies
        .into_iter()
        .map(|(name, body)| {
            let called = names
                .iter()
                .filter(|callee| {
                    body.match_indices(&format!("{callee}(")).any(|(at, _)| {
                        !body[..at]
                            .chars()
                            .next_back()
                            .is_some_and(|c| c.is_alphanumeric() || c == '_')
                    })
                })
                .cloned()
                .collect();
            (name, called)
        })
        .collect()
}

/// Whether the `std.rt` function `function` can reach an output sink.
fn writes_output(calls: &std::collections::BTreeMap<String, Vec<String>>, function: &str) -> bool {
    let mut seen = std::collections::BTreeSet::new();
    let mut stack = vec![function.to_string()];
    while let Some(at) = stack.pop() {
        if OUTPUT_SINKS.contains(&at.as_str()) {
            return true;
        }
        if !OWN_REPORTS.contains(&at.as_str()) && seen.insert(at.clone()) {
            stack.extend(calls.get(&at).into_iter().flatten().cloned());
        }
    }
    false
}

/// `--max-output` fails closed: a prelude intrinsic whose `std.rt`
/// function can write the program's text to stdout or stderr is a
/// [`Prelude::WriteLine`], or `Approver.confirm`'s [`Prelude::Confirm`],
/// which count the bytes first, and the lowering calls no other such
/// function directly but `dbg_write`, `main_failed` and
/// `approver_confirm`, which it counts at the call (`spend_output`).
#[test]
fn every_write_of_the_programs_text_is_counted_against_max_output() {
    let calls = rt_calls();
    let mut write_lines = 0;
    for (name, implementation) in PRELUDE {
        let Some(function) = runtime_function(*implementation) else {
            continue;
        };
        let counted = matches!(implementation, Prelude::WriteLine(_) | Prelude::Confirm);
        write_lines += usize::from(matches!(implementation, Prelude::WriteLine(_)));
        assert_eq!(
            writes_output(&calls, function),
            counted,
            "`{name}` (`{function}`) writes output exactly when it is a `Prelude::WriteLine` or a `Prelude::Confirm`"
        );
        if matches!(implementation, Prelude::Confirm) {
            assert_eq!(*name, "Approver.confirm", "only `Approver.confirm` asks");
        }
    }
    assert_eq!(write_lines, 2, "`Stdout.println` and `Stderr.println`");
    let lower = [
        include_str!("../../lower.rs"),
        include_str!("../../lower/closures.rs"),
        include_str!("../../lower/data.rs"),
        include_str!("../../lower/debug.rs"),
        include_str!("../../lower/html.rs"),
        include_str!("../../lower/lists.rs"),
        include_str!("../../lower/patterns.rs"),
        include_str!("../../lower/tasks.rs"),
        include_str!("../../lower/tests.rs"),
        include_str!("../../lower/traits.rs"),
    ];
    let mut direct = std::collections::BTreeSet::new();
    for source in lower {
        for (at, _) in source.match_indices("call_runtime(\"") {
            let rest = &source[at + "call_runtime(\"".len()..];
            let function = &rest[..rest.find('"').expect("a closing quote")];
            if writes_output(&calls, function) {
                direct.insert(function);
            }
        }
    }
    assert_eq!(
        direct,
        std::collections::BTreeSet::from(["approver_confirm", "dbg_write", "main_failed"]),
        "the lowering writes output only through these, which it counts"
    );
}

/// A `Bytes` read fails closed on its own: the generated code panics
/// when `bytes_readable` says the bytes do not all lie inside the
/// buffer, and `bytes_read` checks again rather than trust that it
/// did, so no call of it can read past the buffer.
#[test]
fn the_std_rt_function_of_every_bytes_read_checks_its_own_bounds() {
    let calls = rt_calls();
    for (name, implementation) in PRELUDE {
        if let Prelude::ByteRead { .. } = implementation {
            let function = runtime_function(*implementation).expect("a function");
            assert!(
                calls
                    .get(function)
                    .is_some_and(|called| called.iter().any(|c| c == "bytes_readable")),
                "`{name}` reads through `{function}`, which does not check its bounds"
            );
        }
    }
}

#[test]
fn every_runtime_function_a_prelude_intrinsic_names_is_in_std_rt() {
    let report = oska_check::check_toolchain_rt();
    let program = report.program.expect("std.rt should check");
    for (name, implementation) in PRELUDE {
        if let Some(function) = runtime_function(*implementation) {
            assert!(
                program.functions.contains_key(function),
                "`{name}` names `{function}`, which std.rt lacks"
            );
        }
    }
}
