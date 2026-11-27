// SPDX-License-Identifier: Apache-2.0
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
    assert_eq!(diagnostic.code, "E0398");
    assert!(
        diagnostic.message.starts_with("internal error:")
            && !diagnostic.message.contains("interpret"),
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

/// A value marked in a callee and returned is secret in the caller.
#[test]
fn a_value_marked_in_a_callee_is_secret_in_the_caller() {
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
        "bytes_read_secret",
        1,
        vec![
            Inst::Load {
                width: Width::W8,
                dst: 1,
                base: 0,
                offset: 24,
            },
            Inst::Secret { dst: 2, src: 1 },
            Inst::BinaryImm {
                op: BinOp::Shl,
                dst: 3,
                lhs: 2,
                imm: 8,
            },
            Inst::Return(3),
        ],
    );
    assert_eq!(
        leak(&[caller, read]),
        Leak {
            function: "main".to_string(),
            what: "the condition of a branch",
        }
    );
}

/// What a secret exposure gives is public: the result of a constant-time
/// comparison, which the caller may branch on.
#[test]
fn an_exposed_value_is_public() {
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
    let compare = function(
        "bytes_constant_time_eq",
        1,
        vec![
            Inst::Load {
                width: Width::W8,
                dst: 1,
                base: 0,
                offset: 24,
            },
            Inst::Secret { dst: 2, src: 1 },
            Inst::Expose { dst: 3, src: 2 },
            Inst::CompareImm {
                cond: Cond::Eq,
                dst: 4,
                lhs: 3,
                imm: 0,
            },
            Inst::Return(4),
        ],
    );
    assert!(secrets(&[caller, compare]).is_ok());
}

/// A recursive function that branches on what its own recursive call
/// returns, computed from its secret argument.
#[test]
fn a_branch_on_a_recursive_call_of_a_secret_is_refused() {
    let caller = function(
        "main",
        1,
        with_secret(vec![
            Inst::Call {
                dst: 4,
                callee: 1,
                args: vec![2],
            },
            Inst::Return(0),
        ]),
    );
    let recursive = function(
        "recursive",
        1,
        vec![
            Inst::Call {
                dst: 1,
                callee: 1,
                args: vec![0],
            },
            Inst::JumpIfZero { cond: 1, target: 0 },
            Inst::Label(0),
            Inst::Return(0),
        ],
    );
    assert_eq!(leak(&[caller, recursive]).function, "recursive");
}

/// An indirect call's callee is not known, so a secret argument to one
/// cannot be followed: it is refused.
#[test]
fn a_secret_argument_of_an_indirect_call_is_refused() {
    let f = function(
        "f",
        1,
        with_secret(vec![
            Inst::Addr {
                dst: 4,
                symbol: crate::ir::Symbol::Function(1),
            },
            Inst::CallIndirect {
                dst: 5,
                target: 4,
                args: vec![2],
            },
            Inst::Return(5),
        ]),
    );
    let g = function("g", 1, vec![Inst::Return(0)]);
    assert_eq!(
        leak(&[f, g]).what,
        "an argument of an indirect call, whose callee is not known,"
    );
}

