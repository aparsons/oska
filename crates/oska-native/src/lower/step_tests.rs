// SPDX-License-Identifier: Apache-2.0
//! Tests of the `--max-steps` counter.

use oska_check::limits::Limits;

use crate::ir::{Inst, Module, Symbol};
use crate::{layout, Target};

/// Every kind of step: calls of functions, methods, closures, named
/// functions as values, a user `Eq` through lists and `Option`, a
/// `requires` clause, a task, and each kind of loop.
const STEPS: &str = r#"struct P { x: Int }

impl Eq for P {
    fn eq(self, other: P) -> Bool {
        self.x == other.x
    }
}

fn half(n: Int) -> Int
    requires n % 2 == 0
{
    n / 2
}

fn main(io: Stdout) {
    var i = 0
    while i < 3 {
        i = i + 1
    }
    for n in 0..2 {
        i = i + half(n * 2)
    }
    for s in ["a", "b"] {
        i = i + s.len()
    }
    for c in "xy".chars() {
        i = i + c.len()
    }
    let same = [P(x: 1)] == [P(x: 1)] && Some(P(x: 2)) == Some(P(x: 2))
    let doubled = [1, 2].map(|n: Int| n * 2).map(half)
    let lens = ["a"].map(String.len)
    let t = scope |s| {
        let a = s.spawn(|| half(4))
        a.join()
    }
    let d = doubled.len() + lens.len()
    if same {
        let _ = io.println("{i} {d} {t}")
    }
}
"#;

fn lower(target: Target, limits: Limits) -> Module {
    let report = oska_check::check_module(STEPS);
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    let program = report.program.expect("the program should check");
    let rt = oska_check::check_toolchain_rt()
        .program
        .expect("std.rt should check");
    super::program(
        &program,
        &rt,
        &crate::Unplaced,
        super::Entry::Main,
        target,
        limits,
        false,
    )
    .expect("the program should lower")
}

/// The instructions that reach the operating system: system calls,
/// with their numbers, and library calls, with their imports.
fn os_calls(module: &Module) -> std::collections::BTreeSet<(bool, u16)> {
    module
        .functions
        .iter()
        .flat_map(|f| &f.insts)
        .filter_map(|inst| match inst {
            Inst::Syscall { number, .. } => Some((true, *number)),
            Inst::LibCall { import, .. } => Some((false, *import)),
            _ => None,
        })
        .collect()
}

fn counter_uses(module: &Module) -> usize {
    let at = super::STEPS_WORD * layout::WORD;
    module
        .functions
        .iter()
        .flat_map(|f| &f.insts)
        .filter(|inst| matches!(inst, Inst::Addr { symbol: Symbol::Data(d), .. } if *d == at))
        .count()
}

const STEPS_ONLY: Limits = Limits {
    max_steps: Some(1000),
    ..Limits::NONE
};

/// Under `--max-memory` the writable data holds the memory limit's
/// report in word 55, past the step counter's word, whether or not the
/// step counter is there; the call area is unchanged.
#[test]
fn max_memory_adds_its_report_word_after_the_step_counter() {
    assert_eq!(super::MEMORY_REPORT_WORD, 55, "std.rt names word 55");
    for target in Target::ALL {
        let plain = lower(target, Limits::NONE);
        for max_steps in [None, Some(1000)] {
            let limits = Limits {
                max_steps,
                max_memory: Some(1 << 20),
                ..Limits::NONE
            };
            let counted = lower(target, limits);
            assert_eq!(
                counted.extra_args,
                (super::GLOBALS + 2) * layout::WORD,
                "{target:?}"
            );
            assert_eq!(
                counted.data_size - counted.extra_args,
                plain.data_size - plain.extra_args,
                "{target:?}: the same call area"
            );
        }
    }
}

/// Without `--max-steps` nothing counts steps: no code reads the
/// counter, and the writable data is the globals and the call area as
/// it always was. With it, the counter is the word after the globals.
#[test]
fn without_max_steps_nothing_counts_steps() {
    for target in Target::ALL {
        let plain = lower(target, Limits::NONE);
        assert_eq!(counter_uses(&plain), 0, "{target:?}");
        assert_eq!(plain.extra_args, super::GLOBALS * layout::WORD);
        let counted = lower(target, STEPS_ONLY);
        assert!(counter_uses(&counted) > 0, "{target:?}");
        assert_eq!(counted.extra_args, (super::GLOBALS + 1) * layout::WORD);
        assert_eq!(
            counted.data_size - counted.extra_args,
            plain.data_size - plain.extra_args,
            "{target:?}: the same call area"
        );
    }
}

/// The counter makes no system call and no library call of its own
/// (ADR 0013): the program under the limit reaches the operating
/// system exactly as without it.
#[test]
fn the_step_counter_reaches_the_operating_system_no_more() {
    for target in Target::ALL {
        assert_eq!(
            os_calls(&lower(target, STEPS_ONLY)),
            os_calls(&lower(target, Limits::NONE)),
            "{target:?}"
        );
    }
}

/// Every target with a backend compiles the counter.
#[test]
fn every_target_builds_a_program_under_the_step_limit() {
    let report = oska_check::check_module(STEPS);
    let program = report.program.expect("the program should check");
    for target in Target::ALL {
        crate::build_limited(&program, target, &crate::Unplaced, STEPS_ONLY)
            .unwrap_or_else(|d| panic!("{target:?}: {d:?}"));
    }
}
