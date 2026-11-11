//! Function values: closures, named functions and methods used as values
//! (`cap fn` values among them: the type erases the difference), calls of
//! them, and `List.map`.
//!
//! # Representation
//!
//! A function value is an object (`layout.rs`): `[count][code][drop]`,
//! then what it captured. `code` is the address of a function that takes
//! the object itself, then the call's arguments; `drop` is the address of
//! the function that frees the object when its last reference goes:
//! `std.rt`'s `free` when it captured nothing counted, else a generated
//! one that releases the captures first ([`Instance::ClosureDrop`]).
//! Releasing a value of a function type calls `drop` through the object,
//! since the type does not say what it captured.
//!
//! - A **closure** captures, by value, the slots its body reads from the
//!   enclosing frame and the receiver when it reads `self`: each copied
//!   and retained into the object, so assigning the original later never
//!   changes the copy. Its code ([`Instance::Closure`]) loads them back
//!   into the slot numbers the checker gave them, borrowed from the
//!   object, which the caller holds for the call. Its body has no
//!   contracts of its own (`ensuring` is `None`): a `return` in it returns
//!   from the closure.
//! - A **named function** used as a value captures nothing. Its code is a
//!   thunk ([`Instance::Thunk`]) that drops the object argument and calls
//!   the function, or the prelude intrinsic, with the rest.
//!
//! # Calls
//!
//! A call of a function value counts its depth at the call, as the
//! interpreter's `call_value` does (`P0021` at the call's span). A
//! function's `requires` clauses name the call's span too, which the
//! thunk does not know: so every function value's code returns a second
//! result, a status, 0 when the call went ahead, else the static text of
//! the broken clause's message, which the caller panics with (`P0006`).
//! A thunk of an intrinsic undoes the count, as the interpreter does not
//! count an intrinsic's call. An intrinsic that may panic (`String.slice`,
//! `String.repeat`, `Env.secret`) is refused as a value (`E0390`): its
//! report names the call, which its thunk does not know.

use std::sync::Arc;

use oska_check::program::{Callee, Closure, Expr, ExprKind, Type};
use oska_syntax::{Diagnostic, Span};

use super::{body_end, panics, Body, Instance, Receiver, Slot, Source};
use crate::intrinsics::{self, Prelude};
use crate::ir::{self, Cond, FnId, Inst, Label, Symbol, Temp};
use crate::layout;
use crate::NOWHERE;

/// A closure's code, as it is compiled: once for each instance of the
/// function it is written in.
#[derive(Debug, Clone)]
pub(super) struct ClosureCode {
    pub(super) code: Arc<Closure>,
    pub(super) source: Source,
    /// The enclosing instance's type arguments, for the body's types.
    pub(super) subst: Vec<(String, Type)>,
    /// The concrete type of each captured slot, in the order of
    /// [`Closure::captures`], then of the receiver when it captures one.
    pub(super) captures: Vec<Type>,
}

/// What a closure's code is told apart by: its code (by address, which
/// stays put while the checked program is borrowed), the program, and the
/// enclosing instance's type arguments.
pub(super) type ClosureKey = (usize, Source, Vec<(String, Type)>);

