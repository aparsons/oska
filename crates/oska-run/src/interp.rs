//! Evaluating the checked program.
//!
//! The interpreter walks [`oska_check::program`] trees and never resolves
//! anything itself (ADR 0007). Each call gets a fresh [`Frame`]: its local
//! slots, pushed as bindings are made and popped when their block ends,
//! and a method's receiver. Anything that stops evaluation early, a
//! `return`, a `break` or `continue`, or a panic, is an [`Unwind`], passed
//! up with `?` until the construct that handles it.
//!
//! A closure captures by value: it keeps a copy of the frame's slots and
//! receiver where it was made (a reference-count increment each), and a
//! call of it pushes its arguments after them. A mutating call takes its
//! receiver out of its place, so a value nothing else shares is changed
//! in place, and writes the method's final `self` back.
//!
//! To evaluate a new node kind, add its arm to [`Interpreter::eval`] (or
//! [`Interpreter::stmt`]), and keep the frame's slots in step with the
//! checker's locals: push exactly where the checker binds.

use std::rc::Rc;

use oska_check::program::{
    self, Arm, Callee, Expr, ExprKind, Function, Part, Pattern, Place, Program, Stmt,
};
use oska_syntax::ast::BinaryOp;
use oska_syntax::Span;

use crate::host::Host;
use crate::intrinsics;
use crate::ops;
use crate::panics::Panic;
use crate::tasks::Tasks;
use crate::value::{ClosureValue, Value};

/// Why evaluation stopped before producing a value.
#[derive(Debug)]
pub enum Unwind {
    /// `return`, with its value: handled by the call.
    Return(Value),
    /// `break`: handled by the innermost loop.
    Break,
    /// `continue`: handled by the innermost loop.
    Continue,
    /// A bug: unwinds all the way to [`crate::run`].
    Panic(Panic),
}

impl From<Panic> for Unwind {
    fn from(panic: Panic) -> Self {
        Unwind::Panic(panic)
    }
}

/// The state of one call.
struct Frame {
    /// The local slots, in the order the checker numbered them.
    slots: Vec<Value>,
    /// A method's receiver.
    receiver: Option<Value>,
}

pub struct Interpreter<'p, 'h> {
    program: &'p Program,
    host: &'h mut dyn Host,
    /// The scopes and tasks of the run (`tasks.rs`).
    pub(crate) tasks: Tasks,
}

impl<'p, 'h> Interpreter<'p, 'h> {
    pub fn new(program: &'p Program, host: &'h mut dyn Host) -> Self {
        Interpreter {
            program,
            host,
            tasks: Tasks::default(),
        }
    }

    /// Call `function` with `args`: the receiver first, for a method, then
    /// one per parameter. Only a panic unwinds out of a call.
    pub fn call(&mut self, function: &Function, args: Vec<Value>) -> Result<Value, Panic> {
        let mut args = args.into_iter();
        let receiver = if function.receiver { args.next() } else { None };
        let (value, _) = self.call_with(function, receiver, args.collect())?;
        Ok(value)
    }

    /// Call `function` with `receiver`, for a method, and `args`, one per
    /// parameter. Returns its result and its receiver as it was when the
    /// call ended: a mutating method may have changed it.
    fn call_with(
        &mut self,
        function: &Function,
        receiver: Option<Value>,
        args: Vec<Value>,
    ) -> Result<(Value, Option<Value>), Panic> {
        let slots = function
            .params
            .iter()
            .zip(args)
            .filter(|(param, _)| param.binds)
            .map(|(_, arg)| arg)
            .collect();
        let mut frame = Frame { slots, receiver };
        match self.eval(&function.body, &mut frame) {
            Ok(value) | Err(Unwind::Return(value)) => Ok((value, frame.receiver)),
            Err(Unwind::Panic(panic)) => Err(panic),
            Err(Unwind::Break | Unwind::Continue) => {
                unreachable!("the checker keeps `break` and `continue` inside loops")
            }
        }
    }

