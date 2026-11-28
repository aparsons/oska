// SPDX-License-Identifier: Apache-2.0
//! Lowering: the checked program, and `std.rt`'s, to the backend's code
//! ([`crate::ir`]).
//!
//! Lowering starts at the entry point it generates, `_start`, and lowers
//! each function the first time something calls it, so only reachable
//! code is compiled: `std.rt` is compiled with the program, function by
//! function, as the program needs it. A function is an [`Instance`]: the
//! program it comes from, its name, and the concrete types its generics
//! stand for. A generic function is compiled once for each list of type
//! arguments a reachable call gives it (monomorphization): its body is
//! lowered with every type substituted ([`Body::ty`]), so the counting
//! and layout of each value is that of a concrete type. The backend also
//! generates functions per type: a drop function and an unshare function
//! (`data.rs`).
//!
//! Anything the backend cannot compile yet is refused with `E0390`, at the
//! span of the construct (for code in `std.rt`, the span of the user code
//! that reached it), so a program is never half compiled.
//!
//! To support a construct: lower it in [`Body::expr`] (or [`Body::stmt`])
//! instead of refusing it, and add a differential test that runs it.
//!
//! # Reference counts
//!
//! Every value of a counted type (`layout::is_counted`) that an
//! expression produces holds a reference of its own (its temporary is in
//! [`Body::owned`]), or is a static object, which needs none. Whoever
//! consumes it releases it ([`Body::consume`]): a call releases its
//! arguments after it returns, a statement its value. A local takes over
//! its value's reference and is released when its scope ends, or before
//! a `return`, `break` or `continue` leaves it; reading it retains. A
//! function's parameters are borrowed from the caller, which holds them
//! for the call, so they are never released by the callee, and so is a
//! method's receiver, unless the method changes it (`data.rs`). The
//! retain and release fast paths are inline; the release that drops a
//! count to zero calls the type's drop function, which releases its
//! fields, or `std.rt`'s `free` directly when no field holds a reference.
//! A `match` holds its scrutinee while its arms run ([`Held`]).
//!
//! # Panics
//!
//! A check that fails, such as an overflow, jumps to a cold block at the
//! end of the function that calls one of `std.rt`'s `panic` functions
//! with its report's text: the location (`path:line:col: error[P....]:`)
//! and the span are resolved here, at compile time, into static text
//! ([`Body::report`]), the only part made at run time is the values in
//! the message. To add a panic site, branch to a block built with
//! [`Body::panic_static`], [`Body::panic_unary`] or
//! [`Body::panic_binary`].
//!
//! # Calls and the stack
//!
//! `_start` maps the stack `main` runs on (`std.rt`'s `stack_new`): big
//! enough for [`MAX_CALL_DEPTH`] calls of the program's largest frame
//! (and of the largest generated frame between two calls: a `requires`
//! check, a thunk, a task runner),
//! with a guard region below it. Every call of a program's function
//! counts its depth ([`Inst::Enter`]) and panics with `P0021` at the
//! interpreter's limit, so recursion never reaches the guard.
//!
//! # Steps
//!
//! Under `oskac run --max-steps` (ADR 0021), each call of a function,
//! method or closure of the program, and each loop back-edge in it, is a
//! step ([`Body::step`]): a counter after the runtime's globals counts
//! down from the limit, and the step with none left panics with `P0038`
//! at the interpreter's span, before the call's depth is counted, as the
//! interpreter's `enter` takes it. A call of a function value reads from
//! the value whether it is a step (`layout::FN_NO_STEP`), since a prelude
//! method used as a value is none. Nothing in `std.rt` is a step. Without the flag none of this is generated, so the
//! executable is byte for byte the one `oskac build` writes.

mod bits;
mod branch;
mod bytes;
mod calls;
mod closures;
mod contracts;
mod data;
mod debug;
mod expr;
mod float;
mod html;
mod int_methods;
mod interpolation;
mod lists;
mod ops;
mod panic_sites;
mod patterns;
mod refs;
mod runtime;
mod select;
mod steps;
mod stmts;
#[cfg(test)]
pub(crate) use select::BRANCHY;
mod lowering;
#[cfg(test)]
mod step_tests;
mod tasks;
mod tests;
mod traits;

use std::collections::{BTreeMap, HashMap};