/// An indirect call may reach any function whose address is taken: its
/// result is secret when one of theirs is.
#[test]
fn an_indirect_call_gives_a_secret_when_a_function_it_may_reach_does() {
    let f = function(
        "f",
        1,
        vec![
            Inst::Addr {
                dst: 1,
                symbol: crate::ir::Symbol::Function(1),
            },
            Inst::CallIndirect {
                dst: 2,
                target: 1,
                args: vec![0],
            },
            Inst::JumpIfZero { cond: 2, target: 0 },
            Inst::Label(0),
            Inst::Return(0),
        ],
    );
    let g = function(
        "g",
        1,
        vec![
            Inst::Load {
                width: Width::W64,
                dst: 1,
                base: 0,
                offset: 8,
            },
            Inst::Secret { dst: 2, src: 1 },
            Inst::Return(2),
        ],
    );
    assert_eq!(leak(&[f, g]).function, "f");
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
    use std::collections::HashMap;

    use oska_check::limits::Limits;

    use super::super::secrets;
    use crate::ir::{Inst, Module};
    use crate::{lower, Target};

    /// Every operation on a secret, called from `main`.
    const SECRET_OPS: &str = include_str!("secret_ops.osk");

    /// The functions in which `SECRET_OPS`'s operations are computed: one
    /// for each secret integer type's operators and methods, the
    /// conversions, `Secret<Bool>`'s methods, `ct_eq`, `ct_select` and
    /// `Secret<Bytes>`'s methods; and the `std.rt` functions that handle
    /// secret bytes and secret arguments.
    const SECRET_FUNCTIONS: [&str; 12] = [
        "ops_u8",
        "ops_u32",
        "ops_u64",
        "conversions",
        "conditions",
        "equalities",
        "selects",
        "buffers",
        "std.rt.bytes_read_secret",
        "std.rt.bytes_select",
        "std.rt.equal_constant",
        "std.rt.bytes_push_int",
    ];

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
            false,
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
        for name in SECRET_FUNCTIONS {
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
        assert_eq!(diagnostic.code, "E0398");
        assert!(
            diagnostic.message.starts_with("internal error:")
                && diagnostic.message.contains("the condition of a branch"),
            "{}",
            diagnostic.message
        );
        assert!(lower(src, Target::LinuxX86_64).is_ok(), "fixed, it builds");
    }

    /// The machine code `src` lowers to for `target`, inspected: every
    /// instruction that reads a secret has no conditional branch, and reads
    /// or writes memory only in the stack frame, unless it is a load, store
    /// or call, whose address the guard has shown is public. Returns how
    /// many such instructions each function has.
    fn inspect_secret_code(src: &str, target: Target) -> HashMap<String, usize> {
        let module = lower(src, target).expect("it lowers");
        let marked = secrets(&module.functions).expect("the code is constant-time");
        let selected = target.trace(&module);
        let mut inspected: HashMap<String, usize> = HashMap::new();
        for ((function, marked), selected) in module.functions.iter().zip(&marked).zip(&selected) {
            for (inst, code) in function.insts.iter().zip(selected) {
                if !inst.uses().iter().any(|&t| marked[t as usize]) {
                    continue;
                }
                *inspected.entry(function.name.clone()).or_default() += 1;
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
        // The trace sees branches and loads where public values decide
        // them, so it would see them where a secret did.
        let all = selected.iter().flatten();
        assert!(all.clone().any(|code| code.conditional_branches > 0));
        assert!(all.clone().any(|code| code.other_accesses > 0));
        inspected
    }

    /// On aarch64 and on x86_64, for every target, the machine code of
    /// every secret operation is branch-free ([`inspect_secret_code`]).
    /// Each operation's function, and each `std.rt` function that handles
    /// secret bytes, is inspected.
    #[test]
    fn the_machine_code_of_every_secret_operation_is_branch_free() {
        for target in Target::ALL {
            let inspected = inspect_secret_code(SECRET_OPS, target);
            for name in SECRET_FUNCTIONS {
                assert!(
                    inspected.get(name).is_some_and(|&n| n > 0),
                    "{target:?}: nothing in `{name}` inspected: {inspected:?}"
                );
            }
        }
    }

    /// `std.hmac` (#232), Oska source linked into the program with the
    /// `std.sha2` it uses, builds for every target, and on aarch64 and
    /// x86_64 its machine code is branch-free and addresses no memory by
    /// a secret: the key's pads, the secret SHA-256 under them, and the
    /// comparison of the tag with the one given, whose loop over the bytes
    /// has no exit that depends on a byte (`std.rt.equal_constant`, which
    /// ORs the differences together and tests the total once).
    #[test]
    fn the_machine_code_of_std_hmac_is_branch_free() {
        let src = r#"use std.hmac

fn main(io: Stdout, env: Env, args: Args) {
    let key = env.secret("HMAC_KEY").to_bytes()
    let given = match args.get(1) {
        Some(text) => Bytes.from_hex(text).unwrap_or(Bytes.new())
        None => Bytes.new()
    }
    let valid = hmac.verify_sha256(key, "message".to_bytes(), given)
    let _ = io.println(if valid { "valid" } else { "invalid" })
}
"#;
        for target in Target::ALL {
            let inspected = inspect_secret_code(src, target);
            for name in [
                "pkg.std.hmac.padded",
                "pkg.std.sha2.compress256_secret",
                "pkg.std.sha2.Sha256Secret.finish",
                "std.rt.equal_constant",
            ] {
                assert!(
                    inspected.get(name).is_some_and(|&n| n > 0),
                    "{target:?}: nothing in `{name}` inspected: {inspected:?}"
                );
            }
        }
    }

    /// `std.sha2`'s secret SHA-256 (#231), Oska source linked into the
    /// program, builds for every target, and on aarch64 and x86_64 its
    /// machine code is branch-free and addresses no memory by a secret:
    /// the compression over secret words, its functions of a word, and
    /// the end of the hash, which reads the state. (The hasher's `update`
    /// moves secret bytes but reads none.)
    #[test]
    fn the_machine_code_of_std_sha2s_secret_hash_is_branch_free() {
        let src = r#"use std.sha2

fn main(io: Stdout) {
    let key = Secret.new("key material".to_bytes())
    var h = sha2.Sha256Secret.new()
    h.update(key)
    h.update(Secret.new(Bytes.zeroed(100)))
    let same = h.finish().constant_time_eq_secret(sha2.sha256_secret(key))
    let _ = io.println(if same { "same" } else { "different" })
}
"#;
        for target in Target::ALL {
            let inspected = inspect_secret_code(src, target);
            for name in [
                "pkg.std.sha2.compress256_secret",
                "pkg.std.sha2.big_sigma0_256_secret",
                "pkg.std.sha2.big_sigma1_256_secret",
                "pkg.std.sha2.small_sigma0_256_secret",
                "pkg.std.sha2.small_sigma1_256_secret",
                "pkg.std.sha2.Sha256Secret.finish",
            ] {
                assert!(
                    inspected.get(name).is_some_and(|&n| n > 0),
                    "{target:?}: nothing in `{name}` inspected: {inspected:?}"
                );
            }
        }
    }

    /// `std.p256` (#242), Oska source linked into the program with the
    /// `std.ecdsa` it uses to check a peer's point, builds for every
    /// target, and on aarch64 and x86_64 the machine code of its curve
    /// arithmetic over the secret scalar is branch-free and addresses no
    /// memory by a secret: the field operations, the complete point
    /// formulas (which the backend inlines into `multiply`), the choice of
    /// the sum or the point doubled for each bit, and the loop over the
    /// bits itself.
    #[test]
    fn the_machine_code_of_std_p256_is_branch_free() {
        let src = r#"use std.p256

fn main(io: Stdout, env: Env, args: Args) {
    let scalar = env.secret("P256_SCALAR").to_bytes()
    let peer = match args.get(1) {
        Some(text) => Bytes.from_hex(text).unwrap_or(Bytes.new())
        None => Bytes.new()
    }
    let same = match p256.shared_secret(scalar, peer) {
        Some(shared) => match p256.public_key(scalar) {
            Some(public) => shared.constant_time_eq_secret(public)
            None => false
        }
        None => false
    }
    let _ = io.println(if same { "same" } else { "different" })
}
"#;
        for target in Target::ALL {
            let inspected = inspect_secret_code(src, target);
            for name in [
                "pkg.std.p256_ops.mont_mul_mod",
                "pkg.std.p256_ops.add_mod_with",
                "pkg.std.p256_ops.sub_mod",
                "pkg.std.p256_ops.reduce_mod",
                "pkg.std.p256_ops.select",
                "pkg.std.p256_ops.multiply",
            ] {
                assert!(
                    inspected.get(name).is_some_and(|&n| n > 0),
                    "{target:?}: nothing in `{name}` inspected: {inspected:?}"
                );
            }
        }
    }

    /// `std.ecdsa.sign` (#828): the machine code of the signer over the
    /// secret key and the nonce is branch-free and addresses no memory by a
    /// secret, on every target that has a trace: the scalar multiplication
    /// of the nonce, the arithmetic mod the group order (`mont_mul_mod`
    /// and `add_mod_with`), the nonce's HMAC-SHA-256 generator and
    /// the verdict on whether the nonce was usable.
    #[test]
    fn the_machine_code_of_ecdsa_sign_is_branch_free() {
        let src = r#"use std.ecdsa

fn main(io: Stdout, env: Env, args: Args) {
    let key = env.secret("P256_SCALAR").to_bytes()
    let digest = match args.get(1) {
        Some(text) => Bytes.from_hex(text).unwrap_or(Bytes.new())
        None => Bytes.new()
    }
    let line = match ecdsa.sign(key, digest) {
        Some(signature) => signature.expose().to_hex()
        None => "no signature"
    }
    let _ = io.println(line)
}
"#;
        for target in Target::ALL {
            let inspected = inspect_secret_code(src, target);
            for name in [
                "pkg.std.p256_ops.mont_mul_mod",
                "pkg.std.p256_ops.add_mod_with",
                "pkg.std.p256_ops.multiply",
                "pkg.std.p256_ops.sign_with_nonce",
                "pkg.std.p256_ops.in_range_bit",
                "pkg.std.ecdsa.hmac_secret",
            ] {
                assert!(
                    inspected.get(name).is_some_and(|&n| n > 0),
                    "{target:?}: nothing in `{name}` inspected: {inspected:?}"
                );
            }
        }
    }

    /// `std.x25519` (#241), Oska source linked into the program, builds for
    /// every target, and on aarch64 and x86_64 its machine code is
    /// branch-free and addresses no memory by a secret: the ladder with its
    /// masked swap, the field arithmetic and the encoding of the result.
    #[test]
    fn the_machine_code_of_std_x25519_is_branch_free() {
        let src = r#"use std.x25519

fn main(io: Stdout, env: Env, args: Args) {
    let key = env.secret("KEY").to_bytes()
    let peer = match args.get(1) {
        Some(text) => Bytes.from_hex(text).unwrap_or(Bytes.new())
        None => Bytes.new()
    }
    let same = match x25519.scalarmult(key, peer) {
        Some(shared) => shared.constant_time_eq(peer)
        None => false
    }
    let _ = io.println(if same { "same" } else { "different" })
}
"#;
        for target in Target::ALL {
            let inspected = inspect_secret_code(src, target);
            for name in [
                "pkg.std.x25519.scalarmult",
                "pkg.std.x25519.cswap",
                "pkg.std.x25519.mul",
                "pkg.std.x25519.carry",
                "pkg.std.x25519.field_to_bytes",
            ] {
                assert!(
                    inspected.get(name).is_some_and(|&n| n > 0),
                    "{target:?}: nothing in `{name}` inspected: {inspected:?}"
                );
            }
        }
    }

    /// `std.hkdf` (#241): the HMAC over secret messages, and the loop that
    /// chains its blocks.
    #[test]
    fn the_machine_code_of_std_hkdf_is_branch_free() {
        let src = r#"use std.hkdf

fn main(io: Stdout, env: Env, args: Args) {
    let key = env.secret("KEY").to_bytes()
    let prk = hkdf.extract(key, key)
    let same = match hkdf.expand(prk, "info".to_bytes(), 42) {
        Some(okm) => okm.constant_time_eq(Bytes.zeroed(42))
        None => false
    }
    let _ = io.println(if same { "same" } else { "different" })
}
"#;
        for target in Target::ALL {
            let inspected = inspect_secret_code(src, target);
            for name in [
                "pkg.std.hkdf.padded",
                "pkg.std.hkdf.expand",
                "pkg.std.sha2.compress256_secret",
            ] {
                assert!(
                    inspected.get(name).is_some_and(|&n| n > 0),
                    "{target:?}: nothing in `{name}` inspected: {inspected:?}"
                );
            }
        }
    }

    /// `std.chacha20poly1305` (#241): the rounds of ChaCha20 over secret
    /// words, Poly1305's limbs and its final mask, and the tag comparison.
    #[test]
    fn the_machine_code_of_std_chacha20poly1305_is_branch_free() {
        let src = r#"use std.chacha20poly1305

fn main(io: Stdout, env: Env, args: Args) {
    let key = env.secret("KEY").to_bytes()
    let given = match args.get(1) {
        Some(text) => Bytes.from_hex(text).unwrap_or(Bytes.new())
        None => Bytes.new()
    }
    let nonce = Bytes.zeroed(12)
    let sealed = match chacha20poly1305.seal(key, nonce, "aad".to_bytes(), Secret.new("message".to_bytes())) {
        Some(out) => out
        None => Secret.new(Bytes.new())
    }
    let opens = match chacha20poly1305.open(key, nonce, "aad".to_bytes(), given) {
        Some(_) => true
        None => false
    }
    let _ = io.println(if opens && sealed.len() > 0 { "opens" } else { "refused" })
}
"#;
        for target in Target::ALL {
            let inspected = inspect_secret_code(src, target);
            for name in [
                "pkg.std.chacha20poly1305.block",
                "pkg.std.chacha20poly1305.quarter",
                "pkg.std.chacha20poly1305.poly1305",
                "pkg.std.chacha20poly1305.xor_stream",
                "std.rt.equal_constant",
            ] {
                assert!(
                    inspected.get(name).is_some_and(|&n| n > 0),
                    "{target:?}: nothing in `{name}` inspected: {inspected:?}"
                );
            }
        }
    }

    /// `std.aes128gcm` (#241): the bitsliced S-box and rounds, the key
    /// schedule, GHASH's masked multiplication, and the tag comparison.
    #[test]
    fn the_machine_code_of_std_aes128gcm_is_branch_free() {
        let src = r#"use std.aes128gcm

fn main(io: Stdout, env: Env, args: Args) {
    let key = env.secret("KEY").to_bytes()
    let given = match args.get(1) {
        Some(text) => Bytes.from_hex(text).unwrap_or(Bytes.new())
        None => Bytes.new()
    }
    let nonce = Bytes.zeroed(12)
    let sealed = match aes128gcm.seal(key, nonce, "aad".to_bytes(), Secret.new("message".to_bytes())) {
        Some(out) => out
        None => Secret.new(Bytes.new())
    }
    let opens = match aes128gcm.open(key, nonce, "aad".to_bytes(), given) {
        Some(_) => true
        None => false
    }
    let _ = io.println(if opens && sealed.len() > 0 { "opens" } else { "refused" })
}
"#;
        for target in Target::ALL {
            let inspected = inspect_secret_code(src, target);
            for name in [
                "pkg.std.aes128gcm.gf_mul",
                "pkg.std.aes128gcm.sub_bytes",
                "pkg.std.aes128gcm.shift_rows",
                "pkg.std.aes128gcm.mix_columns",
                "pkg.std.aes128gcm.next_key",
                "pkg.std.aes128gcm.ghash",
                "pkg.std.aes128gcm.word_mask",
                "std.rt.equal_constant",
            ] {
                assert!(
                    inspected.get(name).is_some_and(|&n| n > 0),
                    "{target:?}: nothing in `{name}` inspected: {inspected:?}"
                );
            }
        }
    }
}
