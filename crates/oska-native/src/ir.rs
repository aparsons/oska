//! The backend's own code, between the checked program and machine code.
//!
//! A [`Module`] is a list of functions over **temporaries**: numbered
//! 64-bit words, as many as a function needs. Every value is one word:
//! an integer or `Bool` itself, `()` as 0, a fieldless enum as its
//! variant's index, and anything else as the address of its object (see
//! `layout.rs`). Control flow is labels and jumps within a function.
//!
//! Nothing here is aarch64-specific, so a second target reuses it: only
//! `aarch64.rs` (instruction selection and encoding) and `elf.rs` (the
//! file) are per-target.
//!
//! To add an instruction: add a variant to [`Inst`], emit it from
//! `lower.rs`, and select machine code for it in `aarch64::Emitter::inst`.

/// A temporary: a word-sized local of the function. A function's
/// parameters are temporaries `0..params`.
pub type Temp = u32;

/// A jump target within a function.
pub type Label = u32;

/// A function, by its index in [`Module::functions`].
pub type FnId = usize;

/// A whole program, ready for instruction selection.
#[derive(Debug, Default)]
pub struct Module {
    pub functions: Vec<Function>,
    /// The function the process starts in. It never returns.
    pub entry: FnId,
    /// Read-only data: static objects, such as text literals. Each
    /// object starts at a multiple of 8.
    pub rodata: Vec<u8>,
    /// The size in bytes of the zero-initialised writable data.
    pub data_size: u32,
    /// Where in the writable data the arguments of a call past the
    /// [`REGISTER_ARGS`]th go ([`EXTRA_ARGS`] words of them).
    pub extra_args: u32,
}

#[derive(Debug, Clone)]
pub struct Function {
    /// The qualified name, for reading the code: `main`, `std.rt.exit`.
    pub name: String,
    /// How many arguments it takes: temporaries `0..params` hold them.
    pub params: u32,
    /// How many temporaries it uses, its parameters included.
    pub temps: u32,
    /// How many labels it uses: labels are `0..labels`.
    pub labels: u32,
    /// The code: its hot path, then, from [`Function::cold`] on, its cold
    /// blocks.
    pub insts: Vec<Inst>,
    /// Where the cold blocks start in [`Function::insts`]: out-of-line
    /// code, such as a panic, that the hot path jumps to and never comes
    /// back from. Each cold block starts with its label, has exactly one
    /// jump to it, and ends where the next starts; it reads the hot
    /// path's temporaries as they were at that jump.
    pub cold: usize,
}

/// Where an address points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Symbol {
    /// An offset into [`Module::rodata`].
    Rodata(u32),
    /// An offset into the zero-initialised writable data, where the
    /// runtime keeps its state, such as the allocator's free lists.
    Data(u32),
    /// The code of a function: where a function value's code pointer
    /// points ([`Inst::CallIndirect`]).
    Function(FnId),
}

/// An integer type, as checked arithmetic sees it: its width in bits and
/// whether it is signed. A value of it is held in a word sign- or
/// zero-extended, as `layout.rs` says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntKind {
    pub signed: bool,
    /// 8, 16, 32 or 64.
    pub bits: u32,
}

/// An arithmetic operation that is checked against its [`IntKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arith {
    Add,
    Sub,
    Mul,
    /// Division, truncating toward zero. Its divisor is never 0: the
    /// lowering branches away first.
    Div,
    /// The remainder of [`Arith::Div`], with the dividend's sign.
    Rem,
}

/// The width of a memory access. A load zero-extends into the word; a
/// store writes the low bits of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Width {
    W8,
    W16,
    W32,
    W64,
}

/// A two-operand word operation. Arithmetic wraps; shifts take their
/// amount modulo 64.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    And,
    Or,
    Xor,
    /// Shift left.
    Shl,
    /// Logical shift right.
    Shr,
    /// Arithmetic shift right.
    Sar,
}

/// A comparison of two words, as signed integers or, for the `Below` and
/// `Above` forms, as unsigned ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cond {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Below,
    BelowEq,
    Above,
    AboveEq,
}

