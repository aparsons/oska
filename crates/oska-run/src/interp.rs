//! Evaluating the checked program.
//!
//! The interpreter walks [`oska_check::program`] trees and never resolves
//! anything itself (ADR 0007). Each call gets a fresh frame of local
//! slots. Anything that stops evaluation early is an [`Unwind`].

use oska_check::program::{Callee, Expr, ExprKind, Function, Program, Stmt};
use oska_syntax::Span;

use crate::host::Host;
use crate::intrinsics;
use crate::panics::Panic;
use crate::value::Value;

/// Why evaluation stopped before producing a value.
#[derive(Debug)]
pub enum Unwind {
    /// A node the interpreter cannot run yet.
    Unsupported(Span),
    /// A bug: unwinds all the way to [`crate::run`].
    Panic(Panic),
}

impl From<Panic> for Unwind {
    fn from(panic: Panic) -> Self {
        Unwind::Panic(panic)
    }
}

pub struct Interpreter<'p, 'h> {
    program: &'p Program,
    host: &'h mut dyn Host,
}

impl<'p, 'h> Interpreter<'p, 'h> {
    pub fn new(program: &'p Program, host: &'h mut dyn Host) -> Self {
        Interpreter { program, host }
    }

    /// Call `function` with `args`, one per parameter.
    pub fn call(&mut self, function: &Function, args: Vec<Value>) -> Result<Value, Unwind> {
        let mut frame: Vec<Value> = function
            .params
            .iter()
            .zip(args)
            .filter(|(param, _)| param.binds)
            .map(|(_, arg)| arg)
            .collect();
        self.eval(&function.body, &mut frame)
    }

    fn eval(&mut self, expr: &Expr, frame: &mut Vec<Value>) -> Result<Value, Unwind> {
        match &expr.kind {
            ExprKind::Unit => Ok(Value::Unit),
            ExprKind::Str(text) => Ok(Value::str(text)),
            ExprKind::Local(slot) => Ok(frame[*slot].clone()),
            ExprKind::Call { callee, args } => {
                let mut values = Vec::with_capacity(args.len());
                for arg in args {
                    values.push(self.eval(arg, frame)?);
                }
                match callee {
                    Callee::Function(name) => {
                        let program = self.program;
                        match program.functions.get(name) {
                            Some(function) => self.call(function, values),
                            None => Err(Unwind::Unsupported(expr.span)),
                        }
                    }
                    Callee::Intrinsic(name) => match intrinsics::lookup(name) {
                        Some(intrinsic) => intrinsic(self.host, values, expr.span),
                        None => Err(Unwind::Unsupported(expr.span)),
                    },
                }
            }
            ExprKind::Block { stmts, value } => {
                let height = frame.len();
                for stmt in stmts {
                    match stmt {
                        Stmt::Expr(e) => {
                            self.eval(e, frame)?;
                        }
                        Stmt::Let { binds, value } => {
                            let value = self.eval(value, frame)?;
                            if *binds {
                                frame.push(value);
                            }
                        }
                    }
                }
                let result = match value {
                    Some(value) => self.eval(value, frame),
                    None => Ok(Value::Unit),
                };
                frame.truncate(height);
                result
            }
            ExprKind::Unsupported => Err(Unwind::Unsupported(expr.span)),
        }
    }
}