use oska_check::builtins;
use oska_check::limits::{Limit, Limits};
use oska_check::program::{
    secrets, trait_method, Callee, Construct, Expr, ExprKind, Function, IntType, IntrinsicName,
    Part, Program, Stmt, Type,
};
use oska_syntax::ast::BinaryOp;
use oska_syntax::{Diagnostic, Frame, Span, MAX_FRAMES};

use crate::intrinsics::{self, Check, ErrorRead, Fails, Import, OsCall, Prelude, Runtime, Start};
use crate::ir::{self, Arith, BinOp, Cond, FloatOp, FnId, Inst, Label, Symbol, Temp, Width};
use crate::layout::{self, Rodata};
use crate::optimize;
use crate::{codes, Locate, Target, NOWHERE};

/// How deeply calls of the program's functions may nest before it
/// panics with `P0021`: the interpreter's limit (`oska_run`'s
/// `MAX_CALL_DEPTH`), so both panic at the same depth.
pub const MAX_CALL_DEPTH: u32 = 50_000;

/// The stack beyond [`MAX_CALL_DEPTH`] frames: room for `std.rt`'s calls
/// below the deepest frame, and for the entry point's.
const STACK_RESERVE: u64 = 8 << 20;

/// `std.rt`'s allocator functions, each with the one that calls it and
/// counts the bytes against `--max-memory` (`P0040`): under the limit,
/// every request of the first is one of the second.
const COUNTED_MEMORY: [(&str, &str); 3] = [
    ("alloc", "counted_alloc"),
    ("try_alloc", "counted_try_alloc"),
    ("free", "counted_free"),
];

/// The words of the runtime's writable data, at `rt_mem_globals()`.
pub const GLOBALS: u32 = 54;

/// The runtime's writable word that holds the address of the call stack
/// (`std.rt`'s "The call stack"), where each counted call stores the
/// record of its frame ([`Body::record_call`]).
const FRAMES_WORD: u32 = 50;

/// Where the records start in the call stack: after its texts, its most
/// and its room for digits.
const FRAMES_RECORDS: u32 = 7 * layout::WORD;

/// Under `--max-steps`, the word after the runtime's globals holds the
/// steps the program may still take ([`Body::step`]); without the flag
/// the word is not there, so the writable data is what it always was.
const STEPS_WORD: u32 = GLOBALS;

/// Under `--max-memory`, the word after the step counter's holds the
/// report of the panic (`P0040`) that `std.rt`'s memory limit stops the
/// program with; without the flag the word is not there. It is a fixed
/// word, 55, so that `std.rt` can name it whether or not the step counter
/// is there too.
const MEMORY_REPORT_WORD: u32 = GLOBALS + 1;

/// What an executable runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    /// The program's `main`.
    Main,
    /// One of the program's `test` blocks, chosen by its argument
    /// (`tests.rs`).
    Tests,
}

/// The `P`-codes the native code panics with, as the interpreter's
/// `oska_run::codes` names them.
mod panics {
    pub const OVERFLOW: &str = "P0001";
    pub const INDEX_OUT_OF_BOUNDS: &str = "P0002";
    pub const DIVISION_BY_ZERO: &str = "P0004";
    pub const SLICE_OUT_OF_BOUNDS: &str = "P0035";
    pub const NEGATIVE_REPEAT: &str = "P0036";
    pub const TEXT_TOO_LONG: &str = "P0037";
    pub const BYTES_OUT_OF_RANGE: &str = "P0042";
    pub const BYTES_TOO_LONG: &str = "P0043";
    pub const MAP_ENTRY_LIMIT: &str = "P0044";
    pub use oska_check::limits::messages::MAP_ENTRY_LIMIT as MAP_ENTRY_LIMIT_MESSAGE;
    pub const MAP_PROBE_LIMIT: &str = "P0045";
    pub use oska_check::limits::messages::MAP_PROBE_LIMIT as MAP_PROBE_LIMIT_MESSAGE;
    pub const FLOAT_TO_INT: &str = "P0046";
    pub const INT_TO_FLOAT: &str = "P0049";
    pub const FMT_DIGITS: &str = "P0050";
    pub const DEQUE_ITEM_LIMIT: &str = "P0051";
    pub use oska_check::limits::messages::DEQUE_ITEM_LIMIT as DEQUE_ITEM_LIMIT_MESSAGE;
    pub const CALL_DEPTH_EXCEEDED: &str = "P0021";
    pub const TODO: &str = "P0003";
    pub const SPAWN_ON_FINISHED_SCOPE: &str = "P0005";
    /// The interpreter's message for `P0005`.
    pub const FINISHED_SCOPE: &str = "`spawn` was called on a scope whose block has already ended";
    pub const MISSING_SECRET: &str = "P0020";
    /// The interpreter's message for `P0003`.
    pub const NOT_WRITTEN: &str = "reached `todo()`: this code is not written yet";
    pub const PRECONDITION_VIOLATED: &str = "P0006";
    pub const POSTCONDITION_VIOLATED: &str = "P0007";
    pub const SHIFT_OUT_OF_RANGE: &str = "P0008";
    pub const ASSERT_FAILED: &str = "P0010";
    pub const ASSERT_EQ_FAILED: &str = "P0011";
    pub const ASSERT_NE_FAILED: &str = "P0012";
    pub const TEST_FAILED: &str = "P0013";
    /// The interpreter's message for `P0021`.
    pub const TOO_DEEP: &str = "calls nest too deep: is this recursion missing its base case?";
}

