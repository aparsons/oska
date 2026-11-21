//! The guard, fed code that a broken lowering or optimizer could make.

use super::{secrets, verify, Leak};
use crate::ir::{BinOp, Cond, Function, Inst, Width};

/// A function `name` of `params` parameters with the code `insts`.
fn function(name: &str, params: u32, insts: Vec<Inst>) -> Function {
    let temps = insts
        .iter()
        .flat_map(|inst| inst.def().into_iter().chain(inst.uses()))
        .max()
        .map_or(params, |t| (t + 1).max(params));
    let labels = insts
        .iter()
        .filter_map(|inst| match inst {
            Inst::Label(l) => Some(*l + 1),
            _ => inst.target().map(|l| l + 1),
        })
        .max()
        .unwrap_or(0);
    Function {
        name: name.to_string(),
        params,
        temps,
        labels,
        cold: insts.len(),
        insts,
    }
}

/// `t1`, a secret read from the `Secret` at the parameter `t0`, then
/// `t2 = t1 ^ t0`: the code `rest` runs after it.
fn with_secret(rest: Vec<Inst>) -> Vec<Inst> {
    let mut insts = vec![
        Inst::Load {
            width: Width::W64,
            dst: 3,
            base: 0,
            offset: 8,
        },
        Inst::Secret { dst: 1, src: 3 },
        Inst::Binary {
            op: BinOp::Xor,
            dst: 2,
            lhs: 1,
            rhs: 0,
        },
    ];
    insts.extend(rest);
    insts
}

fn leak(functions: &[Function]) -> Leak {
    secrets(functions).expect_err("the guard should refuse the code")
}

#[test]
fn a_branch_on_a_secret_fails_the_build_with_an_internal_error() {
    let f = function(
        "main",
        1,
        with_secret(vec![
            Inst::JumpIfZero { cond: 2, target: 0 },
            Inst::Label(0),
            Inst::Return(0),
        ]),
    );
    let diagnostic = verify(&[f]).expect_err("the build should fail");
    assert_eq!(diagnostic.code, "E0390");
    assert!(
        diagnostic.message.starts_with("internal error:"),
        "{}",
        diagnostic.message
    );
    assert!(
        diagnostic.message.contains("the condition of a branch")
            && diagnostic.message.contains("`main`"),
        "{}",
        diagnostic.message
    );
}

#[test]
fn every_kind_of_branch_on_a_secret_is_refused() {
    let branches = [
        Inst::Branch {
            cond: Cond::Eq,
            lhs: 0,
            rhs: 2,
            target: 0,
        },
        Inst::BranchImm {
            cond: Cond::Lt,
            lhs: 2,
            imm: 3,
            target: 0,
        },
        Inst::JumpIfZero { cond: 2, target: 0 },
    ];
    for branch in branches {
        let f = function(
            "f",
            1,
            with_secret(vec![branch.clone(), Inst::Label(0), Inst::Return(0)]),
        );
        assert_eq!(leak(&[f]).what, "the condition of a branch", "{branch:?}");
    }
}

#[test]
fn checked_arithmetic_on_a_secret_is_refused_for_its_overflow_branch() {
    let f = function(
        "f",
        1,
        with_secret(vec![
            Inst::Arith {
                op: crate::ir::Arith::Add,
                int: crate::ir::IntKind {
                    signed: true,
                    bits: 64,
                },
                dst: 4,
                lhs: 2,
                rhs: 0,
                overflow: 0,
            },
            Inst::Label(0),
            Inst::Return(4),
        ]),
    );
    assert!(leak(&[f]).what.starts_with("checked arithmetic"));
}

#[test]
fn a_load_or_store_at_a_secret_address_is_refused() {
    let load = Inst::Load {
        width: Width::W8,
        dst: 4,
        base: 2,
        offset: 0,
    };
    let store = Inst::Store {
        width: Width::W8,
        base: 2,
        offset: 0,
        src: 0,
    };
    for access in [load, store] {
        let f = function("f", 1, with_secret(vec![access.clone(), Inst::Return(0)]));
        assert_eq!(
            leak(&[f]).what,
            "the address of a load or store",
            "{access:?}"
        );
    }
}

#[test]
fn a_secret_may_be_computed_with_compared_stored_and_returned() {
    let f = function(
        "f",
        1,
        with_secret(vec![
            Inst::Compare {
                cond: Cond::Eq,
                dst: 4,
                lhs: 2,
                rhs: 0,
            },
            Inst::BinaryImm {
                op: BinOp::Shl,
                dst: 5,
                lhs: 4,
                imm: 3,
            },
            Inst::Store {
                width: Width::W64,
                base: 0,
                offset: 8,
                src: 5,
            },
            Inst::Return(5),
        ]),
    );
    let marked = secrets(&[f]).expect("the code is constant-time");
    assert_eq!(marked[0], [false, true, true, false, true, true]);
}