impl Cond {
    /// The condition that holds exactly when this one does not.
    pub fn negated(self) -> Cond {
        match self {
            Cond::Eq => Cond::Ne,
            Cond::Ne => Cond::Eq,
            Cond::Lt => Cond::Ge,
            Cond::Ge => Cond::Lt,
            Cond::Le => Cond::Gt,
            Cond::Gt => Cond::Le,
            Cond::Below => Cond::AboveEq,
            Cond::AboveEq => Cond::Below,
            Cond::BelowEq => Cond::Above,
            Cond::Above => Cond::BelowEq,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inst {
    /// `dst = value`.
    Const {
        dst: Temp,
        value: i64,
    },
    /// `dst = src`.
    Copy {
        dst: Temp,
        src: Temp,
    },
    /// `dst =` the run-time address of `symbol`, found relative to the
    /// code, so the program can be loaded anywhere.
    Addr {
        dst: Temp,
        symbol: Symbol,
    },
    /// `dst = lhs op rhs`.
    Binary {
        op: BinOp,
        dst: Temp,
        lhs: Temp,
        rhs: Temp,
    },
    /// `dst = lhs op imm`, for `Add` and `Sub` with `imm` from 0 to 4095,
    /// and the shifts with `imm` from 0 to 63 (`optimize.rs` makes it).
    BinaryImm {
        op: BinOp,
        dst: Temp,
        lhs: Temp,
        imm: i64,
    },
    /// [`Inst::Branch`] on `lhs cond imm`, `imm` from -4095 to 4095.
    BranchImm {
        cond: Cond,
        lhs: Temp,
        imm: i64,
        target: Label,
    },
    /// [`Inst::Compare`] of `lhs` with `imm`, from -4095 to 4095.
    CompareImm {
        cond: Cond,
        dst: Temp,
        lhs: Temp,
        imm: i64,
    },
    /// [`Inst::Arith`] `Add` or `Sub` of `imm`, from 0 to 4095.
    ArithImm {
        op: Arith,
        int: IntKind,
        dst: Temp,
        lhs: Temp,
        imm: i64,
        overflow: Label,
    },
    /// `dst = 1` if `lhs cond rhs` holds, else 0.
    Compare {
        cond: Cond,
        dst: Temp,
        lhs: Temp,
        rhs: Temp,
    },
    /// `dst =` the `width` bits at address `base + offset`.
    Load {
        width: Width,
        dst: Temp,
        base: Temp,
        offset: u32,
    },
    /// Write the low `width` bits of `src` at address `base + offset`.
    Store {
        width: Width,
        base: Temp,
        offset: u32,
        src: Temp,
    },
    /// `dst = callee(args...)`. At most [`MAX_ARGS`] arguments.
    Call {
        dst: Temp,
        callee: FnId,
        args: Vec<Temp>,
    },
    /// `dst =` the function whose code is at the address `target` (made by
    /// an [`Inst::Addr`] of a [`Symbol::Function`]), called with `args`:
    /// a call of a function value. At most [`MAX_ARGS`] arguments.
    CallIndirect {
        dst: Temp,
        target: Temp,
        args: Vec<Temp>,
    },
    /// `dst =` the Linux system call whose number is `args[0]`, with the
    /// rest of `args` (at most 6): the kernel's raw result, negative
    /// `errno` on failure. The number is the target platform's, from
    /// `std.rt`'s table.
    Syscall {
        dst: Temp,
        args: Vec<Temp>,
    },
    /// `dst = lhs op rhs` when the result fits `int`; otherwise jump to
    /// `overflow`, leaving `dst` unspecified.
    Arith {
        op: Arith,
        int: IntKind,
        dst: Temp,
        lhs: Temp,
        rhs: Temp,
        overflow: Label,
    },
    /// The target of jumps.
    Label(Label),
    Jump(Label),
    /// Jump to `target` when `cond` is 0.
    JumpIfZero {
        cond: Temp,
        target: Label,
    },
    /// Jump to `target` when `lhs cond rhs` holds.
    Branch {
        cond: Cond,
        lhs: Temp,
        rhs: Temp,
        target: Label,
    },
    /// Count one more call nesting: jump to `exhausted` instead when calls
    /// already nest as deep as the program allows. Paired with
    /// [`Inst::Leave`] after the call.
    Enter {
        exhausted: Label,
    },
    /// Count one call nesting less.
    Leave,
    /// Switch to the stack whose top is the address `stack`, allow calls
    /// to nest `depth` deep from here, and call `callee`, which never
    /// returns. Only the entry point uses it.
    CallOnStack {
        stack: Temp,
        depth: u32,
        callee: FnId,
    },
    /// Return `value` to the caller.
    Return(Temp),
    /// Return `value` and a second result, `second`: how a mutating
    /// method gives back its receiver as it left it.
    ReturnPair {
        value: Temp,
        second: Temp,
    },
    /// `dst =` the second result of the [`Inst::Call`] just before it,
    /// which called a function that returns with [`Inst::ReturnPair`].
    Second {
        dst: Temp,
    },
    /// `dst =` the stack pointer the kernel started the process with
    /// (argc, then argv, then envp). Only valid in `_start`, before it
    /// switches stacks.
    EntrySp {
        dst: Temp,
    },
}

/// The most temporaries a function may use: each has a stack slot
/// addressed by a 12-bit scaled offset.
pub const MAX_TEMPS: u32 = 4095;

/// How many arguments a call passes in registers. The caller stores the
/// rest in [`Module::extra_args`] just before it calls, and the callee
/// loads them on entry, before anything else runs.
pub const REGISTER_ARGS: usize = 8;

/// The most arguments past [`REGISTER_ARGS`] a call may pass: each is
/// addressed by a 12-bit scaled offset.
pub const EXTRA_ARGS: usize = 4095;

/// The most arguments a call passes (a function value's object among
/// them).
pub const MAX_ARGS: usize = REGISTER_ARGS + EXTRA_ARGS;

impl Inst {
    /// The label it may jump to, if any.
    pub fn target(&self) -> Option<Label> {
        match self {
            Inst::Jump(label) => Some(*label),
            Inst::JumpIfZero { target, .. }
            | Inst::Branch { target, .. }
            | Inst::BranchImm { target, .. } => Some(*target),
            Inst::Arith { overflow, .. } | Inst::ArithImm { overflow, .. } => Some(*overflow),
            Inst::Enter { exhausted } => Some(*exhausted),
            _ => None,
        }
    }

    /// The temporary it writes, if any.
    pub fn def(&self) -> Option<Temp> {
        match self {
            Inst::Const { dst, .. }
            | Inst::Copy { dst, .. }
            | Inst::Addr { dst, .. }
            | Inst::Binary { dst, .. }
            | Inst::BinaryImm { dst, .. }
            | Inst::CompareImm { dst, .. }
            | Inst::ArithImm { dst, .. }
            | Inst::Compare { dst, .. }
            | Inst::Load { dst, .. }
            | Inst::Call { dst, .. }
            | Inst::CallIndirect { dst, .. }
            | Inst::Syscall { dst, .. }
            | Inst::Arith { dst, .. }
            | Inst::Second { dst }
            | Inst::EntrySp { dst } => Some(*dst),
            _ => None,
        }
    }

    /// Replace every temporary it writes or reads, `t`, by `map(t)`.
    pub fn map_temps(&mut self, map: impl Fn(Temp) -> Temp) {
        let each = |t: &mut Temp| *t = map(*t);
        match self {
            Inst::Const { dst, .. }
            | Inst::Addr { dst, .. }
            | Inst::Second { dst }
            | Inst::EntrySp { dst } => each(dst),
            Inst::Copy { dst, src } => {
                each(dst);
                each(src);
            }
            Inst::Binary { dst, lhs, rhs, .. }
            | Inst::Compare { dst, lhs, rhs, .. }
            | Inst::Arith { dst, lhs, rhs, .. } => {
                each(dst);
                each(lhs);
                each(rhs);
            }
            Inst::BinaryImm { dst, lhs, .. }
            | Inst::CompareImm { dst, lhs, .. }
            | Inst::ArithImm { dst, lhs, .. } => {
                each(dst);
                each(lhs);
            }
            Inst::Branch { lhs, rhs, .. } => {
                each(lhs);
                each(rhs);
            }
            Inst::BranchImm { lhs, .. } => each(lhs),
            Inst::Load { dst, base, .. } => {
                each(dst);
                each(base);
            }
            Inst::Store { base, src, .. } => {
                each(base);
                each(src);
            }
            Inst::Call { dst, args, .. } | Inst::Syscall { dst, args, .. } => {
                each(dst);
                args.iter_mut().for_each(each);
            }
            Inst::CallIndirect { dst, target, args } => {
                each(dst);
                each(target);
                args.iter_mut().for_each(each);
            }
            Inst::JumpIfZero { cond, .. } => each(cond),
            Inst::CallOnStack { stack, .. } => each(stack),
            Inst::Return(value) => each(value),
            Inst::ReturnPair { value, second } => {
                each(value);
                each(second);
            }
            Inst::Label(_) | Inst::Jump(_) | Inst::Enter { .. } | Inst::Leave => {}
        }
    }

    /// The temporaries it reads, in order.
    pub fn uses(&self) -> Vec<Temp> {
        match self {
            Inst::Copy { src, .. } => vec![*src],
            Inst::Binary { lhs, rhs, .. }
            | Inst::Compare { lhs, rhs, .. }
            | Inst::Arith { lhs, rhs, .. }
            | Inst::Branch { lhs, rhs, .. } => vec![*lhs, *rhs],
            Inst::BinaryImm { lhs, .. }
            | Inst::BranchImm { lhs, .. }
            | Inst::CompareImm { lhs, .. }
            | Inst::ArithImm { lhs, .. } => vec![*lhs],
            Inst::Load { base, .. } => vec![*base],
            Inst::Store { base, src, .. } => vec![*base, *src],
            Inst::Call { args, .. } | Inst::Syscall { args, .. } => args.clone(),
            Inst::CallIndirect { target, args, .. } => std::iter::once(*target)
                .chain(args.iter().copied())
                .collect(),
            Inst::JumpIfZero { cond, .. } => vec![*cond],
            Inst::CallOnStack { stack, .. } => vec![*stack],
            Inst::Return(value) => vec![*value],
            Inst::ReturnPair { value, second } => vec![*value, *second],
            Inst::Const { .. }
            | Inst::Second { .. }
            | Inst::EntrySp { .. }
            | Inst::Addr { .. }
            | Inst::Label(_)
            | Inst::Jump(_)
            | Inst::Enter { .. }
            | Inst::Leave => Vec::new(),
        }
    }
}