/// Which checked program a function comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Source {
    /// The program being built.
    User,
    /// `std.rt`.
    Runtime,
}

/// A function to compile: one of a program's, specialised for its type
/// arguments, or one the backend generates for a type.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Instance {
    /// A function of `source`, by the qualified name [`Program::functions`]
    /// keys it by, with concrete types for its generics, in the order of
    /// [`Function::generics`] (empty for a function that has none).
    Function {
        source: Source,
        name: String,
        type_args: Vec<Type>,
    },
    /// Release the fields of an object of this type, of `source`'s
    /// [`Program::types`], that hold references, then free it: what
    /// releasing its last reference calls.
    Drop(Source, Type),
    /// A copy of a shared struct object of this type, whose count is not
    /// 1, holding a reference of its own to each field.
    Unshare(Source, Type),
    /// Whether two lists of items of this type are equal (`List.eq`).
    /// Under `--max-steps`, one for each `==` that compares them, at the
    /// span its calls of a user `Eq` are steps at ([`Body::step_site`]).
    ListEq(Source, Type, Option<Span>),
    /// The text `dbg` shows a value of this struct or enum type as.
    Debug(Source, Type),
    /// Whether two values of this `Option` or `Result` type are equal,
    /// through `Eq` for each field's type (`traits.rs`). Under
    /// `--max-steps`, one for each `==`, as [`Instance::ListEq`].
    Equal(Source, Type, Option<Span>),
    /// The hash of a value of this `List`, `Option`, or struct or enum
    /// type, which derives `Hash` or has the prelude's (`traits.rs`). Under
    /// `--max-steps`, as [`Instance::Equal`].
    Hash(Source, Type, Option<Span>),
    /// How two values of this `Option` or `List` type order through `Ord`
    /// for the item's type, as -1, 0 or 1 (`traits.rs`). Under
    /// `--max-steps`, as [`Instance::Equal`].
    Compare(Source, Type, Option<Span>),
    /// The check of a function's `requires` clauses, a function of the
    /// same parameters as it: gives 0 when they all hold, else the number
    /// (from 1) of the first that is false. The caller panics with it,
    /// for the span of its call is the report's.
    Requires {
        source: Source,
        name: String,
        type_args: Vec<Type>,
    },
    /// The code of a closure, [`Lowering::closures`]'s `index`th
    /// (`closures.rs`).
    Closure(usize),
    /// What frees a function value of a closure that captured references:
    /// releases them, then frees the object.
    ClosureDrop(usize),
    /// The code of a function value of type `ty` that calls the program's
    /// function `name` or, when `intrinsic`, the prelude intrinsic `name`,
    /// with the concrete `type_args`.
    Thunk {
        source: Source,
        intrinsic: bool,
        name: String,
        type_args: Vec<Type>,
        ty: Type,
    },
    /// The body of the program's `index`th `test` block, a function of
    /// `t` (`tests.rs`).
    Test(usize),
    /// What runs a task whose function value gives a `T`, spawned by the
    /// call at bytes `start..end`: calls the value, keeps its result and
    /// lets go of the queue's reference (`tasks.rs`).
    TaskRunner {
        result: Type,
        start: usize,
        end: usize,
    },
    /// Run the queued tasks of a scope (its parameter) until the task that
    /// is its second parameter has run, or all of them when that is 0.
    RunTasks,
}