impl Body<'_, '_> {
    /// A closure value, `expr`: the object holding its code and a copy of
    /// what it captures.
    pub(super) fn closure(&mut self, expr: &Expr, code: &Arc<Closure>) -> Result<Temp, Diagnostic> {
        if code.params.len() >= ir::MAX_ARGS {
            return Err(self.refuse(expr.span, "a closure with more than 7 parameters"));
        }
        let mut values = Vec::new();
        let mut captures = Vec::new();
        for &slot in &code.captures {
            let Some(Slot { temp, ty, .. }) = self.slots.get(slot).cloned() else {
                return Err(self.refuse(expr.span, "a closure capturing a slot out of scope"));
            };
            values.push(self.read(temp, &ty));
            captures.push(ty);
        }
        if code.captures_self {
            let Some(Receiver { temp, ty, .. }) = self.receiver.clone() else {
                return Err(self.refuse(expr.span, "`self` outside a method"));
            };
            values.push(self.read(temp, &ty));
            captures.push(ty);
        }
        let counted = captures.iter().any(|ty| self.counted(ty));
        let index = self.lowering.closure_index(ClosureCode {
            code: Arc::clone(code),
            source: self.source,
            subst: self.subst.clone(),
            captures,
        });
        let origin = self.origin.unwrap_or(expr.span);
        let code_id = self.lowering.request(Instance::Closure(index), origin);
        let drop_id = if counted {
            self.lowering.request(Instance::ClosureDrop(index), origin)
        } else {
            self.free_id()
        };
        let object = self.function_object(code_id, drop_id, &values)?;
        self.own(object, &expr.ty);
        Ok(object)
    }

    /// A named function or method used as a value, `expr`: an object
    /// holding the code of a thunk that calls it.
    pub(super) fn fn_value(
        &mut self,
        expr: &Expr,
        callee: &Callee,
        type_args: &[Type],
    ) -> Result<Temp, Diagnostic> {
        let ty = self.ty(&expr.ty);
        let Type::Fn { params, .. } = &ty else {
            return Err(self.refuse(expr.span, &format!("a function value of type `{ty}`")));
        };
        if params.len() >= ir::MAX_ARGS {
            return Err(self.refuse(expr.span, "a function value with more than 7 parameters"));
        }
        // A trait's method is the implementation for the type of the
        // value it is called on: its first parameter's.
        let (callee, type_args) = self.resolve(callee, type_args, params.first(), expr.span)?;
        let (intrinsic, name) = match callee {
            Callee::Function(name) => (false, name),
            // A panic names the call's span, which a thunk does not know.
            Callee::Intrinsic(name) if may_panic(&name) => {
                return Err(self.refuse(
                    expr.span,
                    &format!("`{name}`, which may panic, used as a function value"),
                ))
            }
            Callee::Intrinsic(name) => (true, name),
            Callee::Trait { .. } => return Err(self.refuse(expr.span, "a function value")),
        };
        let thunk = Instance::Thunk {
            source: self.source,
            intrinsic,
            name,
            type_args,
            ty: ty.clone(),
        };
        let code = self
            .lowering
            .request(thunk, self.origin.unwrap_or(expr.span));
        let drop = self.free_id();
        let object = self.function_object(code, drop, &[])?;
        self.own(object, &ty);
        Ok(object)
    }

    /// `std.rt`'s `free`, as a function value's drop function.
    fn free_id(&mut self) -> FnId {
        let free = Instance::Function {
            source: Source::Runtime,
            name: "free".to_string(),
            type_args: Vec::new(),
        };
        let origin = self.origin.unwrap_or(NOWHERE);
        self.lowering.request(free, origin)
    }

    /// A new function value object running `code`, freed by `drop`, that
    /// takes over the references of `captured`.
    fn function_object(
        &mut self,
        code: FnId,
        drop: FnId,
        captured: &[Temp],
    ) -> Result<Temp, Diagnostic> {
        let size = i64::from(layout::FN_CAPTURES) + 8 * captured.len() as i64;
        let object = self.alloc(size)?;
        let code = self.function_address(code);
        self.store(object, layout::FN_CODE, code);
        let drop = self.function_address(drop);
        self.store(object, layout::FN_DROP, drop);
        for (index, &value) in captured.iter().enumerate() {
            self.store(object, layout::FN_CAPTURES + 8 * index as u32, value);
            self.owned.remove(&value);
        }
        Ok(object)
    }

    /// The address of the code of the function `id`.
    pub(super) fn function_address(&mut self, id: FnId) -> Temp {
        let dst = self.temp();
        self.push(Inst::Addr {
            dst,
            symbol: Symbol::Function(id),
        });
        dst
    }

