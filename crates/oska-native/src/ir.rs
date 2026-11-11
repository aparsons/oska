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
    pub insts: Vec<Inst>,
}

/// Where an address points. The writable data gets a variant when the
/// runtime first keeps state there, such as the allocator's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Symbol {
    /// An offset into [`Module::rodata`].
    Rodata(u32),
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
    /// The target of jumps.
    Label(Label),
    Jump(Label),
    /// Jump to `target` when `cond` is 0.
    JumpIfZero {
        cond: Temp,
        target: Label,
    },
    /// Return `value` to the caller.
    Return(Temp),
}

/// The most arguments a call passes, all in registers. More need
/// arguments on the stack, which no lowering produces yet.
pub const MAX_ARGS: usize = 8;