/// Lower `user`, with `rt`, the checked `std.rt`, to a module. `locate`
/// gives a user span its file, line and column, for panic reports.
pub fn program(
    user: &Program,
    rt: &Program,
    locate: &dyn Locate,
    kind: Entry,
    target: Target,
    limits: Limits,
    dns_test_hooks: bool,
) -> Result<ir::Module, Diagnostic> {
    let mut lowering = Lowering {
        user,
        rt,
        locate,
        kind,
        target,
        limits,
        dns_test_hooks,
        uncounted: false,
        human: locate.places(),
        frame: "main".to_string(),
        ids: HashMap::new(),
        functions: Vec::new(),
        queue: Vec::new(),
        rodata: Rodata::default(),
        user_functions: Vec::new(),
        between_functions: Vec::new(),
        closures: Vec::new(),
        closure_ids: HashMap::new(),
        callback: None,
    };
    let entry = lowering.entry()?;
    lowering.drain_queue()?;
    optimize::module(&mut lowering.functions);
    // The code is final: no secret may decide a branch or an address.
    crate::constant_time::verify(&lowering.functions)?;
    let stack = lowering.size_stack(entry);
    // The globals, then the step counter under `--max-steps`, then the
    // memory limit's report under `--max-memory` (which keeps the counter's
    // word, unused without `--max-steps`, so that its own word is fixed).
    let globals = if limits.max_memory.is_some() {
        MEMORY_REPORT_WORD + 1
    } else {
        GLOBALS + u32::from(limits.max_steps.is_some())
    };
    let extra_args = lowering.extra_args();
    Ok(ir::Module {
        functions: lowering.functions,
        entry,
        rodata: lowering.rodata.into_bytes(),
        data_size: (globals + extra_args) * layout::WORD,
        extra_args: globals * layout::WORD,
        callback: lowering.callback,
        stack,
    })
}

impl Lowering<'_> {
    /// Lower every queued instance, which may queue more, and note the
    /// user's functions and the generated ones that run between counted
    /// calls (for the stack's size).
    fn drain_queue(&mut self) -> Result<(), Diagnostic> {
        while let Some((instance, id, origin, requester)) = self.queue.pop() {
            self.frame = self.frame_of(&instance, requester);
            self.uncounted = matches!(
                &instance,
                Instance::Function { source: Source::Runtime, name, .. }
                    if COUNTED_MEMORY.iter().any(|(_, counted)| counted == name)
            );
            self.functions[id] = self.function(&instance, origin)?;
            if matches!(
                instance,
                Instance::Function {
                    source: Source::User,
                    ..
                } | Instance::Closure(_)
                    | Instance::Test(_)
            ) {
                self.user_functions.push(id);
            }
            if matches!(
                instance,
                Instance::Requires { .. }
                    | Instance::TaskRunner { .. }
                    | Instance::RunTasks
                    | Instance::Thunk { .. }
                    | Instance::Equal(..)
                    | Instance::Hash(..)
                    | Instance::Compare(..)
                    | Instance::ListEq(..)
            ) {
                self.between_functions.push(id);
            }
        }
        Ok(())
    }

    /// The most arguments any call passes, or any function takes, past
    /// those the target passes in registers: they share one area after
    /// the globals.
    fn extra_args(&self) -> u32 {
        self.functions
            .iter()
            .flat_map(|function| {
                let calls = function.insts.iter().filter_map(|inst| match inst {
                    Inst::Call { args, .. } | Inst::CallIndirect { args, .. } => Some(args.len()),
                    _ => None,
                });
                calls.chain([function.params as usize])
            })
            .map(|count| count.saturating_sub(self.target.register_args()))
            .max()
            .unwrap_or(0) as u32
    }
}

