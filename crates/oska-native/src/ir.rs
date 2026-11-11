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
}

#[derive(Debug)]
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
    /// `dst =` the Linux system call `number` with `args` (at most 6): the
    /// kernel's raw result, negative `errno` on failure.
    Syscall {
        dst: Temp,
        number: u16,
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

/// The most arguments a call passes, all in registers. More need
/// arguments on the stack, which no lowering produces yet.
pub const MAX_ARGS: usize = 8;

impl Inst {
    /// The temporary it writes, if any.
    pub fn def(&self) -> Option<Temp> {
        match self {
            Inst::Const { dst, .. }
            | Inst::Copy { dst, .. }
            | Inst::Addr { dst, .. }
            | Inst::Binary { dst, .. }
            | Inst::Compare { dst, .. }
            | Inst::Load { dst, .. }
            | Inst::Call { dst, .. }
            | Inst::Syscall { dst, .. }
            | Inst::Arith { dst, .. }
            | Inst::Second { dst }
            | Inst::EntrySp { dst } => Some(*dst),
            _ => None,
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
            Inst::Load { base, .. } => vec![*base],
            Inst::Store { base, src, .. } => vec![*base, *src],
            Inst::Call { args, .. } | Inst::Syscall { args, .. } => args.clone(),
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