    /// `callee(args)`, a call of a function value, for `expr`.
    pub(super) fn call_value(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        expr: &Expr,
    ) -> Result<Temp, Diagnostic> {
        let function = self.expr(callee)?;
        let mut values = Vec::with_capacity(args.len());
        for arg in args {
            values.push(self.expr(arg)?);
        }
        let result = self.invoke(function, &values, expr.span)?;
        self.consume(function)?;
        for value in values {
            self.consume(value)?;
        }
        self.own(result, &expr.ty);
        Ok(result)
    }

    /// Call the function value `function` with `args`, at `span`,
    /// borrowing both: its result, holding a reference of its own when
    /// its type is counted. The call counts its depth, and panics with
    /// `P0006` when the status the code returns names a broken `requires`
    /// clause.
    pub(super) fn invoke(
        &mut self,
        function: Temp,
        args: &[Temp],
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        if args.len() >= ir::MAX_ARGS {
            return Err(self.refuse(span, "a call with more than 7 arguments"));
        }
        let exhausted = self.panic_static(panics::CALL_DEPTH_EXCEEDED, panics::TOO_DEEP, span)?;
        self.push(Inst::Enter { exhausted });
        let target = self.load(function, layout::FN_CODE);
        let dst = self.temp();
        let mut all = vec![function];
        all.extend_from_slice(args);
        self.push(Inst::CallIndirect {
            dst,
            target,
            args: all,
        });
        self.push(Inst::Leave);
        let status = self.second_result();
        let broken = self.broken_precondition(status, span)?;
        let zero = self.constant(0);
        self.push(Inst::Branch {
            cond: Cond::Ne,
            lhs: status,
            rhs: zero,
            target: broken,
        });
        Ok(dst)
    }