    fn eval(&mut self, expr: &Expr, frame: &mut Frame) -> Result<Value, Unwind> {
        match &expr.kind {
            ExprKind::Unit => Ok(Value::Unit),
            ExprKind::Str(text) => Ok(Value::str(text)),
            ExprKind::Interpolated(parts) => self.interpolated(parts, frame),
            ExprKind::Int(n) => Ok(Value::Int(*n)),
            ExprKind::Bool(b) => Ok(Value::Bool(*b)),
            ExprKind::Local(slot) => Ok(frame.slots[*slot].clone()),
            ExprKind::SelfValue => Ok(frame
                .receiver
                .clone()
                .expect("the checker allows `self` only in a method")),
            ExprKind::Binary { op, lhs, rhs, int } => {
                let lhs = self.eval(lhs, frame)?;
                // `&&`, `||`, and `implies` stop once `lhs` decides.
                match (op, &lhs) {
                    (BinaryOp::And, Value::Bool(false)) => return Ok(Value::Bool(false)),
                    (BinaryOp::Or, Value::Bool(true)) => return Ok(Value::Bool(true)),
                    (BinaryOp::Implies, Value::Bool(false)) => return Ok(Value::Bool(true)),
                    _ => {}
                }
                let rhs = self.eval(rhs, frame)?;
                Ok(ops::binary(*op, lhs, rhs, int.as_ref(), expr.span)?)
            }
            ExprKind::Neg { operand, int } => {
                let operand = self.eval(operand, frame)?;
                Ok(ops::neg(operand, int, expr.span)?)
            }
            ExprKind::Not(operand) => Ok(Value::Bool(!self.eval(operand, frame)?.as_bool())),
            ExprKind::Range { start, end } => {
                let start = self.eval(start, frame)?.as_int();
                let end = self.eval(end, frame)?.as_int();
                Ok(Value::Range { start, end })
            }
            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                if self.eval(cond, frame)?.as_bool() {
                    self.eval(then_branch, frame)
                } else {
                    match else_branch {
                        Some(else_branch) => self.eval(else_branch, frame),
                        None => Ok(Value::Unit),
                    }
                }
            }
            ExprKind::List(items) => Ok(Value::List(Rc::new(self.eval_all(items, frame)?))),
            ExprKind::Construct { ctor, fields } => {
                let fields = self.eval_all(fields, frame)?;
                Ok(Value::construct(ctor, fields))
            }
            ExprKind::Field { base, index } => Ok(self.eval(base, frame)?.field(*index)),
            ExprKind::Closure(code) => Ok(Value::Closure(Rc::new(ClosureValue {
                code: Rc::clone(code),
                slots: frame.slots.clone(),
                receiver: frame.receiver.clone(),
            }))),
            ExprKind::CallValue { callee, args } => {
                let callee = self.eval(callee, frame)?;
                let args = self.eval_all(args, frame)?;
                Ok(self.call_value(&callee, args, expr.span)?)
            }
            ExprKind::Call { callee, args } => {
                let values = self.eval_all(args, frame)?;
                Ok(self.call_callee(callee, values, expr.span)?)
            }
            ExprKind::MutatingCall {
                callee,
                receiver,
                args,
            } => {
                let args = self.eval_all(args, frame)?;
                // Taking the receiver out of its place leaves the method
                // holding it, so a value nothing else shares has one
                // reference and is changed in place.
                let mut taken = std::mem::replace(place_mut(receiver, frame), Value::Unit);
                let value = self.call_mutating(callee, &mut taken, args, expr.span);
                *place_mut(receiver, frame) = taken;
                Ok(value?)
            }
            ExprKind::Block { stmts, value } => {
                let height = frame.slots.len();
                let result = self.block(stmts, value.as_deref(), frame);
                frame.slots.truncate(height);
                result
            }
            ExprKind::Match { scrutinee, arms } => {
                let scrutinee = self.eval(scrutinee, frame)?;
                self.match_arms(&scrutinee, arms, frame)
            }
            ExprKind::Try { operand, convert } => {
                let Value::Variant {
                    ty,
                    variant,
                    fields,
                } = self.eval(operand, frame)?
                else {
                    unreachable!("the checker allows `?` only on a `Result` or an `Option`")
                };
                match &*variant {
                    "Ok" | "Some" => Ok(fields[0].clone()),
                    "None" => Err(Unwind::Return(Value::none())),
                    _ => {
                        let error = fields[0].clone();
                        let error = match convert {
                            Some(convert) => self.call_callee(convert, vec![error], expr.span)?,
                            None => error,
                        };
                        debug_assert_eq!(&*ty, "Result");
                        Err(Unwind::Return(Value::err(error)))
                    }
                }
            }
            ExprKind::FnValue(callee) => Ok(Value::Function(Rc::new(callee.clone()))),
            ExprKind::Scope(body) => {
                let scope = self.tasks.open();
                let height = frame.slots.len();
                frame.slots.push(Value::Scope(scope));
                let result = self.eval(body, frame);
                frame.slots.truncate(height);
                // The scope does not exit until its tasks have finished,
                // however the block ended: by its value, or a `return`.
                // A panic ends the program, so its tasks never run.
                let finished = match &result {
                    Err(Unwind::Panic(_)) => Ok(()),
                    _ => self.finish_scope(scope),
                };
                self.tasks.close(scope);
                finished?;
                result
            }
            ExprKind::Unsupported => {
                unreachable!("`crate::run` refuses a program with an unsupported node")
            }
        }
    }

    /// The value of the first arm that matches `scrutinee`. Each arm's
    /// bindings are popped once it is done with, matched or not.
    fn match_arms(
        &mut self,
        scrutinee: &Value,
        arms: &[Arm],
        frame: &mut Frame,
    ) -> Result<Value, Unwind> {
        let height = frame.slots.len();
        for arm in arms {
            let mut chosen = self.matches(&arm.pattern, scrutinee, frame)?;
            if chosen {
                if let Some(guard) = &arm.guard {
                    chosen = self.eval(guard, frame)?.as_bool();
                }
            }
            let result = if chosen {
                Some(self.eval(&arm.body, frame))
            } else {
                None
            };
            frame.slots.truncate(height);
            if let Some(result) = result {
                return result;
            }
        }
        unreachable!("the checker guarantees an exhaustive `match`")
    }

    /// Whether `value` matches `pattern`, pushing its bindings as it
    /// goes. On a mismatch, the caller pops them.
    fn matches(
        &mut self,
        pattern: &Pattern,
        value: &Value,
        frame: &mut Frame,
    ) -> Result<bool, Unwind> {
        Ok(match pattern {
            Pattern::Wildcard => true,
            Pattern::Bind => {
                frame.slots.push(value.clone());
                true
            }
            Pattern::Literal(literal) => self.eval(literal, frame)? == *value,
            Pattern::Construct { ctor, fields } => {
                let (variant, values) = match value {
                    Value::Struct { fields, .. } => (None, fields),
                    Value::Variant {
                        variant, fields, ..
                    } => (Some(&**variant), fields),
                    other => {
                        unreachable!("the checker guarantees a struct or enum, found {other:?}")
                    }
                };
                if variant != ctor.variant.as_deref() {
                    return Ok(false);
                }
                for (field, value) in fields.iter().zip(values.iter()) {
                    if !self.matches(field, value, frame)? {
                        return Ok(false);
                    }
                }
                true
            }
        })
    }

    /// Each of `exprs`, in order.
    fn eval_all(&mut self, exprs: &[Expr], frame: &mut Frame) -> Result<Vec<Value>, Unwind> {
        let mut values = Vec::with_capacity(exprs.len());
        for expr in exprs {
            values.push(self.eval(expr, frame)?);
        }
        Ok(values)
    }

    /// The statements of a block, then its value. The caller pops the
    /// block's slots.
    fn block(
        &mut self,
        stmts: &[Stmt],
        value: Option<&Expr>,
        frame: &mut Frame,
    ) -> Result<Value, Unwind> {
        for stmt in stmts {
            self.stmt(stmt, frame)?;
        }
        match value {
            Some(value) => self.eval(value, frame),
            None => Ok(Value::Unit),
        }
    }

    fn stmt(&mut self, stmt: &Stmt, frame: &mut Frame) -> Result<(), Unwind> {
        match stmt {
            Stmt::Expr(e) => {
                self.eval(e, frame)?;
            }
            Stmt::Let { binds, value } => {
                let value = self.eval(value, frame)?;
                if *binds {
                    frame.slots.push(value);
                }
            }
            Stmt::Assign { place, value } => {
                let value = self.eval(value, frame)?;
                // The old value is dropped: a copy elsewhere keeps its own
                // reference to it.
                *place_mut(place, frame) = value;
            }
            Stmt::Return(value) => {
                let value = match value {
                    Some(value) => self.eval(value, frame)?,
                    None => Value::Unit,
                };
                return Err(Unwind::Return(value));
            }
            Stmt::Break => return Err(Unwind::Break),
            Stmt::Continue => return Err(Unwind::Continue),
            Stmt::While { cond, body } => {
                while self.eval(cond, frame)?.as_bool() {
                    if !self.loop_body(body, frame)? {
                        break;
                    }
                }
            }
            Stmt::For { binds, iter, body } => {
                // Iterating a list holds a reference to it, so the body
                // writing the `var` it came from changes a copy.
                let items: Box<dyn Iterator<Item = Value>> = match self.eval(iter, frame)? {
                    Value::Range { start, end } => Box::new((start..end).map(Value::Int)),
                    Value::List(items) => Box::new(ListIter { items, next: 0 }),
                    other => unreachable!(
                        "the checker allows `for` over a range or a list, found {other:?}"
                    ),
                };
                let height = frame.slots.len();
                for item in items {
                    if *binds {
                        frame.slots.push(item);
                    }
                    let go_on = self.loop_body(body, frame);
                    frame.slots.truncate(height);
                    if !go_on? {
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    /// Run one iteration of a loop's body. Returns whether the loop goes
    /// on: `false` after a `break`.
    fn loop_body(&mut self, body: &Expr, frame: &mut Frame) -> Result<bool, Unwind> {
        match self.eval(body, frame) {
            Ok(_) | Err(Unwind::Continue) => Ok(true),
            Err(Unwind::Break) => Ok(false),
            Err(other) => Err(other),
        }
    }

    /// Call `callee` with `args`, the receiver first for a method. Only a
    /// panic unwinds out of a call.
    pub fn call_callee(
        &mut self,
        callee: &Callee,
        args: Vec<Value>,
        span: Span,
    ) -> Result<Value, Panic> {
        match callee {
            Callee::Function(name) => {
                let program = self.program;
                let function = program
                    .functions
                    .get(name)
                    .expect("`crate::run` refuses a call to a function the program lacks");
                self.call(function, args)
            }
            Callee::Intrinsic(name) => {
                if let Some(intrinsic) = intrinsics::lookup(name) {
                    return intrinsic(self.host, args, span);
                }
                if let Some(intrinsic) = intrinsics::lookup_higher_order(name) {
                    return intrinsic(self, args, span);
                }
                // A mutating method called on a temporary: the change is
                // dropped with it.
                let mut args = args.into_iter();
                let mut receiver = args.next().expect("a method's receiver");
                self.call_mutating(callee, &mut receiver, args.collect(), span)
            }
            Callee::Trait { .. } => {
                let callee = self.dispatch(callee, &args[0]);
                self.call_callee(&callee, args, span)
            }
        }
    }

    /// The implementation `callee` reaches for `receiver`: a trait's
    /// method found by the receiver's runtime type tag (ADR 0007), the
    /// function `<Tag as Trait>.method` when the program has one and the
    /// prelude's intrinsic `Tag.method` otherwise. Any other callee is
    /// already resolved.
    fn dispatch(&self, callee: &Callee, receiver: &Value) -> Callee {
        let Callee::Trait { trait_name, method } = callee else {
            return callee.clone();
        };
        let tag = receiver.type_tag();
        let name = program::trait_method(tag, trait_name, method);
        if self.program.functions.contains_key(&name) {
            Callee::Function(name)
        } else {
            Callee::Intrinsic(format!("{tag}.{method}"))
        }
    }

    /// Call `trait_name`'s method `method` on `receiver`, through the
    /// implementation for its runtime type tag, with `args` after it.
    pub fn call_trait(
        &mut self,
        trait_name: &str,
        method: &str,
        receiver: Value,
        args: Vec<Value>,
        span: Span,
    ) -> Result<Value, Panic> {
        let callee = Callee::Trait {
            trait_name: trait_name.to_string(),
            method: method.to_string(),
        };
        let args = std::iter::once(receiver).chain(args).collect();
        self.call_callee(&callee, args, span)
    }

    /// Call the function value `callee`, a closure or a named function,
    /// with `args`. `span` is the call's.
    pub fn call_value(
        &mut self,
        callee: &Value,
        args: Vec<Value>,
        span: Span,
    ) -> Result<Value, Panic> {
        let closure = match callee {
            Value::Closure(closure) => closure,
            Value::Function(function) => return self.call_callee(function, args, span),
            other => unreachable!("the checker calls only functions, found {other:?}"),
        };
        let mut slots = closure.slots.clone();
        slots.extend(
            closure
                .code
                .params
                .iter()
                .zip(args)
                .filter(|(binds, _)| **binds)
                .map(|(_, arg)| arg),
        );
        let mut frame = Frame {
            slots,
            receiver: closure.receiver.clone(),
        };
        match self.eval(&closure.code.body, &mut frame) {
            Ok(value) | Err(Unwind::Return(value)) => Ok(value),
            Err(Unwind::Panic(panic)) => Err(panic),
            Err(Unwind::Break | Unwind::Continue) => {
                unreachable!("the checker keeps `break` and `continue` inside loops")
            }
        }
    }

    /// Call the mutating method `callee` on `receiver`, which it changes.
    fn call_mutating(
        &mut self,
        callee: &Callee,
        receiver: &mut Value,
        args: Vec<Value>,
        span: Span,
    ) -> Result<Value, Panic> {
        match callee {
            Callee::Function(name) => {
                let program = self.program;
                let function = program
                    .functions
                    .get(name)
                    .expect("`crate::run` refuses a call to a function the program lacks");
                let taken = std::mem::replace(receiver, Value::Unit);
                let (value, after) = self.call_with(function, Some(taken), args)?;
                *receiver = after.expect("a method keeps its receiver");
                Ok(value)
            }
            Callee::Intrinsic(name) => {
                let intrinsic = intrinsics::lookup_mutating(name)
                    .expect("`crate::run` refuses a call to a missing intrinsic");
                intrinsic(receiver, args, span)
            }
            Callee::Trait { .. } => {
                let callee = self.dispatch(callee, receiver);
                self.call_mutating(&callee, receiver, args, span)
            }
        }
    }

    /// An interpolated string: each hole displayed through the
    /// implementation the checker chose.
    fn interpolated(&mut self, parts: &[Part], frame: &mut Frame) -> Result<Value, Unwind> {
        let mut text = String::new();
        for part in parts {
            match part {
                Part::Text(t) => text.push_str(t),
                Part::Hole { value, display } => {
                    let span = value.span;
                    let value = self.eval(value, frame)?;
                    let shown = self.call_callee(display, vec![value], span)?;
                    text.push_str(shown.as_str());
                }
            }
        }
        Ok(Value::str(&text))
    }
}

/// The value at `place`, to write. Each struct on the way is made
/// unshared first ([`Rc::make_mut`]): one that is shared is copied, so a
/// copy elsewhere keeps the old value.
fn place_mut<'f>(place: &Place, frame: &'f mut Frame) -> &'f mut Value {
    match place {
        Place::Local(slot) => &mut frame.slots[*slot],
        Place::SelfValue => frame
            .receiver
            .as_mut()
            .expect("the checker allows `self` only in a method"),
        Place::Field { base, index } => match place_mut(base, frame) {
            Value::Struct { fields, .. } => &mut Rc::make_mut(fields)[*index],
            other => unreachable!("the checker guarantees a struct, found {other:?}"),
        },
    }
}

/// The items of a list, one at a time.
struct ListIter {
    items: Rc<Vec<Value>>,
    next: usize,
}

impl Iterator for ListIter {
    type Item = Value;

    fn next(&mut self) -> Option<Value> {
        let item = self.items.get(self.next)?.clone();
        self.next += 1;
        Some(item)
    }
}
