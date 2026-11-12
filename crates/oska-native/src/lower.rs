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

mod closures;
mod data;
mod debug;
mod html;
mod lists;
mod patterns;
mod tasks;
mod tests;
mod traits;

use std::collections::{BTreeMap, HashMap};

use oska_check::builtins;
use oska_check::program::{
    trait_method, Callee, Construct, Expr, ExprKind, Function, IntType, Part, Program, Stmt, Type,
};
use oska_syntax::ast::BinaryOp;
use oska_syntax::{Diagnostic, Span};

use crate::intrinsics::{self, Check, Prelude, Runtime};
use crate::ir::{self, Arith, BinOp, Cond, FnId, Inst, Label, Symbol, Temp, Width};
use crate::layout::{self, Rodata};
use crate::optimize;
use crate::{codes, Locate, NOWHERE};

/// How deeply calls of the program's functions may nest before it
/// panics with `P0021`: the interpreter's limit (`oska_run`'s
/// `MAX_CALL_DEPTH`), so both panic at the same depth.
pub const MAX_CALL_DEPTH: u32 = 50_000;

/// The stack beyond [`MAX_CALL_DEPTH`] frames: room for `std.rt`'s calls
/// below the deepest frame, and for the entry point's.
const STACK_RESERVE: u64 = 8 << 20;

/// The words of the runtime's writable data, at `rt_mem_globals()`.
pub const GLOBALS: u32 = 40;

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
    ListEq(Source, Type),
    /// The text `dbg` shows a value of this struct or enum type as.
    Debug(Source, Type),
    /// Whether two values of this `Option` or `Result` type are equal,
    /// through `Eq` for each field's type (`traits.rs`).
    Equal(Source, Type),
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
    locate: Locate,
    kind: Entry,
) -> Result<ir::Module, Diagnostic> {
    let mut lowering = Lowering {
        user,
        rt,
        locate,
        kind,
        ids: HashMap::new(),
        functions: Vec::new(),
        queue: Vec::new(),
        rodata: Rodata::default(),
        user_functions: Vec::new(),
        between_functions: Vec::new(),
        closures: Vec::new(),
        closure_ids: HashMap::new(),
    };
    let entry = lowering.entry()?;
    while let Some((instance, id, origin)) = lowering.queue.pop() {
        lowering.functions[id] = lowering.function(&instance, origin)?;
        if matches!(
            instance,
            Instance::Function {
                source: Source::User,
                ..
            } | Instance::Closure(_)
                | Instance::Test(_)
        ) {
            lowering.user_functions.push(id);
        }
        if matches!(
            instance,
            Instance::Requires { .. }
                | Instance::TaskRunner { .. }
                | Instance::RunTasks
                | Instance::Thunk { .. }
                | Instance::Equal(..)
                | Instance::ListEq(..)
        ) {
            lowering.between_functions.push(id);
        }
    }
    optimize::module(&mut lowering.functions);
    lowering.size_stack(entry);
    // The most arguments any call passes, or any function takes, past
    // those in registers: they share one area after the globals.
    let extra_args = lowering
        .functions
        .iter()
        .flat_map(|function| {
            let calls = function.insts.iter().filter_map(|inst| match inst {
                Inst::Call { args, .. } | Inst::CallIndirect { args, .. } => Some(args.len()),
                _ => None,
            });
            calls.chain([function.params as usize])
        })
        .map(|count| count.saturating_sub(ir::REGISTER_ARGS))
        .max()
        .unwrap_or(0) as u32;
    Ok(ir::Module {
        functions: lowering.functions,
        entry,
        rodata: lowering.rodata.into_bytes(),
        data_size: (GLOBALS + extra_args) * layout::WORD,
        extra_args: GLOBALS * layout::WORD,
    })
}

struct Lowering<'p> {
    user: &'p Program,
    rt: &'p Program,
    locate: Locate<'p>,
    /// What the executable runs: a test executable's `std.rt` reaches its
    /// test host (`rt_mem_test_host`), a program's never does.
    kind: Entry,
    /// Every function requested so far.
    ids: HashMap<Instance, FnId>,
    /// The lowered functions, by id; a requested one is a placeholder
    /// until it is lowered.
    functions: Vec<ir::Function>,
    /// Functions requested and not lowered yet, with the span of the user
    /// code that first reached them.
    queue: Vec<(Instance, FnId, Span)>,
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