#[test]
fn a_secret_argument_is_followed_into_the_function_it_is_passed_to() {
    let caller = function(
        "caller",
        1,
        with_secret(vec![
            Inst::Call {
                dst: 4,
                callee: 1,
                args: vec![0, 2],
            },
            Inst::Return(4),
        ]),
    );
    let callee = function(
        "helper",
        2,
        vec![
            Inst::BranchImm {
                cond: Cond::Eq,
                lhs: 1,
                imm: 0,
                target: 0,
            },
            Inst::Label(0),
            Inst::Return(0),
        ],
    );
    assert_eq!(
        leak(&[caller, callee]),
        Leak {
            function: "helper".to_string(),
            what: "the condition of a branch",
        }
    );
}

#[test]
fn a_result_computed_from_a_secret_argument_is_secret_to_the_caller() {
    let caller = function(
        "caller",
        1,
        with_secret(vec![
            Inst::Call {
                dst: 4,
                callee: 1,
                args: vec![2],
            },
            Inst::Load {
                width: Width::W8,
                dst: 5,
                base: 4,
                offset: 0,
            },
            Inst::Return(5),
        ]),
    );
    let rotl = function(
        "rotl",
        1,
        vec![
            Inst::BinaryImm {
                op: BinOp::Shl,
                dst: 1,
                lhs: 0,
                imm: 1,
            },
            Inst::Return(1),
        ],
    );
    assert_eq!(leak(&[caller, rotl]).function, "caller");
}

/// `bytes_read` marks the bytes it reads, but `Bytes.at` of public bytes
/// is public: a mark stays in the function that makes it.
#[test]
fn a_mark_made_in_a_callee_does_not_make_its_result_secret() {
    let caller = function(
        "main",
        1,
        vec![
            Inst::Call {
                dst: 1,
                callee: 1,
                args: vec![0],
            },
            Inst::JumpIfZero { cond: 1, target: 0 },
            Inst::Label(0),
            Inst::Return(1),
        ],
    );
    let read = function(
        "bytes_read",
        1,
        vec![
            Inst::Load {
                width: Width::W8,
                dst: 1,
                base: 0,
                offset: 24,
            },
            Inst::Secret { dst: 2, src: 1 },
            Inst::Return(2),
        ],
    );
    assert!(secrets(&[caller, read]).is_ok());
}

#[test]
fn a_loop_bound_on_a_public_length_is_fine_and_on_a_secret_is_not() {
    // `i` counts up to the parameter `t0`, a public length, while the
    // loop folds secret bytes into `t5`.
    let body = |bound| {
        vec![
            Inst::Const { dst: 1, value: 0 },
            Inst::Const { dst: 5, value: 0 },
            Inst::Label(0),
            Inst::Branch {
                cond: Cond::Ge,
                lhs: 1,
                rhs: bound,
                target: 1,
            },
            Inst::Load {
                width: Width::W8,
                dst: 2,
                base: 0,
                offset: 0,
            },
            Inst::Secret { dst: 3, src: 2 },
            Inst::Binary {
                op: BinOp::Or,
                dst: 5,
                lhs: 5,
                rhs: 3,
            },
            Inst::BinaryImm {
                op: BinOp::Add,
                dst: 1,
                lhs: 1,
                imm: 1,
            },
            Inst::Jump(0),
            Inst::Label(1),
            Inst::Return(5),
        ]
    };
    assert!(secrets(&[function("f", 1, body(0))]).is_ok());
    assert_eq!(
        leak(&[function("f", 1, body(5))]).what,
        "the condition of a branch"
    );
}

/// Programs through the whole pipeline: lowering, the optimizer, the
/// guard, and each target's instruction selection.
mod programs {
    use oska_check::limits::Limits;

    use super::super::secrets;
    use crate::ir::{Inst, Module};
    use crate::{lower, Target};

    /// Every operation on a secret, called from `main`.
    const SECRET_OPS: &str = include_str!("secret_ops.osk");

    /// `src`, which checks cleanly, lowered and optimized for `target`.
    fn lower(src: &str, target: Target) -> Result<Module, oska_syntax::Diagnostic> {
        let report = oska_check::check_module(src);
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        let program = report.program.expect("the program should check");
        let rt = oska_check::check_toolchain_rt()
            .program
            .expect("std.rt should check");
        lower::program(
            &program,
            &rt,
            &crate::Unplaced,
            lower::Entry::Main,
            target,
            Limits::NONE,
        )
    }

    /// The names of the functions in which some temporary holds a secret.
    fn with_secrets(module: &Module) -> Vec<&str> {
        let marked = secrets(&module.functions).expect("the code is constant-time");
        module
            .functions
            .iter()
            .zip(&marked)
            .filter(|(_, m)| m.contains(&true))
            .map(|(f, _)| f.name.as_str())
            .collect()
    }

