//! Lowering: the checked program, and `std.rt`'s, to the backend's code
//! ([`crate::ir`]).
//!
//! Lowering starts at the entry point it generates, `_start`, and lowers
//! each function the first time something calls it, so only reachable
//! code is compiled: `std.rt` is compiled with the program, function by
//! function, as the program needs it. A function is an [`Instance`]: the
//! program it comes from and its name. Monomorphization makes the type
//! arguments part of it.
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
//! for the call, so they are never released by the callee. The retain
//! and release fast paths are inline; the release that drops a count to
//! zero calls `std.rt`'s `free`.
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
//! enough for [`MAX_CALL_DEPTH`] calls of the program's largest frame,
//! with a guard region below it. Every call of a program's function
//! counts its depth ([`Inst::Enter`]) and panics with `P0021` at the
//! interpreter's limit, so recursion never reaches the guard.

use std::collections::{BTreeMap, BTreeSet};

use oska_check::builtins;
use oska_check::program::{
    trait_method, Callee, Construct, Expr, ExprKind, Function, IntType, Part, Place, Program, Stmt,
    Type,
};
use oska_syntax::ast::BinaryOp;
use oska_syntax::{Diagnostic, Span};

use crate::intrinsics::{self, Prelude, Runtime};
use crate::ir::{self, Arith, BinOp, Cond, FnId, Inst, Label, Symbol, Temp, Width};
use crate::layout::{self, Rodata};
use crate::{codes, Locate, NOWHERE};

/// The most temporaries a function may use: each has a stack slot
/// addressed by a 12-bit scaled offset.
const MAX_TEMPS: u32 = 4095;

/// How deeply calls of the program's functions may nest before it
/// panics with `P0021`: the interpreter's limit (`oska_run`'s
/// `MAX_CALL_DEPTH`), so both panic at the same depth.
pub const MAX_CALL_DEPTH: u32 = 50_000;

/// The stack beyond [`MAX_CALL_DEPTH`] frames: room for `std.rt`'s calls
/// below the deepest frame, and for the entry point's.
const STACK_RESERVE: u64 = 8 << 20;

/// The words of the runtime's writable data, at `rt_mem_globals()`.
pub const GLOBALS: u32 = 16;

/// The `P`-codes the native code panics with, as the interpreter's
/// `oska_run::codes` names them.
mod panics {
    pub const OVERFLOW: &str = "P0001";
    pub const DIVISION_BY_ZERO: &str = "P0004";
    pub const CALL_DEPTH_EXCEEDED: &str = "P0021";
    /// The interpreter's message for `P0021`.
    pub const TOO_DEEP: &str = "calls nest too deep: is this recursion missing its base case?";
}

/// Which checked program a function comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Source {
    /// The program being built.
    User,
    /// `std.rt`.
    Runtime,
}

/// A function to compile.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Instance {
    source: Source,
    /// The qualified name, as [`Program::functions`] keys it.
    name: String,
}

/// Lower `user`, with `rt`, the checked `std.rt`, to a module. `locate`
/// gives a user span its file, line and column, for panic reports.
pub fn program(user: &Program, rt: &Program, locate: Locate) -> Result<ir::Module, Diagnostic> {
    let mut lowering = Lowering {
        user,
        rt,
        locate,
        ids: BTreeMap::new(),
        functions: Vec::new(),
        queue: Vec::new(),
        rodata: Rodata::default(),
        largest_frame: 0,
    };
    let entry = lowering.entry()?;
    while let Some((instance, id, origin)) = lowering.queue.pop() {
        lowering.functions[id] = lowering.function(&instance, origin)?;
    }
    lowering.size_stack(entry);
    Ok(ir::Module {
        functions: lowering.functions,
        entry,
        rodata: lowering.rodata.into_bytes(),
        data_size: GLOBALS * layout::WORD,
    })
}

struct Lowering<'p> {
    user: &'p Program,
    rt: &'p Program,
    locate: Locate<'p>,
    /// Every function requested so far.
    ids: BTreeMap<Instance, FnId>,
    /// The lowered functions, by id; a requested one is a placeholder
    /// until it is lowered.
    functions: Vec<ir::Function>,
    /// Functions requested and not lowered yet, with the span of the user
    /// code that first reached them.
    queue: Vec<(Instance, FnId, Span)>,
    rodata: Rodata,
    /// The most temporaries a function of the program uses: its frame
    /// bounds every frame that recursion can stack up.
    largest_frame: u32,
}