impl<'p> Lowering<'p> {
    fn program(&self, source: Source) -> &'p Program {
        match source {
            Source::User => self.user,
            Source::Runtime => self.rt,
        }
    }

    /// The id of `instance`, lowered later if it is new. `origin` is the
    /// user code that reached it.
    fn request(&mut self, instance: Instance, origin: Span) -> FnId {
        if let Some(&id) = self.ids.get(&instance) {
            return id;
        }
        let id = self.reserve(qualified(&instance));
        self.ids.insert(instance.clone(), id);
        self.queue.push((instance, id, origin));
        id
    }

    /// The index of `code` in [`Lowering::closures`], added if it is new.
    fn closure_index(&mut self, code: closures::ClosureCode) -> usize {
        let key = (
            std::sync::Arc::as_ptr(&code.code) as usize,
            code.source,
            code.subst.clone(),
        );
        if let Some(&index) = self.closure_ids.get(&key) {
            return index;
        }
        self.closures.push(code);
        self.closure_ids.insert(key, self.closures.len() - 1);
        self.closures.len() - 1
    }

    /// A placeholder for the function `name`, lowered later: its id.
    fn reserve(&mut self, name: String) -> FnId {
        self.functions.push(ir::Function {
            name,
            params: 0,
            temps: 0,
            labels: 0,
            insts: Vec::new(),
            cold: 0,
        });
        self.functions.len() - 1
    }

    /// The entry point, `_start`: map the stack and run `_main` on it,
    /// which calls `main` with its capabilities, reports an `Err` it
    /// returns, and exits. The stack's size is a placeholder until every
    /// function is lowered ([`Lowering::size_stack`]).
    fn entry(&mut self) -> Result<FnId, Diagnostic> {
        let user = self.user;
        let start = self.reserve("_start".to_string());
        let run = self.reserve("_main".to_string());

        let kind = self.kind;
        let mut body = Body::new(self, Source::User, None);
        match kind {
            Entry::Main => {
                if let Some(main) = user.functions.get("main") {
                    body.call_main(main)?;
                }
            }
            Entry::Tests => body.call_test()?,
        }
        let zero = body.constant(0);
        body.call_runtime("exit", vec![zero], NOWHERE)?;
        body.push(Inst::Return(zero));
        self.functions[run] = body.finish("_main", 0, NOWHERE)?;

        let mut body = Body::new(self, Source::User, None);
        let size = body.constant(0);
        let sp = body.temp();
        body.push(Inst::EntrySp { dst: sp });
        body.call_runtime("process_init", vec![sp], NOWHERE)?;
        let stack = body.call_runtime("stack_new", vec![size], NOWHERE)?;
        body.push(Inst::CallOnStack {
            stack,
            depth: MAX_CALL_DEPTH,
            callee: run,
        });
        body.push(Inst::Return(size));
        self.functions[start] = body.finish("_start", 0, NOWHERE)?;
        Ok(start)
    }

    /// Fill in the size of the stack `_start` maps: [`MAX_CALL_DEPTH`]
    /// levels of the program's largest frame and two of the largest frame
    /// that may run between two counted calls (each frame its temporaries, the
    /// callee-saved registers and the frame record), and
    /// [`STACK_RESERVE`]. A task recursing through `join` stacks a queue's
    /// and a runner's frame for every two counted calls, the function's
    /// and the closure's.
    fn size_stack(&mut self, start: FnId) {
        let largest = |ids: &[FnId]| {
            ids.iter()
                .filter_map(|&id| self.functions.get(id))
                .map(|f| 8 * u64::from(f.temps) + 8 * 9 + 32)
                .max()
                .unwrap_or(0)
        };
        // Up to two of the generated frames per level: a thunk, then the
        // `requires` check of the function it calls.
        let level = largest(&self.user_functions) + 2 * largest(&self.between_functions);
        let size = (u64::from(MAX_CALL_DEPTH) * level + STACK_RESERVE).next_multiple_of(1 << 16);
        if let Some(Inst::Const { value, .. }) = self.functions[start].insts.first_mut() {
            *value = size as i64;
        }
    }

    /// Lower the function `instance` names.
    fn function(&mut self, instance: &Instance, origin: Span) -> Result<ir::Function, Diagnostic> {
        let (source, name, type_args) = match instance {
            Instance::Function {
                source,
                name,
                type_args,
            }
            | Instance::Requires {
                source,
                name,
                type_args,
            } => (*source, name, type_args),
            Instance::Drop(source, ty) => {
                let mut body = Body::new(self, *source, Some(origin));
                body.drop_function(ty)?;
                return body.finish(&qualified(instance), 1, NOWHERE);
            }
            Instance::Debug(source, ty) => {
                let mut body = Body::new(self, *source, Some(origin));
                body.debug_function(ty)?;
                return body.finish(&qualified(instance), 1, NOWHERE);
            }
            Instance::Unshare(source, ty) => {
                let mut body = Body::new(self, *source, Some(origin));
                body.unshare_function(ty)?;
                return body.finish(&qualified(instance), 1, NOWHERE);
            }
            Instance::ListEq(source, ty) => {
                let mut body = Body::new(self, *source, Some(origin));
                body.list_eq_function(ty)?;
                return body.finish(&qualified(instance), 2, NOWHERE);
            }
            Instance::Equal(source, ty) => {
                let runtime_origin = (*source == Source::Runtime).then_some(origin);
                let mut body = Body::new(self, *source, runtime_origin);
                body.equal_function(ty, origin)?;
                return body.finish(&qualified(instance), 2, NOWHERE);
            }
            Instance::Closure(index) => {
                let source = self.closures[*index].source;
                let runtime_origin = (source == Source::Runtime).then_some(origin);
                let mut body = Body::new(self, source, runtime_origin);
                let (params, span) = body.closure_function(*index)?;
                return body.finish(&qualified(instance), params, span);
            }
            Instance::ClosureDrop(index) => {
                let source = self.closures[*index].source;
                let runtime_origin = (source == Source::Runtime).then_some(origin);
                let mut body = Body::new(self, source, runtime_origin);
                body.closure_drop_function(*index)?;
                return body.finish(&qualified(instance), 1, NOWHERE);
            }
            Instance::Test(index) => {
                let mut body = Body::new(self, Source::User, None);
                let span = body.test_function(*index)?;
                return body.finish(&qualified(instance), 1, span);
            }
            Instance::TaskRunner { result, start, end } => {
                let mut body = Body::new(self, Source::User, None);
                let at = Span {
                    start: *start,
                    end: *end,
                };
                body.task_runner(result, at)?;
                return body.finish(&qualified(instance), 1, NOWHERE);
            }
            Instance::RunTasks => {
                let mut body = Body::new(self, Source::User, None);
                body.run_tasks_function()?;
                return body.finish(&qualified(instance), 2, NOWHERE);
            }
            Instance::Thunk {
                source,
                intrinsic,
                name,
                type_args,
                ty,
            } => {
                let runtime_origin = (*source == Source::Runtime).then_some(origin);
                let mut body = Body::new(self, *source, runtime_origin);
                let params = body.thunk_function(*intrinsic, name, type_args, ty, origin)?;
                return body.finish(&qualified(instance), params, NOWHERE);
            }
        };
        let program = self.program(source);
        let runtime_origin = (source == Source::Runtime).then_some(origin);
        let Some(function) = program.functions.get(name) else {
            return Err(refusal(
                origin,
                &format!("a call of `{name}`"),
                runtime_origin.is_some(),
            ));
        };
        let at = |span: Span| runtime_origin.unwrap_or(span);
        if function.generics.len() != type_args.len() {
            return Err(refusal(
                at(function.body.span),
                &format!(
                    "the generic function `{}` without its type arguments",
                    function.name
                ),
                runtime_origin.is_some(),
            ));
        }
        if function.params.len() + usize::from(function.receiver) > ir::MAX_ARGS {
            return Err(refusal(
                at(function.body.span),
                &format!("a function with more than {} parameters", ir::MAX_ARGS),
                runtime_origin.is_some(),
            ));
        }
        let checks_requires = matches!(instance, Instance::Requires { .. });
        let mut body = Body::new(self, source, runtime_origin);
        body.ensuring =
            (!checks_requires && !function.contracts.ensures.is_empty()).then_some(function);
        body.subst = function
            .generics
            .iter()
            .cloned()
            .zip(type_args.clone())
            .collect();
        body.ret = body.ty(&function.ret);
        if function.receiver {
            let receiver = body.temp();
            let ty = function
                .self_ty
                .as_ref()
                .map_or(Type::Unit, |ty| body.ty(ty));
            body.receiver = Some(Receiver {
                temp: receiver,
                // A mutating method owns its receiver, and gives it back.
                owned: data::writes_self(function) && !checks_requires,
                ty,
            });
        }
        for param in &function.params {
            let temp = body.temp();
            body.param_temps.push(temp);
            if param.binds {
                // Borrowed from the caller: never released here.
                let ty = body.ty(&param.ty);
                body.slots.push(Slot {
                    temp,
                    release: false,
                    ty,
                });
            }
        }
        let params = body.temps;
        if checks_requires {
            body.requires_function(function)?;
        } else {
            let value = body.expr(&function.body)?;
            body.return_value(value, body_end(&function.body))?;
        }
        body.finish(&qualified(instance), params, function.body.span)
    }
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
        Instance::ListEq(_, ty) => format!("list_eq<{ty}>"),
        Instance::Debug(_, ty) => format!("debug<{ty}>"),
        Instance::Equal(_, ty) => format!("eq<{ty}>"),
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

    /// Whether values of `ty` are counted, in the instance being lowered.
    fn counted(&self, ty: &Type) -> bool {
        layout::is_counted(self.program(), &self.ty(ty))
    }

    // ---- Reference counts ------------------------------------------------

    /// Count one more reference to the object `value`, unless it is
    /// static.
    fn retain(&mut self, value: Temp) {
        let count = self.load(value, 0);
        let zero = self.constant(0);
        let done = self.label();
        self.push(Inst::Branch {
            cond: Cond::Lt,
            lhs: count,
            rhs: zero,
            target: done,
        });
        let one = self.constant(1);
        let more = self.binary_op(BinOp::Add, count, one);
        self.push(Inst::Store {
            width: Width::W64,
            base: value,
            offset: 0,
            src: more,
        });
        self.push(Inst::Label(done));
    }

    /// Drop one reference to the object `value`, of the concrete type
    /// `ty`, unless it is static. When that was the last, release what
    /// its fields hold and free it.
    fn release(&mut self, value: Temp, ty: &Type) -> Result<(), Diagnostic> {
        let count = self.load(value, 0);
        let zero = self.constant(0);
        let one = self.constant(1);
        let (done, shared) = (self.label(), self.label());
        self.push(Inst::Branch {
            cond: Cond::Lt,
            lhs: count,
            rhs: zero,
            target: done,
        });
        self.push(Inst::Branch {
            cond: Cond::Ne,
            lhs: count,
            rhs: one,
            target: shared,
        });
        if layout::is_confined(ty) {
            self.call_runtime("root_drop", vec![value], NOWHERE)?;
        } else if matches!(ty, Type::Fn { .. }) {
            if self.dropping {
                let code = self.load(value, layout::FN_DROP);
                self.call_runtime("drop_later", vec![value, code], NOWHERE)?;
            } else {
                self.drop_function_value(value);
            }
        } else if layout::holds_counted(self.program(), ty) {
            let drop = Instance::Drop(self.source, ty.clone());
            let origin = self.origin.unwrap_or(NOWHERE);
            let id = self.lowering.request(drop, origin);
            if self.dropping {
                let code = self.function_address(id);
                self.call_runtime("drop_later", vec![value, code], NOWHERE)?;
            } else {
                self.call_id(id, vec![value], NOWHERE)?;
            }
        } else {
            self.call_runtime("free", vec![value], NOWHERE)?;
        }
        self.push(Inst::Jump(done));
        self.push(Inst::Label(shared));
        let less = self.binary_op(BinOp::Sub, count, one);
        self.push(Inst::Store {
            width: Width::W64,
            base: value,
            offset: 0,
            src: less,
        });
        self.push(Inst::Label(done));
        Ok(())
    }

    /// Release `value` if it holds a reference of its own.
    fn consume(&mut self, value: Temp) -> Result<(), Diagnostic> {
        if let Some(ty) = self.owned.remove(&value) {
            self.release(value, &ty)?;
        }
        Ok(())
    }

    /// Release the slots above `height`, innermost first, without taking
    /// them out of scope: for code that leaves their scope.
    fn release_slots(&mut self, height: usize) -> Result<(), Diagnostic> {
        let slots: Vec<Slot> = self.slots.get(height..).unwrap_or_default().to_vec();
        for slot in slots.iter().rev() {
            if slot.release {
                self.release(slot.temp, &slot.ty)?;
            }
        }
        Ok(())
    }

    /// Release the values held inside the innermost `loops` open loops
    /// (all of them for `None`), innermost first, without letting go of
    /// them: for code that leaves them.
    fn release_held(&mut self, loops: Option<usize>) -> Result<(), Diagnostic> {
        let held: Vec<Held> = self.held.clone();
        for value in held.iter().rev() {
            if loops.map_or(true, |open| value.loops >= open) {
                if value.scope {
                    self.finish_scope(value.temp)?;
                }
                self.release(value.temp, &value.ty)?;
            }
        }
        Ok(())
    }

    /// End a scope: release its slots and take them out of scope.
    fn close_scope(&mut self, height: usize) -> Result<(), Diagnostic> {
        self.release_slots(height)?;
        self.slots.truncate(height);
        Ok(())
    }

    /// Return `value`, whose reference passes to the caller, releasing
    /// every local, and every value an expression still holds: a `return`
    /// or `?` inside an argument abandons the arguments before it.
    fn leave_function(&mut self, value: Temp) -> Result<(), Diagnostic> {
        self.owned.remove(&value);
        let pending: Vec<(Temp, Type)> = self
            .owned
            .iter()
            .rev()
            .map(|(&temp, ty)| (temp, ty.clone()))
            .collect();
        for (temp, ty) in pending {
            self.release(temp, &ty)?;
        }
        self.release_slots(0)?;
        self.release_held(None)?;
        if self.status {
            let went_ahead = self.constant(0);
            self.push(Inst::ReturnPair {
                value,
                second: went_ahead,
            });
            return Ok(());
        }
        match self.receiver.clone() {
            // A mutating method gives its receiver back.
            Some(Receiver {
                temp, owned: true, ..
            }) => self.push(Inst::ReturnPair {
                value,
                second: temp,
            }),
            _ => self.push(Inst::Return(value)),
        }
        Ok(())
    }

    /// Return `value`, at `at` (the `return`, or the end of the body),
    /// after its function's `ensures` clauses held: the first that is
    /// false panics with `P0007`, naming `at`. Every way out of a function
    /// goes through here.
    fn return_value(&mut self, value: Temp, at: Span) -> Result<(), Diagnostic> {
        if let Some(function) = self.ensuring {
            // The clauses run in a frame of their own: the parameters
            // that take a slot there, then `r`.
            let frame = self.contract_slots(function);
            let outer = std::mem::replace(&mut self.slots, frame);
            let loops = std::mem::take(&mut self.loops);
            let height = self.slots.len();
            let (line, column) = self.program().line_col(at.start);
            for ensures in &function.contracts.ensures {
                if ensures.binds {
                    let ty = self.ty(&function.ret);
                    // Borrowed: the return still owns the value.
                    self.slots.push(Slot {
                        temp: value,
                        release: false,
                        ty,
                    });
                }
                let holds = self.expr(&ensures.clause.cond)?;
                self.slots.truncate(height);
                let message = self.worded(
                    panics::POSTCONDITION_VIOLATED,
                    &format!(
                        "postcondition violated: `{}` ensures `{}`, \
                         broken by the return at line {line}, column {column}",
                        function.name, ensures.clause.text
                    ),
                    ensures.clause.span,
                );
                let broken = self.panic_static(
                    panics::POSTCONDITION_VIOLATED,
                    &message,
                    ensures.clause.span,
                )?;
                self.push(Inst::JumpIfZero {
                    cond: holds,
                    target: broken,
                });
            }
            self.slots = outer;
            self.loops = loops;
        }
        self.leave_function(value)
    }

    /// The slots of a function's contracts' frame, before `r`: its
    /// parameters that take one there.
    fn contract_slots(&self, function: &Function) -> Vec<Slot> {
        let mut slots = Vec::new();
        for (index, &takes) in function.contracts.params.iter().enumerate() {
            if let (true, Some(param), Some(&temp)) = (
                takes,
                function.params.get(index),
                self.param_temps.get(index),
            ) {
                slots.push(Slot {
                    temp,
                    release: false,
                    ty: self.ty(&param.ty),
                });
            }
        }
        slots
    }

    /// The body of [`Instance::Requires`]: evaluate each clause in order,
    /// returning the number (from 1) of the first that is false, or 0.
    fn requires_function(&mut self, function: &Function) -> Result<(), Diagnostic> {
        self.slots = self.contract_slots(function);
        for (index, clause) in function.contracts.requires.iter().enumerate() {
            let holds = self.expr(&clause.cond)?;
            let ok = self.label();
            let zero = self.constant(0);
            self.push(Inst::Branch {
                cond: Cond::Ne,
                lhs: holds,
                rhs: zero,
                target: ok,
            });
            let failed = self.constant(index as i64 + 1);
            self.push(Inst::Return(failed));
            self.push(Inst::Label(ok));
        }
        let none = self.constant(0);
        self.push(Inst::Return(none));
        Ok(())
    }

    /// Before a call of the user function `name`, with `args` (evaluated
    /// already) at `span`: evaluate its `requires` clauses, and panic with
    /// `P0006` at the call if one is false.
    fn check_requires(
        &mut self,
        name: &str,
        type_args: &[Type],
        args: &[Temp],
        span: Span,
    ) -> Result<(), Diagnostic> {
        let Some(function) = self.program().functions.get(name) else {
            return Ok(());
        };
        if function.contracts.requires.is_empty() {
            return Ok(());
        }
        let instance = Instance::Requires {
            source: self.source,
            name: name.to_string(),
            type_args: type_args.to_vec(),
        };
        let id = self.lowering.request(instance, self.origin.unwrap_or(span));
        let failed = self.call_id(id, args.to_vec(), span)?;
        let (before, after) = self.report(panics::PRECONDITION_VIOLATED, span);
        let clauses = &function.contracts.requires;
        let broken = self.cold_block(|body| {
            // Read the result first: it is a hot temporary.
            let failed = body.copy(failed);
            for (index, clause) in clauses.iter().enumerate() {
                let next = body.label();
                if index + 1 < clauses.len() {
                    let this = body.constant(index as i64 + 1);
                    body.push(Inst::Branch {
                        cond: Cond::Ne,
                        lhs: failed,
                        rhs: this,
                        target: next,
                    });
                }
                let message = body.worded(
                    panics::PRECONDITION_VIOLATED,
                    &format!(
                        "precondition violated: `{}` requires `{}`",
                        function.name, clause.text
                    ),
                    span,
                );
                let report = body.text(&format!("{before}{message}{after}"));
                let unit = body.call_runtime("panic", vec![report], NOWHERE)?;
                body.push(Inst::Return(unit));
                body.push(Inst::Label(next));
            }
            Ok(())
        })?;
        let zero = self.constant(0);
        self.push(Inst::Branch {
            cond: Cond::Ne,
            lhs: failed,
            rhs: zero,
            target: broken,
        });
        Ok(())
    }

    // ---- Panics ------------------------------------------------------------

    /// A cold block that `build` fills: its label.
    fn cold_block(
        &mut self,
        build: impl FnOnce(&mut Self) -> Result<(), Diagnostic>,
    ) -> Result<Label, Diagnostic> {
        let label = self.label();
        let hot = std::mem::replace(&mut self.insts, vec![Inst::Label(label)]);
        let built = build(self);
        let block = std::mem::replace(&mut self.insts, hot);
        built?;
        self.cold.extend(block);
        Ok(label)
    }

    /// The text before and after the message of a panic with `code` at
    /// `span`, as the interpreter's report prints it:
    /// `path:line:col: error[P....]: ` and ` (bytes a..b)` and a newline.
    /// A panic in `std.rt` has no file to name.
    fn report(&self, code: &'static str, span: Span) -> (String, String) {
        let (location, after) = self.report_around(span);
        (format!("{location}error[{code}]: "), after)
    }

    /// The text before a panic's code (`path:line:col: `, or nothing in
    /// `std.rt`) and after its message, for a panic at `span`.
    fn report_around(&self, span: Span) -> (String, String) {
        let unlocated = Diagnostic::new("", "", span);
        let located = match self.source {
            Source::User => (self.lowering.locate)(unlocated),
            Source::Runtime => unlocated,
        };
        let before = match &located.location {
            Some(location) => format!("{location}: "),
            None => String::new(),
        };
        let after = format!(" (bytes {}..{})\n", located.span.start, located.span.end);
        (before, after)
    }

    /// `message`, a panic's at `span`, as the caller's locator words it:
    /// in a package, each item key in it (`pkg.util.half`) names the item
    /// as code in the module at `span` writes it (`util.half`, `half`), as
    /// the interpreter's report does once `oskac` locates it.
    fn worded(&self, code: &'static str, message: &str, span: Span) -> String {
        match self.source {
            Source::User => (self.lowering.locate)(Diagnostic::new(code, message, span)).message,
            Source::Runtime => message.to_string(),
        }
    }

    /// In [`Body::deferred`] code, a cold block that gives the caller the
    /// text `status` builds (in the cold block) as the status.
    fn defer_panic(
        &mut self,
        status: impl FnOnce(&mut Self) -> Result<Temp, Diagnostic>,
    ) -> Result<Label, Diagnostic> {
        self.cold_block(|body| {
            let text = status(body)?;
            let nothing = body.constant(0);
            body.push(Inst::ReturnPair {
                value: nothing,
                second: text,
            });
            Ok(())
        })
    }

    /// A cold block that panics with the status `status` a function value's
    /// code returned, its report's middle, at the call at `span`. In
    /// [`Body::deferred`] code, it passes the status on to its own caller.
    fn panic_status(&mut self, status: Temp, span: Span) -> Result<Label, Diagnostic> {
        if self.deferred {
            return self.defer_panic(|_| Ok(status));
        }
        let reworded = self.reworded_preconditions(span);
        let (before, after) = self.report_around(span);
        self.cold_block(|body| {
            // Read the status first: it is a hot temporary.
            let status = body.copy(status);
            let start = body.text(&before);
            let end = body.text(&after);
            for (raw, shown) in &reworded {
                let next = body.label();
                let raw = body.text(raw);
                body.push(Inst::Branch {
                    cond: Cond::Ne,
                    lhs: status,
                    rhs: raw,
                    target: next,
                });
                let shown = body.text(shown);
                let unit = body.call_runtime("panic_message", vec![start, end, shown], NOWHERE)?;
                body.push(Inst::Return(unit));
                body.push(Inst::Label(next));
            }
            let unit = body.call_runtime("panic_message", vec![start, end, status], NOWHERE)?;
            body.push(Inst::Return(unit));
            Ok(())
        })
    }

    /// Every `requires` status a function value's code may return
    /// (`error[P0006]: precondition violated: ...`, naming the function by
    /// its key) that the report at `span` words differently, with that
    /// wording: in a package, the module at `span` names an item as its
    /// code writes it, as the interpreter's located report does. Texts are
    /// shared, so a status is the same object as its text here, and
    /// [`Body::panic_status`] picks the wording by comparing addresses.
    fn reworded_preconditions(&self, span: Span) -> Vec<(String, String)> {
        let mut reworded = Vec::new();
        if self.source != Source::User {
            return reworded;
        }
        let code = panics::PRECONDITION_VIOLATED;
        for function in self.program().functions.values() {
            for clause in &function.contracts.requires {
                let message = format!(
                    "precondition violated: `{}` requires `{}`",
                    function.name, clause.text
                );
                let shown = self.worded(code, &message, span);
                if shown != message {
                    reworded.push((
                        format!("error[{code}]: {message}"),
                        format!("error[{code}]: {shown}"),
                    ));
                }
            }
        }
        reworded
    }

    /// A cold block that panics with `code` and the fixed `message`.
    fn panic_static(
        &mut self,
        code: &'static str,
        message: &str,
        span: Span,
    ) -> Result<Label, Diagnostic> {
        if self.deferred {
            let status = format!("error[{code}]: {message}");
            return self.defer_panic(|body| Ok(body.text(&status)));
        }
        let (before, after) = self.report(code, span);
        self.cold_block(|body| {
            let report = body.text(&format!("{before}{message}{after}"));
            let unit = body.call_runtime("panic", vec![report], NOWHERE)?;
            body.push(Inst::Return(unit));
            Ok(())
        })
    }

    /// A cold block that panics with `code`, by the `std.rt` function
    /// `function`, which writes the message between the report's start and
    /// end texts, which it is given first, then `args`.
    fn panic_text(
        &mut self,
        code: &'static str,
        function: &str,
        args: &[Temp],
        span: Span,
    ) -> Result<Label, Diagnostic> {
        if self.deferred {
            let Some(message) = function.strip_prefix("panic_") else {
                return Err(self.refuse(span, &format!("`{function}` in a function value")));
            };
            let builder = format!("{message}_message");
            return self.defer_panic(|body| {
                let head = body.text(&format!("error[{code}]: "));
                let mut all = vec![head];
                all.extend_from_slice(args);
                body.call_runtime(&builder, all, NOWHERE)
            });
        }
        let (before, after) = self.report(code, span);
        self.cold_block(|body| {
            let start = body.text(&before);
            let end = body.text(&after);
            let mut all = vec![start, end];
            all.extend_from_slice(args);
            let unit = body.call_runtime(function, all, NOWHERE)?;
            body.push(Inst::Return(unit));
            Ok(())
        })
    }

    /// A cold block that panics with `code` and the message `head`, the
    /// integer `a`, then `tail`.
    fn panic_unary(
        &mut self,
        code: &'static str,
        (head, a, tail): (&str, Temp, &str),
        unsigned: bool,
        span: Span,
    ) -> Result<Label, Diagnostic> {
        if self.deferred {
            return self.defer_panic(|body| {
                let start = body.text(&format!("error[{code}]: {head}"));
                let end = body.text(tail);
                let unsigned = body.constant(i64::from(unsigned));
                body.call_runtime("unary_message", vec![start, a, end, unsigned], NOWHERE)
            });
        }
        let (before, after) = self.report(code, span);
        self.cold_block(|body| {
            let start = body.text(&format!("{before}{head}"));
            let end = body.text(&format!("{tail}{after}"));
            let unsigned = body.constant(i64::from(unsigned));
            let unit = body.call_runtime("panic_unary", vec![start, a, end, unsigned], NOWHERE)?;
            body.push(Inst::Return(unit));
            Ok(())
        })
    }

    /// A cold block that panics with `code` and the message `head`, the
    /// integer `a`, `mid`, the integer `b`, then `tail`.
    fn panic_binary(
        &mut self,
        code: &'static str,
        (head, a, mid, b, tail): (&str, Temp, &str, Temp, &str),
        unsigned: bool,
        span: Span,
    ) -> Result<Label, Diagnostic> {
        if self.deferred {
            return self.defer_panic(|body| {
                let start = body.text(&format!("error[{code}]: {head}"));
                let mid = body.text(mid);
                let end = body.text(tail);
                let unsigned = body.constant(i64::from(unsigned));
                body.call_runtime(
                    "binary_message",
                    vec![start, a, mid, b, end, unsigned],
                    NOWHERE,
                )
            });
        }
        let (before, after) = self.report(code, span);
        self.cold_block(|body| {
            let start = body.text(&format!("{before}{head}"));
            let mid = body.text(mid);
            let end = body.text(&format!("{tail}{after}"));
            let unsigned = body.constant(i64::from(unsigned));
            let unit = body.call_runtime(
                "panic_binary",
                vec![start, a, mid, b, end, unsigned],
                NOWHERE,
            )?;
            body.push(Inst::Return(unit));
            Ok(())
        })
    }

    // ---- Calls -------------------------------------------------------------

    /// Call the function `id` with `args`.
    fn call_id(&mut self, callee: FnId, args: Vec<Temp>, span: Span) -> Result<Temp, Diagnostic> {
        if args.len() > ir::MAX_ARGS {
            let what = format!("a call with more than {} arguments", ir::MAX_ARGS);
            return Err(self.refuse(span, &what));
        }
        let dst = self.temp();
        self.push(Inst::Call { dst, callee, args });
        Ok(dst)
    }

    /// Call the function `name` of `source`, instantiated with the
    /// concrete `type_args`.
    fn call_function(
        &mut self,
        source: Source,
        name: &str,
        type_args: Vec<Type>,
        args: Vec<Temp>,
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        let origin = self.origin.unwrap_or(span);
        let instance = Instance::Function {
            source,
            name: name.to_string(),
            type_args,
        };
        let id = self.lowering.request(instance, origin);
        self.call_id(id, args, span)
    }

    /// Call the `std.rt` function `name`.
    fn call_runtime(
        &mut self,
        name: &str,
        args: Vec<Temp>,
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        self.call_function(Source::Runtime, name, Vec::new(), args, span)
    }

    /// Call the function `name` of the program being lowered with the
    /// concrete `type_args`. A call of a user function from user code
    /// counts its depth: at [`MAX_CALL_DEPTH`] it panics with `P0021` at
    /// `span` instead.
    fn call_program(
        &mut self,
        name: &str,
        type_args: Vec<Type>,
        args: Vec<Temp>,
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        if self.source == Source::Runtime {
            return self.call_function(Source::Runtime, name, type_args, args, span);
        }
        let exhausted = self.panic_static(panics::CALL_DEPTH_EXCEEDED, panics::TOO_DEEP, span)?;
        self.push(Inst::Enter { exhausted });
        self.check_requires(name, &type_args, &args, span)?;
        let result = self.call_function(Source::User, name, type_args, args, span)?;
        self.push(Inst::Leave);
        Ok(result)
    }

    /// The second result of the call just made: a mutating method's
    /// receiver as it left it.
    fn second_result(&mut self) -> Temp {
        let dst = self.temp();
        self.push(Inst::Second { dst });
        dst
    }

    /// A call of `callee`, instantiated with `type_args` (not yet
    /// substituted), with `args`, evaluated already, at `span`, whose
    /// result is of type `ret`. The call releases the arguments that hold
    /// a reference of their own, and its result holds one when `ret` is
    /// counted.
    fn call(
        &mut self,
        callee: &Callee,
        type_args: &[Type],
        args: Vec<Temp>,
        ret: &Type,
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        let result = match callee {
            Callee::Function(name) if self.program().functions.contains_key(name) => {
                let type_args: Vec<Type> = type_args.iter().map(|t| self.ty(t)).collect();
                let mutated = self.mutated_receiver(name, &type_args);
                let result = self.call_program(name, type_args, args.clone(), span)?;
                if let (Some(ty), Some(&receiver)) = (mutated, args.first()) {
                    // A mutating method called on a value, not a place:
                    // it took the receiver's reference and gives back the
                    // receiver it changed, which is released here.
                    let after = self.second_result();
                    self.owned.remove(&receiver);
                    if self.counted(&ty) {
                        self.owned.insert(after, ty);
                    }
                    self.consume(after)?;
                }
                result
            }
            Callee::Function(name) => match intrinsics::runtime(name) {
                // Only `std.rt` may name an intrinsic (the checker's
                // `E0209`), and the backend holds it to that too.
                Some(runtime) if self.source == Source::Runtime => {
                    self.runtime_intrinsic(runtime, &args)
                }
                _ => return Err(self.refuse(span, &format!("a call of `{name}`"))),
            },
            // `dbg(value)` writes the value's text to stderr and returns
            // the value: its reference passes on.
            Callee::Intrinsic(name) if name == "dbg" => {
                let (Some(&value), Some(ty)) = (args.first(), type_args.first()) else {
                    return Err(self.refuse(span, "`dbg`"));
                };
                let ty = self.ty(ty);
                let text = self.debug_text(&ty, value, span)?;
                self.call_runtime("dbg_write", vec![text], span)?;
                self.release(text, &Type::String)?;
                return Ok(value);
            }
            Callee::Intrinsic(name) => match intrinsics::prelude(name) {
                Some(Prelude::Runtime(function)) => {
                    self.call_runtime(function, args.clone(), span)?
                }
                // The argument is the result: its reference passes on.
                Some(Prelude::Identity) => match args.first() {
                    Some(&value) => return Ok(value),
                    None => return Err(self.refuse(span, &format!("`{name}`"))),
                },
                Some(Prelude::Field(index)) => match args.first() {
                    Some(&value) => {
                        let field = self.load(value, layout::struct_field(index));
                        if self.counted(ret) {
                            self.retain(field);
                        }
                        field
                    }
                    None => return Err(self.refuse(span, &format!("`{name}`"))),
                },
                Some(Prelude::Checked(function, check)) => {
                    let result = self.call_runtime(function, args.clone(), span)?;
                    self.check(check, result, &args, span)?;
                    result
                }
                None => return Err(self.refuse(span, &format!("`{name}`"))),
            },
            Callee::Trait { trait_name, method } => {
                return Err(self.refuse(
                    span,
                    &format!("a call of `{trait_name}.{method}` on a value of a type parameter"),
                ))
            }
        };
        for arg in args {
            self.consume(arg)?;
        }
        self.own(result, ret);
        Ok(result)
    }

    /// For a mutating method of the program, `name` (one that may change
    /// its receiver, `data::writes_self`), instantiated with the concrete
    /// `type_args`: its receiver's type. `None` for any other function.
    fn mutated_receiver(&self, name: &str, type_args: &[Type]) -> Option<Type> {
        let function = self.program().functions.get(name)?;
        if !data::writes_self(function) {
            return None;
        }
        let subst: Vec<(String, Type)> = function
            .generics
            .iter()
            .cloned()
            .zip(type_args.iter().cloned())
            .collect();
        Some(function.self_ty.as_ref()?.substitute(&subst))
    }

    /// Panic after a [`Prelude::Checked`] call of `args` that gave `result`:
    /// the word 0 (and, for `Repeat`, -1) says the call was a bug.
    fn check(
        &mut self,
        check: Check,
        result: Temp,
        args: &[Temp],
        span: Span,
    ) -> Result<(), Diagnostic> {
        match check {
            Check::Slice => {
                let label =
                    self.panic_text(panics::SLICE_OUT_OF_BOUNDS, "panic_slice", args, span)?;
                self.push(Inst::JumpIfZero {
                    cond: result,
                    target: label,
                });
            }
            Check::Secret => {
                let label =
                    self.panic_text(panics::MISSING_SECRET, "panic_missing_secret", args, span)?;
                self.push(Inst::JumpIfZero {
                    cond: result,
                    target: label,
                });
            }
            Check::Repeat => {
                let negative = self.panic_text(
                    panics::NEGATIVE_REPEAT,
                    "panic_repeat_negative",
                    &args[1..],
                    span,
                )?;
                self.push(Inst::JumpIfZero {
                    cond: result,
                    target: negative,
                });
                let long =
                    self.panic_text(panics::TEXT_TOO_LONG, "panic_repeat_long", args, span)?;
                let zero = self.constant(0);
                self.push(Inst::Branch {
                    cond: Cond::Lt,
                    lhs: result,
                    rhs: zero,
                    target: long,
                });
            }
        }
        Ok(())
    }

    /// A `std.rt` intrinsic applied to `args`. The checker has matched
    /// their number to its declaration.
    fn runtime_intrinsic(&mut self, runtime: Runtime, args: &[Temp]) -> Temp {
        let arg = |i: usize| args.get(i).copied().unwrap_or_default();
        match runtime {
            Runtime::Syscall(number) => {
                let dst = self.temp();
                self.push(Inst::Syscall {
                    dst,
                    number,
                    args: args.to_vec(),
                });
                dst
            }
            Runtime::Load(width) => {
                let dst = self.temp();
                self.push(Inst::Load {
                    width,
                    dst,
                    base: arg(0),
                    offset: 0,
                });
                dst
            }
            Runtime::Store(width) => {
                self.push(Inst::Store {
                    width,
                    base: arg(0),
                    offset: 0,
                    src: arg(1),
                });
                self.constant(0)
            }
            Runtime::Binary(op) => self.binary_op(op, arg(0), arg(1)),
            // A copy: the argument's reference, if it holds one, is
            // released after the call, and the word stays.
            Runtime::Identity => self.copy(arg(0)),
            Runtime::Globals => {
                let dst = self.temp();
                self.push(Inst::Addr {
                    dst,
                    symbol: Symbol::Data(0),
                });
                dst
            }
            Runtime::TestHost => self.test_host(),
        }
    }

    /// Call `main` with its capabilities and, when it returns `Err(e)`,
    /// hand `e`'s text to `std.rt`'s `main_failed`, which exits. As in
    /// the interpreter, the call of `main` is not counted.
    fn call_main(&mut self, main: &Function) -> Result<(), Diagnostic> {
        let span = main.body.span;
        let mut args = Vec::new();
        for param in &main.params {
            if !is_stateless_input(&param.ty) {
                return Err(self.refuse(span, &format!("a `main` given `{}`", param.ty)));
            }
            args.push(self.constant(0));
        }
        let result = self.call_function(Source::User, "main", Vec::new(), args, span)?;
        match &main.ret {
            Type::Unit => Ok(()),
            Type::Named { name, args } if name == builtins::RESULT && args.len() == 2 => {
                let err = self.variant(builtins::RESULT, builtins::ERR, span)?;
                let tag = self.load(result, layout::ENUM_TAG);
                let err = self.constant(err as i64);
                let ok = self.label();
                self.push(Inst::Branch {
                    cond: Cond::Ne,
                    lhs: tag,
                    rhs: err,
                    target: ok,
                });
                let error = self.load(result, layout::enum_field(0));
                let text = self.display(&args[1], error, span)?;
                // `main_failed` exits, so the text is never released.
                self.call_runtime("main_failed", vec![text], span)?;
                self.push(Inst::Label(ok));
                Ok(())
            }
            other => Err(self.refuse(span, &format!("a `main` that returns `{other}`"))),
        }
    }

    /// The `Display` text of `value`, of the concrete type `ty`: through
    /// the program's impl, generic or not, or the prelude's.
    fn display(&mut self, ty: &Type, value: Temp, span: Span) -> Result<Temp, Diagnostic> {
        let tag = match ty {
            Type::String => return Ok(value),
            Type::Int(int) => int.name(),
            Type::Bool => "Bool",
            Type::Named { name, .. } => name,
            _ => return Err(self.refuse(span, &format!("the `Display` text of a `{ty}`"))),
        };
        let user = trait_method(tag, "Display", "to_string");
        let (callee, type_args) = match self.program().functions.get(&user) {
            Some(function) => {
                let mut bound = Vec::new();
                if let Some(self_ty) = &function.self_ty {
                    self_ty.bind(ty, &mut bound);
                }
                let type_args = function
                    .generics
                    .iter()
                    .map(|g| {
                        bound
                            .iter()
                            .find(|(name, _)| name == g)
                            .map_or(Type::Unit, |(_, t)| t.clone())
                    })
                    .collect();
                (Callee::Function(user), type_args)
            }
            None => (Callee::Intrinsic(format!("{tag}.to_string")), Vec::new()),
        };
        self.call(&callee, &type_args, vec![value], &Type::String, span)
    }

    /// The index of `variant` in the enum `ty`.
    fn variant(&self, ty: &str, variant: &str, span: Span) -> Result<usize, Diagnostic> {
        self.program()
            .types
            .get(ty)
            .and_then(|decl| layout::variant_index(decl, variant))
            .ok_or_else(|| self.refuse(span, &format!("the variant `{ty}.{variant}`")))
    }

    // ---- Expressions -------------------------------------------------------

    fn expr(&mut self, expr: &Expr) -> Result<Temp, Diagnostic> {
        let span = expr.span;
        match &expr.kind {
            ExprKind::Unit => Ok(self.constant(0)),
            ExprKind::Bool(value) => Ok(self.constant(i64::from(*value))),
            // The bit pattern: a `U64` above `i64::MAX` wraps to negative.
            ExprKind::Int(value, _) => Ok(self.constant(*value as i64)),
            ExprKind::Str(text) => Ok(self.text(text)),
            ExprKind::Local(slot) => match self.slots.get(*slot) {
                Some(&Slot { temp, .. }) => Ok(self.read(temp, &expr.ty)),
                None => Err(self.refuse(span, "a local slot out of scope")),
            },
            ExprKind::SelfValue => match self.receiver.as_ref().map(|r| r.temp) {
                Some(receiver) => Ok(self.read(receiver, &expr.ty)),
                None => Err(self.refuse(span, "`self` outside a method")),
            },
            ExprKind::Block { stmts, value } => {
                let height = self.slots.len();
                for stmt in stmts {
                    self.stmt(stmt)?;
                }
                let value = match value {
                    Some(value) => self.expr(value)?,
                    None => self.constant(0),
                };
                self.close_scope(height)?;
                Ok(value)
            }
            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let result = self.temp();
                let (otherwise, end) = (self.label(), self.label());
                self.branch_unless(cond, otherwise)?;
                let then_value = self.expr(then_branch)?;
                self.push(Inst::Copy {
                    dst: result,
                    src: then_value,
                });
                // Not held on the other branch, which may leave the
                // function (`?`) and releases what is held.
                let then_owned = self.owned.remove(&then_value);
                self.push(Inst::Jump(end));
                self.push(Inst::Label(otherwise));
                let else_value = match else_branch {
                    Some(else_branch) => self.expr(else_branch)?,
                    None => self.constant(0),
                };
                self.push(Inst::Copy {
                    dst: result,
                    src: else_value,
                });
                self.push(Inst::Label(end));
                let else_owned = self.owned.remove(&else_value);
                self.merge_owned(vec![then_owned, else_owned], result);
                Ok(result)
            }
            ExprKind::Not(operand) => {
                let value = self.expr(operand)?;
                let one = self.constant(1);
                Ok(self.binary_op(BinOp::Xor, value, one))
            }
            ExprKind::Neg { operand, int } => self.negate(operand, *int, span),
            ExprKind::Binary { op, lhs, rhs, int } => self.binary(*op, lhs, rhs, *int, span),
            ExprKind::Interpolated(parts) => self.interpolated(parts),
            ExprKind::Construct { ctor, fields } => self.construct(expr, ctor, fields),
            ExprKind::Range { start, end } => {
                // A range is the prelude's struct `Range(start:, end:)`.
                let ctor = Construct {
                    ty: builtins::RANGE.to_string(),
                    variant: None,
                };
                self.construct(expr, &ctor, &[(**start).clone(), (**end).clone()])
            }
            ExprKind::Call {
                callee,
                type_args,
                args,
            } => {
                if let Some(word) = self.borrowed_word(callee, args) {
                    return Ok(word);
                }
                let mut values = Vec::with_capacity(args.len());
                for arg in args {
                    values.push(self.expr(arg)?);
                }
                let receiver = args.first().map(|arg| &arg.ty);
                let (callee, type_args) = self.resolve(callee, type_args, receiver, span)?;
                if let Callee::Intrinsic(name) = &callee {
                    if let Some(value) = self.data_intrinsic(name, args, &values, expr)? {
                        return Ok(value);
                    }
                    if let Some(value) = self.eq_intrinsic(name, args, &values, span)? {
                        return Ok(value);
                    }
                    if let Some(value) = self.list_intrinsic(name, args, &values, expr)? {
                        return Ok(value);
                    }
                    if let Some(value) = self.test_intrinsic(name, &type_args, &values, span)? {
                        return Ok(value);
                    }
                    if let Some(value) = self.task_intrinsic(name, &values, expr)? {
                        return Ok(value);
                    }
                    if let Some(value) = self.html_intrinsic(name, &values, expr)? {
                        return Ok(value);
                    }
                    if name == "todo" {
                        let todo = self.panic_static(panics::TODO, panics::NOT_WRITTEN, span)?;
                        self.push(Inst::Jump(todo));
                        // Never reached: a value of no type, holding nothing.
                        return Ok(self.constant(0));
                    }
                }
                self.call(&callee, &type_args, values, &expr.ty, span)
            }
            ExprKind::Field { base, index } => self.field(base, *index, &expr.ty),
            ExprKind::List(items) => self.list_literal(expr, items),
            ExprKind::Match { scrutinee, arms } => self.match_arms(scrutinee, arms),
            ExprKind::MutatingCall {
                callee,
                type_args,
                receiver,
                args,
            } => self.mutating_call(callee, type_args, receiver, args, &expr.ty, span),
            ExprKind::Try {
                operand,
                convert,
                type_args,
            } => self.try_expr(operand, convert.as_ref(), type_args, &expr.ty, span),
            ExprKind::Scope(body) => self.scope_block(body),
            ExprKind::Closure(code) => self.closure(expr, code),
            ExprKind::FnValue { callee, type_args } => self.fn_value(expr, callee, type_args),
            ExprKind::CallValue { callee, args } => self.call_value(callee, args, expr),
            other => Err(self.refuse(span, &describe(other))),
        }
    }

    /// In std.rt, a value's object address (`rt_mem_text(s)` and the
    /// other identity intrinsics that give an `Int`) taken from a slot
    /// that never changes and holds no reference of its own (a
    /// parameter): the word itself, with no retain before and release
    /// after the call, which would cancel out. `None` for any other call.
    fn borrowed_word(&mut self, callee: &Callee, args: &[Expr]) -> Option<Temp> {
        let Callee::Function(name) = callee else {
            return None;
        };
        if self.source != Source::Runtime
            || intrinsics::runtime(name) != Some(Runtime::Identity)
            || !name.starts_with("rt_mem_")
            || name.starts_with("rt_mem_as_")
        {
            return None;
        }
        let [arg] = args else {
            return None;
        };
        let ExprKind::Local(slot) = &arg.kind else {
            return None;
        };
        let slot = self.slots.get(*slot)?;
        if slot.release {
            return None;
        }
        let temp = slot.temp;
        Some(self.copy(temp))
    }

    /// The value of the local or receiver in `temp`, of type `ty`: a copy,
    /// so that assigning the local later does not change it, holding a
    /// reference of its own when `ty` is counted.
    fn read(&mut self, temp: Temp, ty: &Type) -> Temp {
        let value = self.copy(temp);
        if self.counted(ty) {
            self.retain(value);
            self.own(value, ty);
        }
        value
    }

    /// `result` holds whichever value a branch chose, each taken out of
    /// [`Body::owned`] (`owned`, the type of each that held a reference)
    /// as its branch ended, so that code in a later branch never releases
    /// it: the result holds a reference of its own if any of them did. A
    /// branch may give a static object, which needs no release; releasing
    /// one is harmless.
    fn merge_owned(&mut self, owned: Vec<Option<Type>>, result: Temp) {
        if let Some(ty) = owned.into_iter().flatten().last() {
            self.owned.insert(result, ty);
        }
    }

    fn binary(
        &mut self,
        op: BinaryOp,
        lhs: &Expr,
        rhs: &Expr,
        int: Option<IntType>,
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        match (op, int) {
            (BinaryOp::And | BinaryOp::Or | BinaryOp::Implies, _) => {
                self.short_circuit(op, lhs, rhs)
            }
            (
                BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem,
                Some(int),
            ) => self.arithmetic(op, lhs, rhs, int, span),
            (
                BinaryOp::Eq
                | BinaryOp::Ne
                | BinaryOp::Lt
                | BinaryOp::Le
                | BinaryOp::Gt
                | BinaryOp::Ge,
                None,
            ) => {
                if let Some(function) = text_equality(op, &lhs.ty) {
                    let (left, right) = (self.expr(lhs)?, self.expr(rhs)?);
                    let equal = self.call_runtime(function, vec![left, right], span)?;
                    self.consume(left)?;
                    self.consume(right)?;
                    if op == BinaryOp::Ne {
                        let one = self.constant(1);
                        return Ok(self.binary_op(BinOp::Xor, equal, one));
                    }
                    return Ok(equal);
                }
                let unsigned = match &lhs.ty {
                    Type::Int(int) => int.min() == 0,
                    Type::Bool | Type::Unit => false,
                    other => {
                        return Err(self.refuse(span, &format!("comparing values of `{other}`")))
                    }
                };
                let cond = comparison(op, unsigned);
                let lhs = self.expr(lhs)?;
                let rhs = self.expr(rhs)?;
                let dst = self.temp();
                self.push(Inst::Compare {
                    cond,
                    dst,
                    lhs,
                    rhs,
                });
                Ok(dst)
            }
            _ => Err(self.refuse(span, &format!("the operator `{}` here", op.text()))),
        }
    }

    /// `lhs op rhs` on integers of type `int`, checked as the interpreter
    /// checks it: division by zero panics with `P0004`, a result outside
    /// the type with `P0001`, with the interpreter's messages.
    fn arithmetic(
        &mut self,
        op: BinaryOp,
        lhs: &Expr,
        rhs: &Expr,
        int: IntType,
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        let a = self.expr(lhs)?;
        let b = self.expr(rhs)?;
        let kind = layout::int_kind(int);
        let unsigned = !kind.signed;
        let arith = match op {
            BinaryOp::Add => Arith::Add,
            BinaryOp::Sub => Arith::Sub,
            BinaryOp::Mul => Arith::Mul,
            BinaryOp::Div => Arith::Div,
            _ => Arith::Rem,
        };
        // A divisor written as a literal other than 0 needs no check.
        let nonzero = matches!(rhs.kind, ExprKind::Int(value, _) if value != 0);
        if matches!(arith, Arith::Div | Arith::Rem) && !nonzero {
            let tail = format!(" {} 0`", op.text());
            let by_zero = self.panic_unary(
                panics::DIVISION_BY_ZERO,
                ("division by zero: `", a, &tail),
                unsigned,
                span,
            )?;
            let zero = self.constant(0);
            self.push(Inst::Branch {
                cond: Cond::Eq,
                lhs: b,
                rhs: zero,
                target: by_zero,
            });
        }
        let mid = format!(" {} ", op.text());
        let tail = format!(
            "` overflows `{}`, which ranges from {} to {}",
            int.name(),
            int.min(),
            int.max()
        );
        let overflow =
            self.panic_binary(panics::OVERFLOW, ("`", a, &mid, b, &tail), unsigned, span)?;
        let dst = self.temp();
        self.push(Inst::Arith {
            op: arith,
            int: kind,
            dst,
            lhs: a,
            rhs: b,
            overflow,
        });
        Ok(dst)
    }

    /// `-operand` on an integer of type `int`: `P0001` when the result
    /// does not fit, as for the most negative value of a signed type, or
    /// anything but 0 of an unsigned one.
    fn negate(&mut self, operand: &Expr, int: IntType, span: Span) -> Result<Temp, Diagnostic> {
        let a = self.expr(operand)?;
        let kind = layout::int_kind(int);
        let tail = format!("` overflows `{}`", int.name());
        let overflow = self.panic_unary(panics::OVERFLOW, ("`-", a, &tail), !kind.signed, span)?;
        let zero = self.constant(0);
        let dst = self.temp();
        self.push(Inst::Arith {
            op: Arith::Sub,
            int: kind,
            dst,
            lhs: zero,
            rhs: a,
            overflow,
        });
        Ok(dst)
    }

    /// Jump to `target` unless the condition `cond` holds: a comparison
    /// of integers or `Bool`s is one compare-and-branch, and `&&`, `||`,
    /// `implies` and `!` branch on their operands, without making a
    /// `Bool` first.
    fn branch_unless(&mut self, cond: &Expr, target: Label) -> Result<(), Diagnostic> {
        self.branch_on(cond, false, target)
    }

    /// Jump to `target` when `cond` is `when`.
    fn branch_on(&mut self, cond: &Expr, when: bool, target: Label) -> Result<(), Diagnostic> {
        match &cond.kind {
            ExprKind::Not(operand) => return self.branch_on(operand, !when, target),
            ExprKind::Binary { op, lhs, rhs, .. } => match op {
                // `a && b` is false when either is; `a || b` true when
                // either is; `a implies b` is `!a || b`.
                BinaryOp::And | BinaryOp::Or | BinaryOp::Implies => {
                    let (first_negated, decided) = match op {
                        BinaryOp::And => (false, false),
                        BinaryOp::Or => (false, true),
                        _ => (true, true),
                    };
                    if when == decided {
                        // Either operand decides: jump on each.
                        self.branch_on(lhs, when != first_negated, target)?;
                        return self.branch_on(rhs, when, target);
                    }
                    // Both must agree: skip past when the first decides
                    // the other way.
                    let skip = self.label();
                    self.branch_on(lhs, decided != first_negated, skip)?;
                    self.branch_on(rhs, when, target)?;
                    self.push(Inst::Label(skip));
                    return Ok(());
                }
                BinaryOp::Eq
                | BinaryOp::Ne
                | BinaryOp::Lt
                | BinaryOp::Le
                | BinaryOp::Gt
                | BinaryOp::Ge
                    if text_equality(*op, &lhs.ty).is_none() =>
                {
                    let unsigned = match &lhs.ty {
                        Type::Int(int) => int.min() == 0,
                        Type::Bool | Type::Unit => false,
                        _ => return self.branch_on_value(cond, when, target),
                    };
                    let mut test = comparison(*op, unsigned);
                    if !when {
                        test = test.negated();
                    }
                    let lhs = self.expr(lhs)?;
                    let rhs = self.expr(rhs)?;
                    self.push(Inst::Branch {
                        cond: test,
                        lhs,
                        rhs,
                        target,
                    });
                    return Ok(());
                }
                _ => {}
            },
            _ => {}
        }
        self.branch_on_value(cond, when, target)
    }

    /// Jump to `target` when the `Bool` `cond`, evaluated, is `when`.
    fn branch_on_value(
        &mut self,
        cond: &Expr,
        when: bool,
        target: Label,
    ) -> Result<(), Diagnostic> {
        let value = self.expr(cond)?;
        if when {
            let zero = self.constant(0);
            self.push(Inst::Branch {
                cond: Cond::Ne,
                lhs: value,
                rhs: zero,
                target,
            });
        } else {
            self.push(Inst::JumpIfZero {
                cond: value,
                target,
            });
        }
        Ok(())
    }

    /// `&&`, `||` and `implies`, which evaluate `rhs` only when they need
    /// it.
    fn short_circuit(&mut self, op: BinaryOp, lhs: &Expr, rhs: &Expr) -> Result<Temp, Diagnostic> {
        let result = self.temp();
        let (skip, end) = (self.label(), self.label());
        let left = self.expr(lhs)?;
        // `a && b` and `a implies b` are decided without `b` when `a` is
        // false: `false` and `true`. `a || b` is, when `a` is true.
        let decided = match op {
            BinaryOp::Implies => 1,
            _ => 0,
        };
        if op == BinaryOp::Or {
            self.push(Inst::Copy {
                dst: result,
                src: left,
            });
            self.push(Inst::JumpIfZero {
                cond: left,
                target: skip,
            });
            self.push(Inst::Jump(end));
            self.push(Inst::Label(skip));
        } else {
            self.push(Inst::Const {
                dst: result,
                value: decided,
            });
            self.push(Inst::JumpIfZero {
                cond: left,
                target: end,
            });
        }
        let right = self.expr(rhs)?;
        self.push(Inst::Copy {
            dst: result,
            src: right,
        });
        self.push(Inst::Label(end));
        Ok(result)
    }

    /// An interpolated string: each part's text, each hole's through the
    /// `Display` implementation the checker chose, copied into one new
    /// text object. An `Int` hole is written straight into it (std.rt
    /// `int_length`, `int_put`), with no text of its own to allocate,
    /// copy and free: most interpolations format numbers.
    fn interpolated(&mut self, parts: &[Part]) -> Result<Temp, Diagnostic> {
        /// A part: a text object, a short literal (its static object and
        /// length), or an `Int` and the length of its text.
        enum Piece {
            Text(Temp),
            Literal(Temp, u32),
            Int(Temp, Temp),
        }
        /// The longest literal copied inline, a word at a time.
        const INLINE: usize = 32;
        let mut pieces = Vec::with_capacity(parts.len());
        // Whether a literal shorter than a word is copied as a whole word,
        // writing up to 7 bytes past it: the next part overwrites them,
        // and after the last there must be room.
        let mut slack = false;
        for part in parts {
            match part {
                Part::Text(text) if text.is_empty() => {}
                Part::Text(text) if text.len() <= INLINE => {
                    slack |= text.len() < 8;
                    let object = self.text(text);
                    pieces.push(Piece::Literal(object, text.len() as u32));
                }
                Part::Text(text) => pieces.push(Piece::Text(self.text(text))),
                Part::Hole {
                    value,
                    display,
                    type_args,
                } => {
                    let shown = self.expr(value)?;
                    let (display, type_args) =
                        self.resolve(display, type_args, Some(&value.ty), value.span)?;
                    if matches!(&display, Callee::Intrinsic(name) if name == "Int.to_string") {
                        let length = self.call_runtime("int_length", vec![shown], NOWHERE)?;
                        pieces.push(Piece::Int(shown, length));
                        continue;
                    }
                    let text =
                        self.call(&display, &type_args, vec![shown], &Type::String, value.span)?;
                    pieces.push(Piece::Text(text));
                }
            }
        }
        let mut length = self.constant(0);
        for piece in &pieces {
            let part = match *piece {
                Piece::Text(text) => self.load(text, layout::TEXT_LEN),
                Piece::Literal(_, size) => self.constant(i64::from(size)),
                Piece::Int(_, length) => length,
            };
            length = self.binary_op(BinOp::Add, length, part);
        }
        let object = if slack {
            let seven = self.constant(7);
            let room = self.binary_op(BinOp::Add, length, seven);
            let object = self.call_runtime("text_new", vec![room], NOWHERE)?;
            self.store(object, layout::TEXT_LEN, length);
            object
        } else {
            self.call_runtime("text_new", vec![length], NOWHERE)?
        };
        let mut at = self.constant(0);
        for piece in &pieces {
            at = match *piece {
                Piece::Text(text) => {
                    self.call_runtime("text_put", vec![object, at, text], NOWHERE)?
                }
                Piece::Literal(text, size) => {
                    self.put_literal(object, at, text, size);
                    let size = self.constant(i64::from(size));
                    self.binary_op(BinOp::Add, at, size)
                }
                Piece::Int(value, length) => {
                    self.call_runtime("int_put", vec![object, at, value, length], NOWHERE)?
                }
            };
        }
        for piece in pieces {
            if let Piece::Text(text) = piece {
                self.consume(text)?;
            }
        }
        self.own(object, &Type::String);
        Ok(object)
    }

    /// Copy the `size` bytes of the static text object `text` into the
    /// text object `object` from its byte `at`, a word at a time: the last
    /// word overlaps the one before it, or for fewer than 8 bytes, one
    /// whole word is written, up to 7 bytes past them (the caller leaves
    /// room). Rodata texts are padded to a word, so every load is inside.
    fn put_literal(&mut self, object: Temp, at: Temp, text: Temp, size: u32) {
        let data = layout::TEXT_LEN + layout::WORD;
        let to = self.binary_op(BinOp::Add, object, at);
        let mut offsets: Vec<u32> = (0..size / 8).map(|k| data + 8 * k).collect();
        if size < 8 {
            offsets.push(data);
        } else if size % 8 != 0 {
            offsets.push(data + size - 8);
        }
        for offset in offsets {
            let word = self.load(text, offset);
            self.store(to, offset, word);
        }
    }

    // ---- Statements --------------------------------------------------------

    fn stmt(&mut self, stmt: &Stmt) -> Result<(), Diagnostic> {
        match stmt {
            Stmt::Expr(expr) => {
                let value = self.expr(expr)?;
                self.consume(value)?;
            }
            Stmt::Let { binds, ty, value } => {
                let value = self.expr(value)?;
                if *binds {
                    // The local takes over the value's reference.
                    self.owned.remove(&value);
                    let temp = self.copy(value);
                    let release = self.counted(ty);
                    let ty = self.ty(ty);
                    self.slots.push(Slot { temp, release, ty });
                } else {
                    self.consume(value)?;
                }
            }
            Stmt::Assign { place, value } => self.assign(place, value)?,
            Stmt::Return { value, span } => {
                let value = match value {
                    Some(value) => self.expr(value)?,
                    None => self.constant(0),
                };
                self.return_value(value, *span)?;
            }
            Stmt::Break | Stmt::Continue => {
                let Some(&innermost) = self.loops.last() else {
                    return Err(self.refuse(NOWHERE, "`break` outside a loop"));
                };
                self.release_slots(innermost.height)?;
                self.release_held(Some(self.loops.len()))?;
                let target = if matches!(stmt, Stmt::Break) {
                    innermost.exit
                } else {
                    innermost.next
                };
                self.push(Inst::Jump(target));
            }
            Stmt::While { cond, body } => {
                let (next, exit) = (self.label(), self.label());
                self.push(Inst::Label(next));
                self.branch_unless(cond, exit)?;
                self.loop_body(next, exit, body)?;
                self.push(Inst::Jump(next));
                self.push(Inst::Label(exit));
            }
            Stmt::For {
                binds,
                ty,
                iter,
                body,
            } => {
                if layout::list_item(&self.ty(&iter.ty)).is_some() {
                    self.for_list(*binds, ty, iter, body)?;
                } else {
                    self.for_range(*binds, ty, iter, body)?;
                }
            }
        }
        Ok(())
    }

    /// The body of a loop whose `continue` jumps to `next` and whose
    /// `break` jumps to `exit`.
    fn loop_body(&mut self, next: Label, exit: Label, body: &Expr) -> Result<(), Diagnostic> {
        self.loops.push(Loop {
            next,
            exit,
            height: self.slots.len(),
        });
        let value = self.expr(body)?;
        self.consume(value)?;
        self.loops.pop();
        Ok(())
    }

    /// `for x in r { body }` over a range: each integer from its start
    /// up to but excluding its end, both read once. A range written in
    /// place, `start..end`, is never built: `start` is evaluated first.
    fn for_range(
        &mut self,
        binds: bool,
        ty: &Type,
        iter: &Expr,
        body: &Expr,
    ) -> Result<(), Diagnostic> {
        let (first, end) = match &iter.kind {
            ExprKind::Range { start, end } => {
                let first = self.expr(start)?;
                (first, self.expr(end)?)
            }
            _ => {
                let range = self.expr(iter)?;
                let first = self.load(range, layout::struct_field(0));
                let end = self.load(range, layout::struct_field(1));
                // Integers: the bounds outlive the range.
                self.consume(range)?;
                (first, end)
            }
        };
        let unsigned = matches!(ty, Type::Int(int) if int.min() == 0);
        let at = self.copy(first);
        let (head, next, exit) = (self.label(), self.label(), self.label());
        self.push(Inst::Label(head));
        self.push(Inst::Branch {
            cond: if unsigned { Cond::AboveEq } else { Cond::Ge },
            lhs: at,
            rhs: end,
            target: exit,
        });
        if binds {
            let temp = self.copy(at);
            self.slots.push(Slot {
                temp,
                release: false,
                ty: ty.clone(),
            });
        }
        self.loop_body(next, exit, body)?;
        if binds {
            self.slots.pop();
        }
        self.push(Inst::Label(next));
        // `at` is below `end`, so one more never overflows.
        let one = self.constant(1);
        self.push(Inst::Binary {
            op: BinOp::Add,
            dst: at,
            lhs: at,
            rhs: one,
        });
        self.push(Inst::Jump(head));
        self.push(Inst::Label(exit));
        Ok(())
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

/// Whether `main` can be given a value of `ty` by `_start`: the word 0.
/// Each of these keeps its state, if it has any (the arguments, the
/// environment, the input buffer, the generator), in the runtime's
/// globals.
fn is_stateless_input(ty: &Type) -> bool {
    matches!(ty, Type::Named { name, args } if args.is_empty() && matches!(
        name.as_str(),
        "Stdout" | "Stderr" | "Stdin" | "Env" | "Clock" | "Random" | "Args"
            | "Fs" | "FsRead" | "FsWrite"
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