struct Lowering<'p> {
    /// [`ir::Module::callback`].
    callback: Option<FnId>,
    user: &'p Program,
    rt: &'p Program,
    locate: &'p dyn Locate,
    /// What the executable runs: a test executable's `std.rt` reaches its
    /// test host (`rt_mem_test_host`), a program's never does.
    kind: Entry,
    /// The target platform: its system-call numbers and `std.rt` table,
    /// and how many arguments its calls pass in registers.
    target: Target,
    /// The run limits the program is built under (`oskac run`): none for
    /// `oskac build` and test executables.
    limits: Limits,
    /// Whether `std.dns` in the executable honors the `OSKA_DNS_*` test
    /// variables (`rt_mem_dns_test_hooks`).
    dns_test_hooks: bool,
    /// Whether the function being lowered is one of `std.rt`'s counting
    /// allocator functions ([`COUNTED_MEMORY`]), whose own calls of the
    /// allocator are not redirected.
    uncounted: bool,
    /// Whether panic reports are for a person, placed in the source by
    /// `locate`, or for `oskac` to read back (`crate::test_failure`).
    human: bool,
    /// The name of the frame of the function being lowered, as a panic's
    /// call stack names it ([`Body::frame_text`]).
    frame: String,
    /// Every function requested so far.
    ids: HashMap<Instance, FnId>,
    /// The lowered functions, by id; a requested one is a placeholder
    /// until it is lowered.
    functions: Vec<ir::Function>,
    /// Functions requested and not lowered yet, with the span of the user
    /// code that first reached them, and the frame of the function that
    /// did.
    queue: Vec<(Instance, FnId, Span, String)>,
    rodata: Rodata,
    /// The program's own functions (not `std.rt`'s): their frames are
    /// those recursion can stack up.
    user_functions: Vec<FnId>,
    /// The generated functions that run between two counted calls of the
    /// program's: a `requires` check, a task's runner and the queue that
    /// runs it, a generated `==` reaching a program's `eq`. One of their
    /// frames may sit on each counted call's.
    between_functions: Vec<FnId>,
    /// Every closure's code requested so far, by [`Instance::Closure`]'s
    /// index.
    closures: Vec<closures::ClosureCode>,
    closure_ids: HashMap<closures::ClosureKey, usize>,
}

/// How a function is named in the code, for reading it: `name`, with
/// its type arguments as `name<Int, String>`, `std.rt.` before a
/// function of the runtime's.
fn qualified(instance: &Instance) -> String {
    match instance {
        Instance::Function {
            source,
            name,
            type_args,
        } => {
            let prefix = match source {
                Source::User => "",
                Source::Runtime => "std.rt.",
            };
            if type_args.is_empty() {
                format!("{prefix}{name}")
            } else {
                let args: Vec<String> = type_args.iter().map(Type::to_string).collect();
                format!("{prefix}{name}<{}>", args.join(", "))
            }
        }
        Instance::Drop(_, ty) => format!("drop<{ty}>"),
        Instance::Unshare(_, ty) => format!("unshare<{ty}>"),
        Instance::ListEq(_, ty, _) => format!("list_eq<{ty}>"),
        Instance::Debug(_, ty) => format!("debug<{ty}>"),
        Instance::Equal(_, ty, _) => format!("eq<{ty}>"),
        Instance::Hash(_, ty, _) => format!("hash<{ty}>"),
        Instance::Compare(_, ty, _) => format!("cmp<{ty}>"),
        Instance::Requires {
            source,
            name,
            type_args,
        } => {
            let function = Instance::Function {
                source: *source,
                name: name.clone(),
                type_args: type_args.clone(),
            };
            format!("requires of {}", qualified(&function))
        }
        Instance::TaskRunner { start, end, .. } => format!("task runner at {start}..{end}"),
        Instance::RunTasks => "run tasks".to_string(),
        Instance::Closure(index) => format!("closure #{index}"),
        Instance::Test(index) => format!("test #{index}"),
        Instance::ClosureDrop(index) => format!("drop of closure #{index}"),
        Instance::Thunk {
            name, type_args, ..
        } => {
            let args: Vec<String> = type_args.iter().map(Type::to_string).collect();
            format!("fn value {name}<{}>", args.join(", "))
        }
    }
}

/// Where a body's value is returned from when it ends without a `return`:
/// its last expression, or else its closing brace.
fn body_end(body: &Expr) -> Span {
    match &body.kind {
        ExprKind::Block {
            value: Some(value), ..
        } => value.span,
        _ => Span {
            start: body.span.end.saturating_sub(1),
            end: body.span.end,
        },
    }
}

