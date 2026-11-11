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

use std::collections::BTreeMap;

use oska_check::builtins;
use oska_check::program::{
    trait_method, Callee, Construct, Expr, ExprKind, Function, IntType, Place, Program, Stmt, Type,
};
use oska_syntax::ast::BinaryOp;
use oska_syntax::{Diagnostic, Span};

use crate::intrinsics::{self, Prelude, Runtime};
use crate::ir::{self, BinOp, Cond, FnId, Inst, Label, Symbol, Temp, Width};
use crate::layout::{self, Rodata};
use crate::{codes, NOWHERE};

/// The most temporaries a function may use: each has a stack slot
/// addressed by a 12-bit scaled offset.
const MAX_TEMPS: u32 = 4095;

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

/// Lower `user`, with `rt`, the checked `std.rt`, to a module.
pub fn program(user: &Program, rt: &Program) -> Result<ir::Module, Diagnostic> {
    let mut lowering = Lowering {
        user,
        rt,
        ids: BTreeMap::new(),
        functions: Vec::new(),
        queue: Vec::new(),
        rodata: Rodata::default(),
    };
    let entry = lowering.entry()?;
    while let Some((instance, id, origin)) = lowering.queue.pop() {
        lowering.functions[id] = lowering.function(&instance, origin)?;
    }
    Ok(ir::Module {
        functions: lowering.functions,
        entry,
        rodata: lowering.rodata.into_bytes(),
        data_size: layout::WORD,
    })
}

struct Lowering<'p> {
    user: &'p Program,
    rt: &'p Program,
    /// Every function requested so far.
    ids: BTreeMap<Instance, FnId>,
    /// The lowered functions, by id; a requested one is a placeholder
    /// until it is lowered.
    functions: Vec<ir::Function>,
    /// Functions requested and not lowered yet, with the span of the user
    /// code that first reached them.
    queue: Vec<(Instance, FnId, Span)>,
    rodata: Rodata,
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
        });
        self.functions.len() - 1
    }

    /// The entry point, `_start`: call `main` with its capabilities, report
    /// an `Err` it returns, and exit.
    fn entry(&mut self) -> Result<FnId, Diagnostic> {
        let user = self.user;
        let id = self.reserve("_start".to_string());
        let mut body = Body::new(self, Source::User, None);
        if let Some(main) = user.functions.get("main") {
            body.call_main(main)?;
        }
        let zero = body.constant(0);
        body.call_runtime("exit", vec![zero], NOWHERE)?;
        body.push(Inst::Return(zero));
        self.functions[id] = body.finish("_start", 0, NOWHERE)?;
        Ok(id)
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
                body.slots.push(temp);
            }
        }
        let params = body.temps;
        let value = body.expr(&function.body)?;
        body.push(Inst::Return(value));
        body.finish(&qualified(instance), params, function.body.span)
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

/// One function being lowered.
struct Body<'l, 'p> {
    lowering: &'l mut Lowering<'p>,
    source: Source,
    /// For a function of `std.rt`, the span of the user code that reached
    /// it, where its refusals are reported.
    origin: Option<Span>,
    insts: Vec<Inst>,
    temps: u32,
    labels: u32,
    /// The temporary of each local slot in scope, by slot number.
    slots: Vec<Temp>,
    /// The receiver of a method.
    receiver: Option<Temp>,
    /// The innermost loops: where `continue` and `break` jump.
    loops: Vec<(Label, Label)>,
}

impl<'l, 'p> Body<'l, 'p> {
    fn new(lowering: &'l mut Lowering<'p>, source: Source, origin: Option<Span>) -> Self {
        Body {
            lowering,
            source,
            origin,
            insts: Vec::new(),
            temps: 0,
            labels: 0,
            slots: Vec::new(),
            receiver: None,
            loops: Vec::new(),
        }
    }

