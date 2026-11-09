//! Evaluating the checked program.
//!
//! The interpreter walks [`oska_check::program`] trees and never resolves
//! anything itself (ADR 0007). Each call gets a fresh [`Frame`]: its local
//! slots, pushed as bindings are made and popped when their block ends,
//! and a method's receiver. Anything that stops evaluation early, a
//! `return`, a `break` or `continue`, or a panic, is an [`Unwind`], passed
//! up with `?` until the construct that handles it.
//!
//! To evaluate a new node kind, add its arm to [`Interpreter::eval`] (or
//! [`Interpreter::stmt`]), and keep the frame's slots in step with the
//! checker's locals: push exactly where the checker binds.

use oska_check::program::{Callee, Expr, ExprKind, Function, Part, Place, Program, Stmt};
use oska_syntax::ast::BinaryOp;
use oska_syntax::Span;

use crate::host::Host;
use crate::intrinsics;
use crate::ops;
use crate::panics::Panic;
use crate::value::Value;

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
}

impl<'p, 'h> Interpreter<'p, 'h> {
    pub fn new(program: &'p Program, host: &'h mut dyn Host) -> Self {
        Interpreter { program, host }
    }

    /// Call `function` with `args`: the receiver first, for a method, then
    /// one per parameter. Only a panic unwinds out of a call.
    pub fn call(&mut self, function: &Function, args: Vec<Value>) -> Result<Value, Panic> {
        let mut args = args.into_iter();
        let receiver = if function.receiver { args.next() } else { None };
        let slots = function
            .params
            .iter()
            .zip(args)
            .filter(|(param, _)| param.binds)
            .map(|(_, arg)| arg)
            .collect();
        let mut frame = Frame { slots, receiver };
        match self.eval(&function.body, &mut frame) {
            Ok(value) | Err(Unwind::Return(value)) => Ok(value),
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
            ExprKind::Call { callee, args } => {
                let mut values = Vec::with_capacity(args.len());
                for arg in args {
                    values.push(self.eval(arg, frame)?);
                }
                self.call_callee(callee, values, expr.span)
            }
            ExprKind::Block { stmts, value } => {
                let height = frame.slots.len();
                let result = self.block(stmts, value.as_deref(), frame);
                frame.slots.truncate(height);
                result
            }
            ExprKind::Unsupported => {
                unreachable!("`crate::run` refuses a program with an unsupported node")
            }
        }
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
                match place {
                    // The old value is dropped: a copy elsewhere keeps its
                    // own reference to it.
                    Place::Local(slot) => frame.slots[*slot] = value,
                }
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
                let (start, end) = match self.eval(iter, frame)? {
                    Value::Range { start, end } => (start, end),
                    other => unreachable!("the checker allows `for` over a range, found {other:?}"),
                };
                let height = frame.slots.len();
                for n in start..end {
                    if *binds {
                        frame.slots.push(Value::Int(n));
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

    fn call_callee(
        &mut self,
        callee: &Callee,
        args: Vec<Value>,
        span: Span,
    ) -> Result<Value, Unwind> {
        match callee {
            Callee::Function(name) => {
                let program = self.program;
                let function = program
                    .functions
                    .get(name)
                    .expect("`crate::run` refuses a call to a function the program lacks");
                Ok(self.call(function, args)?)
            }
            Callee::Intrinsic(name) => {
                let intrinsic = intrinsics::lookup(name)
                    .expect("`crate::run` refuses a call to a missing intrinsic");
                Ok(intrinsic(self.host, args, span)?)
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