/// The refusal of `what`, at `span`. `in_runtime` says the construct is
/// in `std.rt` and `span` is the user code that reached it: a gap in the
/// runtime, not in the program.
fn refusal(span: Span, what: &str, in_runtime: bool) -> Diagnostic {
    let place = if in_runtime {
        " (it is in the runtime, `std.rt`)"
    } else {
        ""
    };
    Diagnostic::new(
        codes::UNSUPPORTED,
        format!("`oskac build` cannot compile {what} yet{place}"),
        span,
    )
}

/// A local slot in scope.
#[derive(Debug, Clone)]
struct Slot {
    temp: Temp,
    /// Whether it holds a reference of its own, released when it goes out
    /// of scope: a counted local's, not a parameter's.
    release: bool,
    /// Its type, concrete.
    ty: Type,
}

/// A method's receiver.
#[derive(Debug, Clone)]
struct Receiver {
    temp: Temp,
    /// Whether the method owns it: a mutating method's, which may change
    /// it and gives it back to the caller with its result. Any other
    /// method's is borrowed, as a parameter is.
    owned: bool,
    /// `Self`, concrete.
    ty: Type,
}

/// A value an expression holds a reference to while code that may leave
/// it runs, such as a `match`'s scrutinee while its arms run: a
/// `return`, or a `break` or `continue` out of a loop it is inside,
/// releases it.
#[derive(Debug, Clone)]
struct Held {
    temp: Temp,
    ty: Type,
    /// How many loops were open where it was taken.
    loops: usize,
    /// Whether it is the handle of a `scope` block, whose queued tasks run
    /// and which is closed when code leaves the block (`tasks.rs`).
    scope: bool,
}

/// A loop being lowered.
#[derive(Debug, Clone, Copy)]
struct Loop {
    /// Where `continue` jumps.
    next: Label,
    /// Where `break` jumps.
    exit: Label,
    /// How many slots were in scope outside it: those above are released
    /// when `break` or `continue` leaves them.
    height: usize,
}

/// One function being lowered.
struct Body<'l, 'p> {
    lowering: &'l mut Lowering<'p>,
    source: Source,
    /// For a function of `std.rt`, the span of the user code that reached
    /// it, where its refusals are reported.
    origin: Option<Span>,
    insts: Vec<Inst>,
    /// The cold blocks, placed after the hot path.
    cold: Vec<Inst>,
    temps: u32,
    labels: u32,
    /// The local slots in scope, by slot number.
    slots: Vec<Slot>,
    /// The receiver of a method.
    receiver: Option<Receiver>,
    /// The innermost loops.
    loops: Vec<Loop>,
    /// The temporaries holding a reference that is theirs to release,
    /// with their types.
    owned: BTreeMap<Temp, Type>,
    /// The values held while code that may leave them runs, innermost
    /// last.
    held: Vec<Held>,
    /// The function being lowered when it has `ensures` clauses: every
    /// return checks them ([`Body::return_value`]).
    ensuring: Option<&'p Function>,
    /// The temporary of each parameter after the receiver, in order,
    /// whether or not it takes a slot.
    param_temps: Vec<Temp>,
    /// The concrete type each generic parameter of the function stands
    /// for in the instance being lowered.
    subst: Vec<(String, Type)>,
    /// The function's result type, concrete: what `?` returns early.
    ret: Type,
    /// Whether it is a function value's code, which returns a status as
    /// its second result: 0 for a call that went ahead (`closures.rs`).
    status: bool,
    /// Whether its panics are its caller's to report: the code of a
    /// function value made from a prelude function, whose panic names
    /// the call of the value. Each panic returns its report's middle,
    /// `error[P....]: message`, as the status (`closures.rs`).
    deferred: bool,
    /// In [`Body::deferred`] code, the label an [`Inst::Enter`] that
    /// counts the caller's call again goes to, which it never does: the
    /// count it takes back was the caller's (`closures.rs`).
    recount: Option<Label>,
    /// Whether it is a drop function, which puts the drops of the objects
    /// it lets go of on std.rt's pending list instead of calling them, so
    /// that dropping a deep structure takes a bounded stack
    /// ([`Body::end_drop`]).
    dropping: bool,
}