    /// The cold block a call of a function value at `span` panics in when
    /// the code's `status` is the text of a broken `requires` clause
    /// (`P0006`). The code words the function by its key; the report
    /// names it as the caller's module writes it, as the interpreter's
    /// does. In a package, where some key reads differently at `span`,
    /// the block picks the wording for the status it was given among
    /// every `requires` message of the program (texts are shared, so the
    /// same message is the same object).
    fn broken_precondition(&mut self, status: Temp, span: Span) -> Result<Label, Diagnostic> {
        let mut reworded = Vec::new();
        if self.source == Source::User {
            for function in self.program().functions.values() {
                for clause in &function.contracts.requires {
                    let raw = format!(
                        "precondition violated: `{}` requires `{}`",
                        function.name, clause.text
                    );
                    let shown = self.worded(panics::PRECONDITION_VIOLATED, &raw, span);
                    if shown != raw {
                        reworded.push((raw, shown));
                    }
                }
            }
        }
        if reworded.is_empty() {
            return self.panic_text(
                panics::PRECONDITION_VIOLATED,
                "panic_message",
                &[status],
                span,
            );
        }
        let (before, after) = self.report(panics::PRECONDITION_VIOLATED, span);
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

    /// Release the function value `value` whose count was 1: free it
    /// through its drop function.
    pub(super) fn drop_function_value(&mut self, value: Temp) {
        let drop = self.load(value, layout::FN_DROP);
        let dst = self.temp();
        self.push(Inst::CallIndirect {
            dst,
            target: drop,
            args: vec![value],
        });
    }

    /// `list.map(f)`, with `list` and `f` evaluated, for `expr`: a new
    /// list of `f` applied to each item in order. The items are borrowed
    /// from the list for each call.
    pub(super) fn list_map(
        &mut self,
        list: Temp,
        function: Temp,
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        let length = self.load(list, layout::LIST_LEN);
        let mapped = self.call_runtime("list_new", vec![length], NOWHERE)?;
        let index = self.constant(0);
        let (head, exit) = (self.label(), self.label());
        self.push(Inst::Label(head));
        self.push(Inst::Branch {
            cond: Cond::Ge,
            lhs: index,
            rhs: length,
            target: exit,
        });
        let item = self.item(list, index);
        let value = self.invoke(function, &[item], span)?;
        let base = self.item_base(mapped, index);
        self.store(base, layout::LIST_ITEMS, value);
        let one = self.constant(1);
        self.push(Inst::Binary {
            op: ir::BinOp::Add,
            dst: index,
            lhs: index,
            rhs: one,
        });
        self.push(Inst::Jump(head));
        self.push(Inst::Label(exit));
        self.store(mapped, layout::LIST_LEN, length);
        Ok(mapped)
    }

    // ---- The code of function values ----------------------------------

    /// The body of [`Instance::Closure`] `index`: its parameters are the
    /// object, then the closure's. Gives how many parameters it takes and
    /// the span of its body.
    pub(super) fn closure_function(&mut self, index: usize) -> Result<(u32, Span), Diagnostic> {
        let spec = self.lowering.closures[index].clone();
        let code = &spec.code;
        self.subst = spec.subst.clone();
        self.status = true;
        let object = self.temp();
        let params: Vec<Temp> = code.params.iter().map(|_| self.temp()).collect();
        let count = self.temps;
        // The slots below the parameters that it does not capture are
        // never read.
        let unread = self.constant(0);
        let mut slots = vec![
            Slot {
                temp: unread,
                release: false,
                ty: Type::Unit,
            };
            code.height
        ];
        for (index, ty) in spec.captures.iter().enumerate() {
            let value = self.load(object, layout::FN_CAPTURES + 8 * index as u32);
            let slot = Slot {
                temp: value,
                // Borrowed from the object, which the caller holds.
                release: false,
                ty: ty.clone(),
            };
            match code.captures.get(index) {
                Some(&number) => {
                    if let Some(place) = slots.get_mut(number) {
                        *place = slot;
                    }
                }
                None => {
                    self.receiver = Some(Receiver {
                        temp: value,
                        owned: false,
                        ty: ty.clone(),
                    })
                }
            }
        }
        for (param, &temp) in code.params.iter().zip(&params) {
            if param.binds {
                let ty = self.ty(&param.ty);
                slots.push(Slot {
                    temp,
                    release: false,
                    ty,
                });
            }
        }
        self.slots = slots;
        self.ret = self.ty(&code.body.ty);
        let value = self.expr(&code.body)?;
        self.return_value(value, body_end(&code.body))?;
        Ok((count, code.body.span))
    }

    /// The body of [`Instance::ClosureDrop`] `index`: release what the
    /// object, its parameter, captured, then free it.
    pub(super) fn closure_drop_function(&mut self, index: usize) -> Result<(), Diagnostic> {
        let spec = self.lowering.closures[index].clone();
        self.subst = spec.subst.clone();
        let object = self.temp();
        for (index, ty) in spec.captures.iter().enumerate() {
            if self.counted(ty) {
                let value = self.load(object, layout::FN_CAPTURES + 8 * index as u32);
                self.release(value, ty)?;
            }
        }
        let unit = self.call_runtime("free", vec![object], NOWHERE)?;
        self.push(Inst::Return(unit));
        Ok(())
    }

    /// The body of [`Instance::Thunk`]: the code of `name` (a function of
    /// the program, or an intrinsic) used as a value of the function type
    /// `ty`. Its parameters are the object, unused, then the call's
    /// arguments. `at` is the code that first used it as a value, where
    /// its refusals are reported. Gives how many parameters it takes.
    pub(super) fn thunk_function(
        &mut self,
        intrinsic: bool,
        name: &str,
        type_args: &[Type],
        ty: &Type,
        at: Span,
    ) -> Result<u32, Diagnostic> {
        let Type::Fn { params, ret } = ty else {
            return Err(self.refuse(at, &format!("a function value of type `{ty}`")));
        };
        self.status = true;
        let _object = self.temp();
        let args: Vec<Temp> = params.iter().map(|_| self.temp()).collect();
        let count = self.temps;
        if intrinsic {
            self.intrinsic_thunk(name, type_args, params, ret, &args, at)?;
        } else {
            self.function_thunk(name, type_args, &args, at)?;
        }
        Ok(count)
    }

    /// A thunk's call of the program's function `name` with `args`,
    /// borrowed: its `requires` first, whose broken clause is returned as
    /// the status for the caller to panic with.
    fn function_thunk(
        &mut self,
        name: &str,
        type_args: &[Type],
        args: &[Temp],
        at: Span,
    ) -> Result<(), Diagnostic> {
        let program = self.program();
        let Some(function) = program.functions.get(name) else {
            return Err(self.refuse(at, &format!("a call of `{name}`")));
        };
        if !function.contracts.requires.is_empty() {
            let instance = Instance::Requires {
                source: self.source,
                name: name.to_string(),
                type_args: type_args.to_vec(),
            };
            let origin = self.origin.unwrap_or(NOWHERE);
            let id = self.lowering.request(instance, origin);
            let failed = self.call_id(id, args.to_vec(), at)?;
            let fine = self.label();
            self.push(Inst::JumpIfZero {
                cond: failed,
                target: fine,
            });
            for (index, clause) in function.contracts.requires.iter().enumerate() {
                let next = self.label();
                let this = self.constant(index as i64 + 1);
                self.push(Inst::Branch {
                    cond: Cond::Ne,
                    lhs: failed,
                    rhs: this,
                    target: next,
                });
                let message = format!(
                    "precondition violated: `{}` requires `{}`",
                    function.name, clause.text
                );
                let text = self.text(&message);
                let nothing = self.constant(0);
                self.push(Inst::ReturnPair {
                    value: nothing,
                    second: text,
                });
                self.push(Inst::Label(next));
            }
            self.push(Inst::Label(fine));
        }
        let mutated = self.mutated_receiver(name, type_args);
        if let (Some(ty), Some(&receiver)) = (&mutated, args.first()) {
            // A mutating method owns its receiver, which is borrowed
            // here: it gets a reference of its own.
            if self.counted(ty) {
                self.retain(receiver);
            }
        }
        let result =
            self.call_function(Source::User, name, type_args.to_vec(), args.to_vec(), at)?;
        if let Some(ty) = mutated {
            let after = self.second_result();
            if self.counted(&ty) {
                self.release(after, &ty)?;
            }
        }
        self.leave_function(result)
    }

    /// A thunk's call of the intrinsic `name` with `args`, borrowed, of
    /// the types `params`, giving a `ret`: lowered as a call written with
    /// them would be. Uncounted, as the interpreter does not count it.
    fn intrinsic_thunk(
        &mut self,
        name: &str,
        type_args: &[Type],
        params: &[Type],
        ret: &Type,
        args: &[Temp],
        at: Span,
    ) -> Result<(), Diagnostic> {
        let mut locals = Vec::with_capacity(args.len());
        for (slot, (ty, &temp)) in params.iter().zip(args).enumerate() {
            self.slots.push(Slot {
                temp,
                release: false,
                ty: ty.clone(),
            });
            locals.push(Expr {
                kind: ExprKind::Local(slot),
                ty: ty.clone(),
                span: at,
            });
        }
        let call = Expr {
            kind: ExprKind::Call {
                callee: Callee::Intrinsic(name.to_string()),
                type_args: type_args.to_vec(),
                args: locals,
            },
            ty: ret.clone(),
            span: at,
        };
        // The caller counted the call: this one is not.
        self.push(Inst::Leave);
        let value = self.expr(&call)?;
        // Count it again for the caller to uncount. It was counted, so
        // it can be again.
        let never = self.panic_static(panics::CALL_DEPTH_EXCEEDED, panics::TOO_DEEP, NOWHERE)?;
        self.push(Inst::Enter { exhausted: never });
        self.leave_function(value)
    }
}

/// Whether a call of the prelude intrinsic `name` may panic: its report
/// names the call, which the code of a function value does not know.
fn may_panic(name: &str) -> bool {
    name == "List.at" || matches!(intrinsics::prelude(name), Some(Prelude::Checked(..)))
}