    #[test]
    fn every_secret_operation_builds_for_every_target() {
        let report = oska_check::check_module(SECRET_OPS);
        let program = report.program.expect("the program should check");
        for target in Target::ALL {
            crate::build(&program, target, &crate::Unplaced)
                .unwrap_or_else(|d| panic!("{target:?}: {d:?}"));
        }
    }

    /// The guard follows the secrets of each operation, in the program's
    /// functions and in the `std.rt` functions that handle secret bytes.
    #[test]
    fn the_guard_follows_every_secret_operation() {
        let module = lower(SECRET_OPS, Target::LinuxAarch64).expect("it lowers");
        let names = with_secrets(&module);
        for name in [
            "ops_u8",
            "ops_u32",
            "ops_u64",
            "conversions",
            "conditions",
            "equalities",
            "selects",
            "buffers",
            "std.rt.bytes_read",
            "std.rt.bytes_select",
            "std.rt.bytes_constant_time_eq",
            "std.rt.text_equal_constant",
            "std.rt.bytes_push_int",
        ] {
            assert!(names.contains(&name), "no secret in `{name}`: {names:?}");
        }
    }

    /// A public program that branches on what it reads from public bytes
    /// and text builds: the marks `std.rt` makes on the bytes it reads
    /// stay in its own functions.
    #[test]
    fn public_bytes_stay_public() {
        let src = r#"fn main(io: Stdout) {
    let b = Bytes.from_hex("0102030405060708").unwrap_or(Bytes.new())
    if b.at(1) == 2 && b.read_u32_be(0) > 3 && b.read_u64_le(0) != 0 {
        let _ = io.println("yes")
    }
}
"#;
        for target in Target::ALL {
            lower(src, target).unwrap_or_else(|d| panic!("{target:?}: {d:?}"));
        }
    }

    /// A deliberately broken lowering, which makes `ct_select` a branch,
    /// trips the guard: the build fails with an internal error.
    #[test]
    fn a_branch_on_a_secret_from_a_broken_lowering_fails_the_build() {
        let src = r#"fn main(io: Stdout) {
    let a: Secret<U32> = Secret.new(1)
    let b: Secret<U32> = Secret.new(2)
    let c = a.ct_eq(b)
    let picked = c.ct_select(a, b).expose().to_int()
    let _ = io.println("{picked}")
}
"#;
        lower::BRANCHY.with(|b| b.set(true));
        let built = lower(src, Target::LinuxX86_64);
        lower::BRANCHY.with(|b| b.set(false));
        let diagnostic = built.expect_err("the build should fail");
        assert_eq!(diagnostic.code, "E0390");
        assert!(
            diagnostic.message.starts_with("internal error:")
                && diagnostic.message.contains("the condition of a branch"),
            "{}",
            diagnostic.message
        );
        assert!(lower(src, Target::LinuxX86_64).is_ok(), "fixed, it builds");
    }

    /// On aarch64 and on x86_64, the machine code of every instruction
    /// that reads a secret has no conditional branch, and reads or writes
    /// memory only in the stack frame, unless it is a load, store or call,
    /// whose address the guard has shown is public.
    #[test]
    fn the_machine_code_of_every_secret_operation_is_branch_free() {
        for target in [Target::LinuxAarch64, Target::LinuxX86_64] {
            let module = lower(SECRET_OPS, target).expect("it lowers");
            let marked = secrets(&module.functions).expect("the code is constant-time");
            let selected = target.trace(&module);
            let mut inspected = 0;
            for ((function, marked), selected) in
                module.functions.iter().zip(&marked).zip(&selected)
            {
                for (inst, code) in function.insts.iter().zip(selected) {
                    if !inst.uses().iter().any(|&t| marked[t as usize]) {
                        continue;
                    }
                    inspected += 1;
                    let at = format!("{target:?}, `{}`: {inst:?}", function.name);
                    assert_eq!(code.conditional_branches, 0, "{at}");
                    let addressed = matches!(
                        inst,
                        Inst::Load { .. }
                            | Inst::Store { .. }
                            | Inst::Call { .. }
                            | Inst::CallIndirect { .. }
                    );
                    if !addressed {
                        assert_eq!(code.other_accesses, 0, "{at}");
                    }
                }
            }
            assert!(inspected > 100, "{target:?}: only {inspected} inspected");
            // The trace sees branches and loads where public values decide
            // them, so it would see them where a secret did.
            let all = selected.iter().flatten();
            assert!(all.clone().any(|code| code.conditional_branches > 0));
            assert!(all.clone().any(|code| code.other_accesses > 0));
        }
    }
}