impl<'l, 'p> Body<'l, 'p> {
    fn new(lowering: &'l mut Lowering<'p>, source: Source, origin: Option<Span>) -> Self {
        Body {
            lowering,
            source,
            origin,
            insts: Vec::new(),
            cold: Vec::new(),
            temps: 0,
            labels: 0,
            slots: Vec::new(),
            receiver: None,
            loops: Vec::new(),
            owned: BTreeMap::new(),
            held: Vec::new(),
            ensuring: None,
            param_temps: Vec::new(),
            subst: Vec::new(),
            ret: Type::Unit,
            status: false,
            deferred: false,
            recount: None,
            dropping: false,
        }
    }

    /// `ty` in the instance being lowered: with its generic parameters
    /// replaced by the concrete types they stand for.
    fn ty(&self, ty: &Type) -> Type {
        if self.subst.is_empty() {
            ty.clone()
        } else {
            ty.substitute(&self.subst)
        }
    }

    /// Record that `value`, of type `ty` (not yet substituted), holds a
    /// reference of its own, when `ty` is counted.
    fn own(&mut self, value: Temp, ty: &Type) {
        let ty = self.ty(ty);
        if self.counted(&ty) {
            self.owned.insert(value, ty);
        }
    }

    fn finish(mut self, name: &str, params: u32, span: Span) -> Result<ir::Function, Diagnostic> {
        if self.temps > ir::MAX_TEMPS {
            return Err(self.refuse(span, "a function this large"));
        }
        let cold = self.insts.len();
        self.insts.append(&mut self.cold);
        Ok(ir::Function {
            name: name.to_string(),
            params,
            temps: self.temps,
            labels: self.labels,
            insts: self.insts,
            cold,
        })
    }

    fn program(&self) -> &'p Program {
        self.lowering.program(self.source)
    }

    fn refuse(&self, span: Span, what: &str) -> Diagnostic {
        refusal(self.origin.unwrap_or(span), what, self.origin.is_some())
    }

    fn temp(&mut self) -> Temp {
        self.temps += 1;
        self.temps - 1
    }

    fn label(&mut self) -> Label {
        self.labels += 1;
        self.labels - 1
    }

    fn push(&mut self, inst: Inst) {
        self.insts.push(inst);
    }

    fn constant(&mut self, value: i64) -> Temp {
        let dst = self.temp();
        self.push(Inst::Const { dst, value });
        dst
    }

    /// A run limit as a constant. `Limits` holds at most `i64::MAX`.
    fn limit_constant(&mut self, limit: u64) -> Temp {
        self.constant(i64::try_from(limit).unwrap_or(i64::MAX))
    }

    fn copy(&mut self, src: Temp) -> Temp {
        let dst = self.temp();
        self.push(Inst::Copy { dst, src });
        dst
    }

    fn load(&mut self, base: Temp, offset: u32) -> Temp {
        let dst = self.temp();
        self.push(Inst::Load {
            width: Width::W64,
            dst,
            base,
            offset,
        });
        dst
    }

    fn binary_op(&mut self, op: BinOp, lhs: Temp, rhs: Temp) -> Temp {
        let dst = self.temp();
        self.push(Inst::Binary { op, dst, lhs, rhs });
        dst
    }

    fn rodata(&mut self, at: u32) -> Temp {
        let dst = self.temp();
        self.push(Inst::Addr {
            dst,
            symbol: Symbol::Rodata(at),
        });
        dst
    }

    /// A static text object holding `text`.
    fn text(&mut self, text: &str) -> Temp {
        let at = self.lowering.rodata.text(text);
        self.rodata(at)
    }
}

/// Whether `ty` is a capability whose value carries no state natively.
/// The `std.rt` function that compares two values of `ty` with `op`
/// (`==` or `!=`), if they are text.
fn text_equality(op: BinaryOp, ty: &Type) -> Option<&'static str> {
    if !matches!(op, BinaryOp::Eq | BinaryOp::Ne) {
        return None;
    }
    match ty {
        Type::String => Some("string_eq"),
        _ => None,
    }
}

/// Whether the prelude intrinsic `name` only reads its arguments while it
/// runs, so they may be borrowed ([`Body::borrowed_args`]): the `String`
/// and `Path` methods `std.rt` implements, which, like every call, leave
/// their arguments to the caller to release.
fn borrows_arguments(name: &str) -> bool {
    let name = intrinsics::unqualified(name);
    (name.starts_with("String.") || name.starts_with("Path."))
        && matches!(
            intrinsics::prelude(name),
            Some(Prelude::Runtime(_) | Prelude::Checked(..))
        )
}