    fn finish(self, name: &str, params: u32, span: Span) -> Result<ir::Function, Diagnostic> {
        if self.temps > MAX_TEMPS {
            return Err(self.refuse(span, "a function this large"));
        }
        Ok(ir::Function {
            name: name.to_string(),
            params,
            temps: self.temps,
            labels: self.labels,
            insts: self.insts,
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

    fn rodata(&mut self, at: u32) -> Temp {
        let dst = self.temp();
        self.push(Inst::Addr {
            dst,
            symbol: Symbol::Rodata(at),
        });
        dst
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

    /// Call the function `name` of the program this function is in.
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

    /// A call of `callee` with `args`, evaluated already, at `span`.
    fn call(&mut self, callee: &Callee, args: Vec<Temp>, span: Span) -> Result<Temp, Diagnostic> {
        match callee {
            Callee::Function(name) if self.program().functions.contains_key(name) => {
                self.call_function(self.source, name, args, span)
            }
            Callee::Function(name) => match intrinsics::runtime(name) {
                // Only `std.rt` may name an intrinsic (the checker's
                // `E0209`), and the backend holds it to that too.
                Some(runtime) if self.source == Source::Runtime => {
                    Ok(self.runtime_intrinsic(runtime, args))
                }
                _ => Err(self.refuse(span, &format!("a call of `{name}`"))),
            },
            Callee::Intrinsic(name) => match intrinsics::prelude(name) {
                Some(Prelude::Runtime(function)) => self.call_runtime(function, args, span),
                Some(Prelude::Identity) => match args.first() {
                    Some(&value) => Ok(value),
                    None => Err(self.refuse(span, &format!("`{name}`"))),
                },
                Some(Prelude::Field(index)) => match args.first() {
                    Some(&value) => Ok(self.load(value, layout::struct_field(index))),
                    None => Err(self.refuse(span, &format!("`{name}`"))),
                },
                None => Err(self.refuse(span, &format!("`{name}`"))),
            },
            Callee::Trait { trait_name, method } => Err(self.refuse(
                span,
                &format!("a call of `{trait_name}.{method}` on a value of a type parameter"),
            )),
        }
    }

    /// A `std.rt` intrinsic applied to `args`. The checker has matched
    /// their number to its declaration.
    fn runtime_intrinsic(&mut self, runtime: Runtime, args: Vec<Temp>) -> Temp {
        let arg = |i: usize| args.get(i).copied().unwrap_or_default();
        match runtime {
            Runtime::Syscall(number) => {
                let dst = self.temp();
                self.push(Inst::Syscall { dst, number, args });
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
            Runtime::Binary(op) => {
                let dst = self.temp();
                self.push(Inst::Binary {
                    op,
                    dst,
                    lhs: arg(0),
                    rhs: arg(1),
                });
                dst
            }
            Runtime::Identity => arg(0),
        }
    }

    /// Call `main` with its capabilities and, when it returns `Err(e)`,
    /// hand `e`'s text to `std.rt`'s `main_failed`, which exits.
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
                let is_err = self.temp();
                self.push(Inst::Compare {
                    cond: Cond::Eq,
                    dst: is_err,
                    lhs: tag,
                    rhs: err,
                });
                let ok = self.label();
                self.push(Inst::JumpIfZero {
                    cond: is_err,
                    target: ok,
                });
                let error = self.load(result, layout::enum_field(0));
                let text = self.display(&args[1], error, span)?;
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
        self.call(&callee, vec![value], span)
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
            ExprKind::Str(text) => {
                let at = self.lowering.rodata.text(text);
                Ok(self.rodata(at))
            }
            ExprKind::Local(slot) => match self.slots.get(*slot) {
                Some(&temp) => Ok(self.copy(temp)),
                None => Err(self.refuse(span, "a local slot out of scope")),
            },
            ExprKind::SelfValue => match self.receiver {
                Some(receiver) => Ok(self.copy(receiver)),
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
                self.slots.truncate(height);
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
                Ok(result)
            }
            ExprKind::Not(operand) => {
                let value = self.expr(operand)?;
                let one = self.constant(1);
                let dst = self.temp();
                self.push(Inst::Binary {
                    op: BinOp::Xor,
                    dst,
                    lhs: value,
                    rhs: one,
                });
                Ok(dst)
            }
            ExprKind::Binary { op, lhs, rhs, int } => self.binary(*op, lhs, rhs, *int, span),
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
                self.call(callee, values, span)
            }
            other => Err(self.refuse(span, &describe(other))),
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
        match op {
            BinaryOp::And | BinaryOp::Or | BinaryOp::Implies => self.short_circuit(op, lhs, rhs),
            BinaryOp::Eq
            | BinaryOp::Ne
            | BinaryOp::Lt
            | BinaryOp::Le
            | BinaryOp::Gt
            | BinaryOp::Ge
                if int.is_none() =>
            {
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
            _ => Err(self.refuse(span, &format!("integer arithmetic (`{}`)", op.text()))),
        }
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
                self.expr(expr)?;
            }
            Stmt::Let { binds, value, .. } => {
                let value = self.expr(value)?;
                if *binds {
                    let slot = self.copy(value);
                    self.slots.push(slot);
                }
            }
            Stmt::Assign { place, value } => {
                let src = self.expr(value)?;
                let slot = match place {
                    Place::Local(slot) => self.slots.get(*slot).copied(),
                    Place::SelfValue | Place::Field { .. } => None,
                };
                match slot {
                    Some(dst) => self.push(Inst::Copy { dst, src }),
                    None => return Err(self.refuse(value.span, "assigning to a field or `self`")),
                }
            }
            Stmt::Return { value, .. } => {
                let value = match value {
                    Some(value) => self.expr(value)?,
                    None => self.constant(0),
                };
                self.push(Inst::Return(value));
            }
            Stmt::Break | Stmt::Continue => {
                let Some(&(next, exit)) = self.loops.last() else {
                    return Err(self.refuse(NOWHERE, "`break` outside a loop"));
                };
                let target = if matches!(stmt, Stmt::Break) {
                    exit
                } else {
                    next
                };
                self.push(Inst::Jump(target));
            }
            Stmt::While { cond, body } => {
                let (next, exit) = (self.label(), self.label());
                self.push(Inst::Label(next));
                let cond = self.expr(cond)?;
                self.push(Inst::JumpIfZero { cond, target: exit });
                self.loops.push((next, exit));
                self.expr(body)?;
                self.loops.pop();
                self.push(Inst::Jump(next));
                self.push(Inst::Label(exit));
            }
            Stmt::For { iter, .. } => return Err(self.refuse(iter.span, "a `for` loop")),
        }
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
        ExprKind::Interpolated(_) => "an interpolated string".to_string(),
        ExprKind::Neg { .. } => "integer negation".to_string(),
        ExprKind::List(_) => "a list".to_string(),
        ExprKind::Field { .. } => "reading a field".to_string(),
        ExprKind::Closure(_) => "a closure".to_string(),
        ExprKind::CallValue { .. } => "a call of a function value".to_string(),
        ExprKind::MutatingCall { .. } => "a mutating method call".to_string(),
        ExprKind::Range { .. } => "a range".to_string(),
        ExprKind::Try { .. } => "`?`".to_string(),
        ExprKind::FnValue { .. } => "a function used as a value".to_string(),
        ExprKind::Match { .. } => "`match`".to_string(),
        ExprKind::Scope(_) => "`scope`".to_string(),
        ExprKind::Unsupported(what) => what.clone(),
        _ => "this expression".to_string(),
    }
}