impl<'p> Lowering<'p> {
    fn program(&self, source: Source) -> &'p Program {
        match source {
            Source::User => self.user,
            Source::Runtime => self.rt,
        }
    }

    /// The id of the function `name` of `source`, lowered later if it is
    /// new. `origin` is the user code that reached it.
    fn request(&mut self, source: Source, name: &str, origin: Span) -> FnId {
        let instance = Instance {
            source,
            name: name.to_string(),
        };
        if let Some(&id) = self.ids.get(&instance) {
            return id;
        }
        let id = self.reserve(qualified(&instance));
        self.ids.insert(instance.clone(), id);
        self.queue.push((instance, id, origin));
        id
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

        let mut body = Body::new(self, Source::User, None);
        if let Some(main) = user.functions.get("main") {
            body.call_main(main)?;
        }
        let zero = body.constant(0);
        body.call_runtime("exit", vec![zero], NOWHERE)?;
        body.push(Inst::Return(zero));
        self.functions[run] = body.finish("_main", 0, NOWHERE)?;

        let mut body = Body::new(self, Source::User, None);
        let size = body.constant(0);
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

    /// Fill in the size of the stack `_start` maps: [`MAX_CALL_DEPTH`] of
    /// the largest frame (its temporaries, the callee-saved registers and
    /// the frame record), and [`STACK_RESERVE`].
    fn size_stack(&mut self, start: FnId) {
        let frame = 8 * u64::from(self.largest_frame) + 8 * 9 + 32;
        let size = (u64::from(MAX_CALL_DEPTH) * frame + STACK_RESERVE).next_multiple_of(1 << 16);
        if let Some(Inst::Const { value, .. }) = self.functions[start].insts.first_mut() {
            *value = size as i64;
        }
    }

    /// Lower the function `instance` names.
    fn function(&mut self, instance: &Instance, origin: Span) -> Result<ir::Function, Diagnostic> {
        let source = instance.source;
        let program = self.program(source);
        let runtime_origin = (source == Source::Runtime).then_some(origin);
        let Some(function) = program.functions.get(&instance.name) else {
            return Err(refusal(
                origin,
                &format!("a call of `{}`", instance.name),
                runtime_origin.is_some(),
            ));
        };
        let at = |span: Span| runtime_origin.unwrap_or(span);
        if !function.generics.is_empty() {
            return Err(refusal(
                at(function.body.span),
                &format!("the generic function `{}`", function.name),
                runtime_origin.is_some(),
            ));
        }
        if let Some(clause) = function
            .contracts
            .requires
            .iter()
            .chain(function.contracts.ensures.iter().map(|e| &e.clause))
            .next()
        {
            return Err(refusal(
                at(clause.span),
                "a function with `requires` or `ensures`",
                runtime_origin.is_some(),
            ));
        }
        if function.params.len() + usize::from(function.receiver) > ir::MAX_ARGS {
            return Err(refusal(
                at(function.body.span),
                "a function with more than 8 parameters",
                runtime_origin.is_some(),
            ));
        }
        let mut body = Body::new(self, source, runtime_origin);
        if function.receiver {
            let receiver = body.temp();
            body.receiver = Some(receiver);
        }
        for param in &function.params {
            let temp = body.temp();
            if param.binds {
                // Borrowed from the caller: never released here.
                body.slots.push(Slot {
                    temp,
                    release: false,
                });
            }
        }
        let params = body.temps;
        let value = body.expr(&function.body)?;
        body.leave_function(value)?;
        let lowered = body.finish(&qualified(instance), params, function.body.span)?;
        if source == Source::User {
            self.largest_frame = self.largest_frame.max(lowered.temps);
        }
        Ok(lowered)
    }
}