/// Whether `main` can be given a value of `ty` by `_start`: the word 0.
/// Each of these keeps its state, if it has any (the arguments, the
/// environment, the input buffer, the generator, the root budget), in the
/// runtime's globals. `Net`, `NetConnect` and `NetListen` have no state:
/// they only attenuate (ADR 0025), and what they give, a host capability or
/// a loopback listener, is an object.
fn is_stateless_input(ty: &Type) -> bool {
    matches!(ty, Type::Named { name, args } if args.is_empty() && matches!(
        name.as_str(),
        "Stdout" | "Stderr" | "Stdin" | "Env" | "Clock" | "Random" | "Budget" | "Args"
            | "Fs" | "FsRead" | "FsWrite" | "Net" | "NetConnect" | "NetListen"
    ))
}

/// The word of `expr` when it is a constant that holds no address.
fn constant_word(program: &Program, expr: &Expr) -> Option<i64> {
    match &expr.kind {
        ExprKind::Unit => Some(0),
        ExprKind::Bool(value) => Some(i64::from(*value)),
        ExprKind::Int(value, _) => Some(*value as i64),
        ExprKind::Construct { ctor, fields } if fields.is_empty() => {
            let decl = program.types.get(&ctor.ty)?;
            if !layout::is_fieldless_enum(decl) {
                return None;
            }
            layout::variant_index(decl, ctor.variant.as_deref()?).map(|i| i as i64)
        }
        _ => None,
    }
}

/// The condition `op` tests, on signed or unsigned words.
fn comparison(op: BinaryOp, unsigned: bool) -> Cond {
    match (op, unsigned) {
        (BinaryOp::Eq, _) => Cond::Eq,
        (BinaryOp::Ne, _) => Cond::Ne,
        (BinaryOp::Lt, false) => Cond::Lt,
        (BinaryOp::Le, false) => Cond::Le,
        (BinaryOp::Gt, false) => Cond::Gt,
        (BinaryOp::Lt, true) => Cond::Below,
        (BinaryOp::Le, true) => Cond::BelowEq,
        (BinaryOp::Gt, true) => Cond::Above,
        (_, true) => Cond::AboveEq,
        (_, false) => Cond::Ge,
    }
}

/// What a refusal of an expression of this kind names.
fn describe(kind: &ExprKind) -> String {
    match kind {
        ExprKind::List(_) => "a list".to_string(),
        ExprKind::Field { .. } => "reading a field".to_string(),
        ExprKind::MutatingCall { .. } => "a mutating method call".to_string(),
        ExprKind::Try { .. } => "`?`".to_string(),
        ExprKind::Match { .. } => "`match`".to_string(),
        ExprKind::Unsupported(what) => what.clone(),
        _ => "this expression".to_string(),
    }
}

/// `kind`, a secret's operation as the public operation it is, with a
/// read of `Bytes` in it (`Bytes.at`, `Bytes.read_u32_be` and the other
/// endian reads) made the read of secret bytes
/// ([`intrinsics::SECRET_BYTES`]), whose bytes are marked secret.
fn secret_bytes_read(kind: ExprKind) -> ExprKind {
    let ExprKind::Construct { ctor, mut fields } = kind else {
        return kind;
    };
    if let [Expr {
        kind: ExprKind::Call {
            callee: Callee::Intrinsic(name),
            ..
        },
        ..
    }] = fields.as_mut_slice()
    {
        if let Some(method) = name.strip_prefix("Bytes.") {
            if matches!(intrinsics::prelude(name), Some(Prelude::ByteRead { .. })) {
                *name = IntrinsicName::new(format!("{}{method}", intrinsics::SECRET_BYTES));
            }
        }
    }
    ExprKind::Construct { ctor, fields }
}

/// A call of a secret's method, such as `Secret.rotl`, as the public
/// operations on the values inside its secrets, which `expand` gives from
/// the intrinsic's name (`oska-check`'s `program/secrets.rs`); `None` for
/// any other call, and for a secret intrinsic implemented here, such as
/// `Secret.expose`.
fn secret_expansion(
    callee: &Callee,
    expand: impl FnOnce(&str) -> Option<ExprKind>,
) -> Option<ExprKind> {
    match callee {
        Callee::Intrinsic(name) if name.starts_with("Secret.") => expand(name),
        _ => None,
    }
}