/// How a function is named in the code, for reading it.
fn qualified(instance: &Instance) -> String {
    match instance.source {
        Source::User => instance.name.clone(),
        Source::Runtime => format!("std.rt.{}", instance.name),
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
#[derive(Debug, Clone, Copy)]
struct Slot {
    temp: Temp,
    /// Whether it holds a reference of its own, released when it goes out
    /// of scope: a counted local's, not a parameter's.
    release: bool,
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
    receiver: Option<Temp>,
    /// The innermost loops.
    loops: Vec<Loop>,
    /// The temporaries holding a reference that is theirs to release.
    owned: BTreeSet<Temp>,
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
            owned: BTreeSet::new(),
        }
    }

    fn finish(mut self, name: &str, params: u32, span: Span) -> Result<ir::Function, Diagnostic> {
        if self.temps > MAX_TEMPS {
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

    /// Whether values of `ty` are counted.
    fn counted(&self, ty: &Type) -> bool {
        layout::is_counted(self.program(), ty)
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

    /// Drop one reference to the object `value`, unless it is static, and
    /// free it when that was the last.
    fn release(&mut self, value: Temp) -> Result<(), Diagnostic> {
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
        self.call_runtime("free", vec![value], NOWHERE)?;
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
        if self.owned.remove(&value) {
            self.release(value)?;
        }
        Ok(())
    }

    /// Release the slots above `height`, innermost first, without taking
    /// them out of scope: for code that leaves their scope.
    fn release_slots(&mut self, height: usize) -> Result<(), Diagnostic> {
        let slots: Vec<Slot> = self.slots.get(height..).unwrap_or_default().to_vec();
        for slot in slots.iter().rev() {
            if slot.release {
                self.release(slot.temp)?;
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
    /// every local.
    fn leave_function(&mut self, value: Temp) -> Result<(), Diagnostic> {
        self.owned.remove(&value);
        self.release_slots(0)?;
        self.push(Inst::Return(value));
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
        let unlocated = Diagnostic::new(code, "", span);
        let located = match self.source {
            Source::User => (self.lowering.locate)(unlocated),
            Source::Runtime => unlocated,
        };
        let before = match &located.location {
            Some(location) => format!("{location}: error[{code}]: "),
            None => format!("error[{code}]: "),
        };
        let after = format!(" (bytes {}..{})\n", located.span.start, located.span.end);
        (before, after)
    }

    /// A cold block that panics with `code` and the fixed `message`.
    fn panic_static(
        &mut self,
        code: &'static str,
        message: &str,
        span: Span,
    ) -> Result<Label, Diagnostic> {
        let (before, after) = self.report(code, span);
        self.cold_block(|body| {
            let report = body.text(&format!("{before}{message}{after}"));
            let unit = body.call_runtime("panic", vec![report], NOWHERE)?;
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
            return Err(self.refuse(span, "a call with more than 8 arguments"));
        }
        let dst = self.temp();
        self.push(Inst::Call { dst, callee, args });
        Ok(dst)
    }

    /// Call the function `name` of `source`.
    fn call_function(
        &mut self,
        source: Source,
        name: &str,
        args: Vec<Temp>,
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        let origin = self.origin.unwrap_or(span);
        let id = self.lowering.request(source, name, origin);
        self.call_id(id, args, span)
    }

    /// Call the `std.rt` function `name`.
    fn call_runtime(
        &mut self,
        name: &str,
        args: Vec<Temp>,
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        self.call_function(Source::Runtime, name, args, span)
    }

    /// Call the program's function `name`, counting the call's depth: at
    /// [`MAX_CALL_DEPTH`] it panics with `P0021` at `span` instead.
    fn call_counted(
        &mut self,
        name: &str,
        args: Vec<Temp>,
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        let exhausted = self.panic_static(panics::CALL_DEPTH_EXCEEDED, panics::TOO_DEEP, span)?;
        self.push(Inst::Enter { exhausted });
        let result = self.call_function(Source::User, name, args, span)?;
        self.push(Inst::Leave);
        Ok(result)
    }

    /// A call of `callee` with `args`, evaluated already, at `span`,
    /// whose result is of type `ret`. The call releases the arguments
    /// that hold a reference of their own, and its result holds one when
    /// `ret` is counted.
    fn call(
        &mut self,
        callee: &Callee,
        args: Vec<Temp>,
        ret: &Type,
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        let result = match callee {
            Callee::Function(name) if self.program().functions.contains_key(name) => {
                match self.source {
                    Source::User => self.call_counted(name, args.clone(), span)?,
                    Source::Runtime => {
                        self.call_function(Source::Runtime, name, args.clone(), span)?
                    }
                }
            }
            Callee::Function(name) => match intrinsics::runtime(name) {
                // Only `std.rt` may name an intrinsic (the checker's
                // `E0209`), and the backend holds it to that too.
                Some(runtime) if self.source == Source::Runtime => {
                    self.runtime_intrinsic(runtime, &args)
                }
                _ => return Err(self.refuse(span, &format!("a call of `{name}`"))),
            },
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
        if self.counted(ret) {
            self.owned.insert(result);
        }
        Ok(result)
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
        }
    }

    /// Call `main` with its capabilities and, when it returns `Err(e)`,
    /// hand `e`'s text to `std.rt`'s `main_failed`, which exits. As in
    /// the interpreter, the call of `main` is not counted.
    fn call_main(&mut self, main: &Function) -> Result<(), Diagnostic> {
        let span = main.body.span;
        let mut args = Vec::new();
        for param in &main.params {
            if !is_stateless_capability(&param.ty) {
                return Err(self.refuse(span, &format!("a `main` given `{}`", param.ty)));
            }
            args.push(self.constant(0));
        }
        let result = self.call_function(Source::User, "main", args, span)?;
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

    /// The `Display` text of `value`, of type `ty`.
    fn display(&mut self, ty: &Type, value: Temp, span: Span) -> Result<Temp, Diagnostic> {
        let name = match ty {
            Type::String => return Ok(value),
            Type::Named { name, args } if args.is_empty() => name,
            _ => return Err(self.refuse(span, &format!("the `Display` text of a `{ty}`"))),
        };
        let user = trait_method(name, "Display", "to_string");
        let callee = if self.program().functions.contains_key(&user) {
            Callee::Function(user)
        } else {
            Callee::Intrinsic(format!("{name}.to_string"))
        };
        self.call(&callee, vec![value], &Type::String, span)
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
            ExprKind::SelfValue => match self.receiver {
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
                let cond = self.expr(cond)?;
                let result = self.temp();
                let (otherwise, end) = (self.label(), self.label());
                self.push(Inst::JumpIfZero {
                    cond,
                    target: otherwise,
                });
                let then_value = self.expr(then_branch)?;
                self.push(Inst::Copy {
                    dst: result,
                    src: then_value,
                });
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
                // A branch may give a static object, which needs no
                // release; releasing one is harmless, so the result is
                // released if either branch's would be.
                let then_owned = self.owned.remove(&then_value);
                if self.owned.remove(&else_value) || then_owned {
                    self.owned.insert(result);
                }
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
            ExprKind::Call {
                callee,
                type_args,
                args,
            } => {
                if !type_args.is_empty() {
                    return Err(self.refuse(span, "a call of a generic function"));
                }
                let mut values = Vec::with_capacity(args.len());
                for arg in args {
                    values.push(self.expr(arg)?);
                }
                self.call(callee, values, &expr.ty, span)
            }
            other => Err(self.refuse(span, &describe(other))),
        }
    }

    /// The value of the local or receiver in `temp`, of type `ty`: a copy,
    /// so that assigning the local later does not change it, holding a
    /// reference of its own when `ty` is counted.
    fn read(&mut self, temp: Temp, ty: &Type) -> Temp {
        let value = self.copy(temp);
        if self.counted(ty) {
            self.retain(value);
            self.owned.insert(value);
        }
        value
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
        if matches!(arith, Arith::Div | Arith::Rem) {
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
    /// text object.
    fn interpolated(&mut self, parts: &[Part]) -> Result<Temp, Diagnostic> {
        let mut texts = Vec::with_capacity(parts.len());
        for part in parts {
            match part {
                Part::Text(text) => texts.push(self.text(text)),
                Part::Hole {
                    value,
                    display,
                    type_args,
                } => {
                    if !type_args.is_empty() {
                        return Err(self.refuse(value.span, "a generic `Display` implementation"));
                    }
                    let shown = self.expr(value)?;
                    let text = self.call(display, vec![shown], &Type::String, value.span)?;
                    texts.push(text);
                }
            }
        }
        let mut length = self.constant(0);
        for &text in &texts {
            let part = self.load(text, layout::TEXT_LEN);
            length = self.binary_op(BinOp::Add, length, part);
        }
        let object = self.call_runtime("text_new", vec![length], NOWHERE)?;
        let mut at = self.constant(0);
        for &text in &texts {
            at = self.call_runtime("text_put", vec![object, at, text], NOWHERE)?;
        }
        for text in texts {
            self.consume(text)?;
        }
        self.owned.insert(object);
        Ok(object)
    }

    /// A struct or enum value. A fieldless enum is its variant's index;
    /// any other value whose fields are constants is a static object.
    fn construct(
        &mut self,
        expr: &Expr,
        ctor: &Construct,
        fields: &[Expr],
    ) -> Result<Temp, Diagnostic> {
        let program = self.program();
        let Some(decl) = program.types.get(&ctor.ty) else {
            return Err(self.refuse(expr.span, &format!("a `{}` value", ctor.ty)));
        };
        let mut words = Vec::new();
        if let Some(variant) = &ctor.variant {
            let index = self.variant(&ctor.ty, variant, expr.span)?;
            if layout::is_fieldless_enum(decl) {
                return Ok(self.constant(index as i64));
            }
            words.push(index as i64);
        }
        for field in fields {
            match constant_word(program, field) {
                Some(word) => words.push(word),
                None => {
                    return Err(self.refuse(
                        expr.span,
                        &format!("a `{}` value built at run time", expr.ty),
                    ))
                }
            }
        }
        let at = self.lowering.rodata.object(&words);
        Ok(self.rodata(at))
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
                    self.slots.push(Slot { temp, release });
                } else {
                    self.consume(value)?;
                }
            }
            Stmt::Assign { place, value } => {
                let src = self.expr(value)?;
                let slot = match place {
                    Place::Local(slot) => self.slots.get(*slot).copied(),
                    Place::SelfValue | Place::Field { .. } => None,
                };
                let Some(slot) = slot else {
                    return Err(self.refuse(value.span, "assigning to a field or `self`"));
                };
                if slot.release {
                    self.release(slot.temp)?;
                }
                self.owned.remove(&src);
                self.push(Inst::Copy {
                    dst: slot.temp,
                    src,
                });
            }
            Stmt::Return { value, .. } => {
                let value = match value {
                    Some(value) => self.expr(value)?,
                    None => self.constant(0),
                };
                self.leave_function(value)?;
            }
            Stmt::Break | Stmt::Continue => {
                let Some(&innermost) = self.loops.last() else {
                    return Err(self.refuse(NOWHERE, "`break` outside a loop"));
                };
                self.release_slots(innermost.height)?;
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
                let cond = self.expr(cond)?;
                self.push(Inst::JumpIfZero { cond, target: exit });
                self.loop_body(next, exit, body)?;
                self.push(Inst::Jump(next));
                self.push(Inst::Label(exit));
            }
            Stmt::For {
                binds,
                ty,
                iter,
                body,
            } => self.for_range(*binds, ty, iter, body)?,
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

    /// `for x in start..end { body }`: each integer from `start` up to
    /// but excluding `end`, both evaluated once, first `start`.
    fn for_range(
        &mut self,
        binds: bool,
        ty: &Type,
        iter: &Expr,
        body: &Expr,
    ) -> Result<(), Diagnostic> {
        let ExprKind::Range { start, end } = &iter.kind else {
            return Err(self.refuse(iter.span, "a `for` loop over a list or a range value"));
        };
        let unsigned = matches!(ty, Type::Int(int) if int.min() == 0);
        let first = self.expr(start)?;
        let end = self.expr(end)?;
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
fn is_stateless_capability(ty: &Type) -> bool {
    matches!(ty, Type::Named { name, args } if args.is_empty() && (name == "Stdout" || name == "Stderr"))
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
        ExprKind::Closure(_) => "a closure".to_string(),
        ExprKind::CallValue { .. } => "a call of a function value".to_string(),
        ExprKind::MutatingCall { .. } => "a mutating method call".to_string(),
        ExprKind::Range { .. } => "a range outside a `for` loop".to_string(),
        ExprKind::Try { .. } => "`?`".to_string(),
        ExprKind::FnValue { .. } => "a function used as a value".to_string(),
        ExprKind::Match { .. } => "`match`".to_string(),
        ExprKind::Scope(_) => "`scope`".to_string(),
        ExprKind::Unsupported(what) => what.clone(),
        _ => "this expression".to_string(),
    }
}
