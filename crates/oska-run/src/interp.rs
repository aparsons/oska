//! Evaluating the checked program.
//!
//! The interpreter walks [`oska_check::program`] trees and never resolves
//! anything itself (ADR 0007). Each call gets a fresh [`Frame`]: its local
//! slots, pushed as bindings are made and popped when their block ends,
//! and a method's receiver. Anything that stops evaluation early, a
//! `return`, a `break` or `continue`, or a panic, is an [`Unwind`], passed
//! up with `?` until the construct that handles it.
//!
//! A closure captures by value: it keeps a copy of the slots its body
//! reads, and of the receiver if it reads `self`, where it was made (a
//! reference-count increment each), as the checker listed them. A call
//! puts them back at their slot numbers and pushes its arguments after
//! them. A mutating call takes its
//! receiver out of its place, so a value nothing else shares is changed
//! in place, and writes the method's final `self` back.
//!
//! To evaluate a new node kind, add its arm to [`Interpreter::eval`] (or
//! [`Interpreter::stmt`]), and keep the frame's slots in step with the
//! checker's locals: push exactly where the checker binds.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use oska_check::builtins::{self, TraitMethod};
use oska_check::program::{
    self, Arm, Callee, Clause, Expr, ExprKind, Function, Part, Pattern, Place, Program, Stmt,
};
use oska_syntax::ast::BinaryOp;
use oska_syntax::Span;

use crate::host::Host;
use crate::intrinsics::{self, Intrinsic};
use crate::ops;
use crate::panics::{codes, Panic};
use crate::tasks::Tasks;
use crate::value::{ClosureValue, Value};

/// Why evaluation stopped before producing a value.
#[derive(Debug)]
pub enum Unwind {
    /// `return`, or the `?` that returns early, with its value and the
    /// span of the statement or expression: handled by the call.
    Return(Value, Span),
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

/// How deeply calls may nest before the program panics with `P0021`:
/// runaway recursion stops with a panic rather than overflowing the
/// interpreter's own stack.
pub const MAX_CALL_DEPTH: usize = 50_000;

/// How much of an interpreter thread's stack is kept free: room for the
/// work between two calls (an intrinsic, a deeply nested expression) and
/// for reporting the panic. A call that would leave less panics with
/// `P0021` too, so however much stack each call of a program takes, the
/// stack never overflows.
const STACK_RESERVE: usize = 16 * 1024 * 1024;

thread_local! {
    /// On an interpreter thread: the address its stack starts at, and how
    /// many bytes of it calls may use.
    static STACK: Cell<Option<(usize, usize)>> = const { Cell::new(None) };
}

/// The address of a local of the caller's frame: how far down the stack
/// this thread is. Only compared, never dereferenced.
#[inline(always)]
fn stack_position() -> usize {
    let here = 0u8;
    std::hint::black_box(&here) as *const u8 as usize
}

/// Mark the current position as the top of an interpreter thread's
/// stack, which is `size` bytes in all, so that calls stop with `P0021`
/// before they use the reserve at its bottom.
pub(crate) fn mark_stack_top(size: usize) {
    let budget = size.saturating_sub(STACK_RESERVE);
    STACK.with(|stack| stack.set(Some((stack_position(), budget))));
}

/// Whether the calls running on this thread have used up its stack
/// budget. Off an interpreter thread, only [`MAX_CALL_DEPTH`] applies.
fn stack_exhausted() -> bool {
    STACK.with(|stack| {
        stack
            .get()
            .is_some_and(|(top, budget)| top.abs_diff(stack_position()) > budget)
    })
}

pub struct Interpreter<'p, 'h> {
    program: &'p Program,
    host: &'h mut dyn Host,
    /// The scopes and tasks of the run (`tasks.rs`).
    pub(crate) tasks: Tasks,
    /// How many calls of functions and closures are running.
    depth: usize,
}

impl<'p, 'h> Interpreter<'p, 'h> {
    pub fn new(program: &'p Program, host: &'h mut dyn Host) -> Self {
        Interpreter {
            program,
            host,
            tasks: Tasks::default(),
            depth: 0,
        }
    }

    /// Enter a call of a function or closure, made at `span`: a panic
    /// with `P0021` when calls already nest [`MAX_CALL_DEPTH`] deep. Each
    /// successful entry is paired with [`Interpreter::leave`].
    fn enter(&mut self, span: Span) -> Result<(), Panic> {
        if self.depth >= MAX_CALL_DEPTH || stack_exhausted() {
            return Err(Panic::new(
                codes::CALL_DEPTH_EXCEEDED,
                "calls nest too deep: is this recursion missing its base case?",
                span,
            ));
        }
        self.depth += 1;
        Ok(())
    }

    fn leave(&mut self) {
        self.depth -= 1;
    }

    /// Call `function` with `args`: the receiver first, for a method, then
    /// one per parameter. `span` is the call's. Only a panic unwinds out
    /// of a call.
    pub fn call(
        &mut self,
        function: &Function,
        args: Vec<Value>,
        span: Span,
    ) -> Result<Value, Panic> {
        let mut args = args.into_iter();
        let receiver = if function.receiver { args.next() } else { None };
        let (value, _) = self.call_with(function, receiver, args.collect(), span)?;
        Ok(value)
    }

    /// Run a test's `body` with `t`, its `TestContext`, in slot 0. Only a
    /// panic, an assertion failure included, unwinds out of it.
    pub fn run_test(&mut self, body: &Expr, t: Value) -> Result<(), Panic> {
        let mut frame = Frame {
            slots: vec![t],
            receiver: None,
        };
        match self.eval(body, &mut frame) {
            Ok(_) | Err(Unwind::Return(..)) => Ok(()),
            Err(Unwind::Panic(panic)) => Err(panic),
            Err(Unwind::Break | Unwind::Continue) => {
                unreachable!("the checker keeps `break` and `continue` inside loops")
            }
        }
    }

    /// Call `function` with `receiver`, for a method, and `args`, one per
    /// parameter. Returns its result and its receiver as it was when the
    /// call ended: a mutating method may have changed it. `span` is the
    /// call's.
    ///
    /// Every call of a function passes through here, so this is where its
    /// contracts are evaluated: its `requires` clauses before the body.
    fn call_with(
        &mut self,
        function: &Function,
        receiver: Option<Value>,
        args: Vec<Value>,
        span: Span,
    ) -> Result<(Value, Option<Value>), Panic> {
        let entry = if function.contracts.is_empty() {
            None
        } else {
            Some(self.requires(function, &receiver, &args, span)?)
        };
        let mut frame = Frame {
            slots: binding(function.params.iter().map(|p| p.binds), args),
            receiver,
        };
        // Every way out of the body ends here, the value it returns with
        // the span of the return that carries it.
        let (value, returned_at) = match self.eval(&function.body, &mut frame) {
            Ok(value) => (value, body_end(&function.body)),
            Err(Unwind::Return(value, at)) => (value, at),
            Err(Unwind::Panic(panic)) => return Err(panic),
            Err(Unwind::Break | Unwind::Continue) => {
                unreachable!("the checker keeps `break` and `continue` inside loops")
            }
        };
        if let Some(slots) = entry {
            let receiver = frame.receiver.clone();
            self.ensures(function, Frame { slots, receiver }, &value, returned_at)?;
        }
        Ok((value, frame.receiver))
    }

    /// Evaluate `function`'s `requires` clauses in order, for a call
    /// made at `span` with `receiver` and `args`. The first that is false
    /// panics with `P0006`, spanning the call: the caller broke the
    /// contract. A panic while evaluating a clause, such as an overflow,
    /// is passed on as it is.
    ///
    /// Returns the arguments that take a slot in the clauses' frame, for
    /// the `ensures` clauses to start from.
    #[inline(never)]
    fn requires(
        &mut self,
        function: &Function,
        receiver: &Option<Value>,
        args: &[Value],
        span: Span,
    ) -> Result<Vec<Value>, Panic> {
        let contracts = &function.contracts;
        let mut frame = Frame {
            slots: binding(contracts.params.iter().copied(), args.to_vec()),
            receiver: receiver.clone(),
        };
        let height = frame.slots.len();
        for clause in &contracts.requires {
            if !self.holds(clause, &mut frame)? {
                return Err(Panic::new(
                    codes::PRECONDITION_VIOLATED,
                    format!(
                        "precondition violated: `{}` requires `{}`",
                        function.name, clause.text
                    ),
                    span,
                ));
            }
        }
        Ok(frame.slots[..height].to_vec())
    }

    /// Evaluate `function`'s `ensures` clauses in order, now that it has
    /// returned `value` at `returned_at`. `frame` is the clauses' own, as
    /// [`Interpreter::requires`] left it, with the receiver the body ended
    /// with: a mutating method's clause sees what the method made of it.
    /// The first clause that is false panics with `P0007`, spanning the
    /// clause: the function broke its promise. The message names the
    /// return. A panic while evaluating a clause is passed on as it is.
    #[inline(never)]
    fn ensures(
        &mut self,
        function: &Function,
        mut frame: Frame,
        value: &Value,
        returned_at: Span,
    ) -> Result<(), Panic> {
        let height = frame.slots.len();
        for ensures in &function.contracts.ensures {
            if ensures.binds {
                frame.slots.push(value.clone());
            }
            let holds = self.holds(&ensures.clause, &mut frame)?;
            frame.slots.truncate(height);
            if !holds {
                let (line, column) = self.program.line_col(returned_at.start);
                return Err(Panic::new(
                    codes::POSTCONDITION_VIOLATED,
                    format!(
                        "postcondition violated: `{}` ensures `{}`, \
                         broken by the return at line {line}, column {column}",
                        function.name, ensures.clause.text
                    ),
                    ensures.clause.span,
                ));
            }
        }
        Ok(())
    }

    /// Whether the condition of `clause` holds in `frame`, a frame of the
    /// clauses' own (see [`program::Contracts`]).
    fn holds(&mut self, clause: &Clause, frame: &mut Frame) -> Result<bool, Panic> {
        match self.eval(&clause.cond, frame) {
            Ok(value) => Ok(value.as_bool()),
            Err(Unwind::Panic(panic)) => Err(panic),
            Err(Unwind::Return(..) | Unwind::Break | Unwind::Continue) => {
                unreachable!(
                    "lowering refuses a `return` or `?` that would leave a contract, and the \
                     checker keeps `break` and `continue` inside loops"
                )
            }
        }
    }

    /// The value of `expr`.
    ///
    /// Every level of a program's recursion passes through here several
    /// times, so this frame stays small: each arm that needs locals of its
    /// own has a method of its own, kept out of line, and only the arm
    /// being run adds its frame to the stack.
    fn eval(&mut self, expr: &Expr, frame: &mut Frame) -> Result<Value, Unwind> {
        match &expr.kind {
            ExprKind::Unit => Ok(Value::Unit),
            ExprKind::Int(n, int) => Ok(Value::Int(*n, *int)),
            ExprKind::Bool(b) => Ok(Value::Bool(*b)),
            ExprKind::Local(slot) => Ok(frame.slots[*slot].clone()),
            ExprKind::Binary { .. } => self.binary(expr, frame),
            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => self.if_expr(cond, then_branch, else_branch.as_deref(), frame),
            ExprKind::Call { callee, args } => self.call_expr(callee, args, expr.span, frame),
            ExprKind::Block { stmts, value } => self.block(stmts, value.as_deref(), frame),
            _ => self.eval_other(expr, frame),
        }
    }

    /// The value of `expr`, of a kind [`Interpreter::eval`] does not
    /// handle itself.
    #[inline(never)]
    fn eval_other(&mut self, expr: &Expr, frame: &mut Frame) -> Result<Value, Unwind> {
        match &expr.kind {
            ExprKind::Str(text) => Ok(Value::str(text)),
            ExprKind::Interpolated(parts) => self.interpolated(parts, frame),
            ExprKind::SelfValue => Ok(frame
                .receiver
                .clone()
                .expect("the checker allows `self` only in a method")),
            ExprKind::Neg { operand, int } => {
                let operand = self.eval(operand, frame)?;
                Ok(ops::neg(operand, *int, expr.span)?)
            }
            ExprKind::Not(operand) => Ok(Value::Bool(!self.eval(operand, frame)?.as_bool())),
            ExprKind::Range { start, end } => {
                let Value::Int(start, int) = self.eval(start, frame)? else {
                    unreachable!("the checker allows `..` only between integers")
                };
                let end = self.eval(end, frame)?.as_int();
                Ok(Value::Range { start, end, int })
            }
            ExprKind::List(items) => Ok(Value::List(Rc::new(self.eval_all(items, frame)?))),
            ExprKind::Construct { ctor, fields } => {
                let fields = self.eval_all(fields, frame)?;
                Ok(Value::construct(ctor, fields))
            }
            ExprKind::Field { base, index } => Ok(self.eval(base, frame)?.field(*index)),
            ExprKind::Closure(code) => Ok(self.closure(code, frame)),
            ExprKind::CallValue { callee, args } => {
                self.call_value_expr(callee, args, expr.span, frame)
            }
            ExprKind::MutatingCall {
                callee,
                receiver,
                args,
            } => self.mutating_call_expr(callee, receiver, args, expr.span, frame),
            ExprKind::Match { scrutinee, arms } => {
                let scrutinee = self.eval(scrutinee, frame)?;
                self.match_arms(&scrutinee, arms, frame)
            }
            ExprKind::Try { operand, convert } => {
                self.try_expr(operand, convert.as_ref(), expr.span, frame)
            }
            ExprKind::FnValue(callee) => Ok(Value::Function(Rc::new(callee.clone()))),
            ExprKind::Scope(body) => self.scope(body, frame),
            ExprKind::Unit
            | ExprKind::Int(..)
            | ExprKind::Bool(_)
            | ExprKind::Local(_)
            | ExprKind::Binary { .. }
            | ExprKind::If { .. }
            | ExprKind::Call { .. }
            | ExprKind::Block { .. } => unreachable!("`eval` handles these itself"),
            ExprKind::Unsupported(_) => {
                unreachable!("`crate::run` refuses a program with an unsupported node")
            }
        }
    }

    /// `if`, with `else` or without.
    #[inline(never)]
    fn if_expr(
        &mut self,
        cond: &Expr,
        then_branch: &Expr,
        else_branch: Option<&Expr>,
        frame: &mut Frame,
    ) -> Result<Value, Unwind> {
        if self.eval(cond, frame)?.as_bool() {
            self.eval(then_branch, frame)
        } else {
            match else_branch {
                Some(else_branch) => self.eval(else_branch, frame),
                None => Ok(Value::Unit),
            }
        }
    }

    /// A binary operator: `&&`, `||`, and `implies` evaluate their right
    /// operand only when the left does not decide.
    #[inline(never)]
    fn binary(&mut self, expr: &Expr, frame: &mut Frame) -> Result<Value, Unwind> {
        let ExprKind::Binary { op, lhs, rhs, int } = &expr.kind else {
            unreachable!("`eval` passes a binary operator");
        };
        let lhs = self.eval(lhs, frame)?;
        match (op, &lhs) {
            (BinaryOp::And, Value::Bool(false)) => return Ok(Value::Bool(false)),
            (BinaryOp::Or, Value::Bool(true)) => return Ok(Value::Bool(true)),
            (BinaryOp::Implies, Value::Bool(false)) => return Ok(Value::Bool(true)),
            _ => {}
        }
        let rhs = self.eval(rhs, frame)?;
        Ok(ops::binary(*op, lhs, rhs, *int, expr.span)?)
    }

    /// A closure value: `code`, with what it captures from `frame`: only
    /// the slots and receiver its body reads.
    #[inline(never)]
    fn closure(&mut self, code: &Arc<program::Closure>, frame: &Frame) -> Value {
        debug_assert_eq!(
            frame.slots.len(),
            code.height,
            "the frame's slots match the checker's locals"
        );
        Value::Closure(Rc::new(ClosureValue {
            code: Arc::clone(code),
            captured: code
                .captures
                .iter()
                .map(|&slot| frame.slots[slot].clone())
                .collect(),
            receiver: frame.receiver.clone().filter(|_| code.captures_self),
        }))
    }

    /// A call of a function value, made at `span`.
    #[inline(never)]
    fn call_value_expr(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        span: Span,
        frame: &mut Frame,
    ) -> Result<Value, Unwind> {
        let callee = self.eval(callee, frame)?;
        let args = self.eval_all(args, frame)?;
        Ok(self.call_value(&callee, args, span)?)
    }

    /// A call of `callee`, made at `span`.
    #[inline(never)]
    fn call_expr(
        &mut self,
        callee: &Callee,
        args: &[Expr],
        span: Span,
        frame: &mut Frame,
    ) -> Result<Value, Unwind> {
        let values = self.eval_all(args, frame)?;
        Ok(self.call_callee(callee, values, span)?)
    }

    /// A call of the mutating method `callee` on `receiver`, made at
    /// `span`. Taking the receiver out of its place leaves the method
    /// holding it, so a value nothing else shares has one reference and
    /// is changed in place.
    #[inline(never)]
    fn mutating_call_expr(
        &mut self,
        callee: &Callee,
        receiver: &Place,
        args: &[Expr],
        span: Span,
        frame: &mut Frame,
    ) -> Result<Value, Unwind> {
        let args = self.eval_all(args, frame)?;
        let mut taken = std::mem::replace(place_mut(receiver, frame), Value::Unit);
        let value = self.call_mutating(callee, &mut taken, args, span);
        *place_mut(receiver, frame) = taken;
        Ok(value?)
    }

    /// `operand?`, at `span`.
    #[inline(never)]
    fn try_expr(
        &mut self,
        operand: &Expr,
        convert: Option<&Callee>,
        span: Span,
        frame: &mut Frame,
    ) -> Result<Value, Unwind> {
        let Value::Variant {
            ty,
            variant,
            fields,
        } = self.eval(operand, frame)?
        else {
            unreachable!("the checker allows `?` only on a `Result` or an `Option`")
        };
        match &*variant {
            builtins::OK | builtins::SOME => Ok(fields[0].clone()),
            builtins::NONE => Err(Unwind::Return(Value::none(), span)),
            _ => {
                let error = fields[0].clone();
                let error = match convert {
                    Some(convert) => self.call_callee(convert, vec![error], span)?,
                    None => error,
                };
                debug_assert_eq!(&*ty, builtins::RESULT);
                Err(Unwind::Return(Value::err(error), span))
            }
        }
    }

    /// `scope |s| body`. The scope does not exit until its tasks have
    /// finished, however the block ended: by its value, or a `return`. A
    /// panic ends the program, so its tasks never run.
    #[inline(never)]
    fn scope(&mut self, body: &Expr, frame: &mut Frame) -> Result<Value, Unwind> {
        let scope = self.tasks.open();
        let height = frame.slots.len();
        frame.slots.push(Value::Scope(scope));
        let result = self.eval(body, frame);
        frame.slots.truncate(height);
        let finished = match &result {
            Err(Unwind::Panic(_)) => Ok(()),
            _ => self.finish_scope(scope),
        };
        self.tasks.close(scope);
        finished?;
        result
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

    /// A block: its statements, then its value. Its slots are popped
    /// when it ends.
    #[inline(never)]
    fn block(
        &mut self,
        stmts: &[Stmt],
        value: Option<&Expr>,
        frame: &mut Frame,
    ) -> Result<Value, Unwind> {
        let height = frame.slots.len();
        let result = self.block_body(stmts, value, frame);
        frame.slots.truncate(height);
        result
    }

    fn block_body(
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
            Stmt::Return { value, span } => {
                let value = match value {
                    Some(value) => self.eval(value, frame)?,
                    None => Value::Unit,
                };
                return Err(Unwind::Return(value, *span));
            }
            Stmt::Break => return Err(Unwind::Break),
            Stmt::Continue => return Err(Unwind::Continue),
            Stmt::While { cond, body } => self.while_loop(cond, body, frame)?,
            Stmt::For { binds, iter, body } => self.for_loop(*binds, iter, body, frame)?,
        }
        Ok(())
    }

    #[inline(never)]
    fn while_loop(&mut self, cond: &Expr, body: &Expr, frame: &mut Frame) -> Result<(), Unwind> {
        while self.eval(cond, frame)?.as_bool() {
            if !self.loop_body(body, frame)? {
                break;
            }
        }
        Ok(())
    }

    /// `for` over a range or a list. Iterating a list holds a reference
    /// to it, so the body writing the `var` it came from changes a copy.
    #[inline(never)]
    fn for_loop(
        &mut self,
        binds: bool,
        iter: &Expr,
        body: &Expr,
        frame: &mut Frame,
    ) -> Result<(), Unwind> {
        let items: Box<dyn Iterator<Item = Value>> = match self.eval(iter, frame)? {
            Value::Range { start, end, int } => {
                Box::new((start..end).map(move |n| Value::Int(n, int)))
            }
            Value::List(items) => Box::new(ListIter { items, next: 0 }),
            other => {
                unreachable!("the checker allows `for` over a range or a list, found {other:?}")
            }
        };
        let height = frame.slots.len();
        for item in items {
            if binds {
                frame.slots.push(item);
            }
            let go_on = self.loop_body(body, frame);
            frame.slots.truncate(height);
            if !go_on? {
                break;
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

    /// The function the program defines as `name`.
    fn function(&self, name: &str) -> &'p Function {
        self.program
            .functions
            .get(name)
            .expect("`crate::run` refuses a call to a function the program lacks")
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
                let function = self.function(name);
                self.enter(span)?;
                let result = self.call(function, args, span);
                self.leave();
                result
            }
            Callee::Intrinsic(name) => match intrinsic(name) {
                Intrinsic::Plain(run) => run(self.host, args, span),
                Intrinsic::HigherOrder(run) => run(self, args, span),
                Intrinsic::Mutating(run) => {
                    // A mutating method called on a temporary, such as a
                    // call's result: the change is dropped with it.
                    let mut args = args.into_iter();
                    let mut receiver = args.next().expect("a method's receiver");
                    run(&mut receiver, args.collect(), span)
                }
            },
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

    /// Call the trait method `method` on `receiver`, through the
    /// implementation for its runtime type tag, with `args` after it.
    pub fn call_trait(
        &mut self,
        method: TraitMethod,
        receiver: Value,
        args: Vec<Value>,
        span: Span,
    ) -> Result<Value, Panic> {
        let callee = Callee::Trait {
            trait_name: method.trait_name.to_string(),
            method: method.method.to_string(),
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
        // The slots the body never reads stay `()`: the numbering is the
        // checker's, so the captured ones and the arguments land where
        // the body looks for them.
        let mut slots = vec![Value::Unit; closure.code.height];
        for (&slot, value) in closure.code.captures.iter().zip(&closure.captured) {
            slots[slot] = value.clone();
        }
        slots.extend(binding(closure.code.params.iter().copied(), args));
        let mut frame = Frame {
            slots,
            receiver: closure.receiver.clone(),
        };
        self.enter(span)?;
        let result = self.eval(&closure.code.body, &mut frame);
        self.leave();
        match result {
            Ok(value) | Err(Unwind::Return(value, _)) => Ok(value),
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
                let function = self.function(name);
                let taken = std::mem::replace(receiver, Value::Unit);
                self.enter(span)?;
                let result = self.call_with(function, Some(taken), args, span);
                self.leave();
                let (value, after) = result?;
                *receiver = after.expect("a method keeps its receiver");
                Ok(value)
            }
            Callee::Intrinsic(name) => match intrinsic(name) {
                Intrinsic::Mutating(run) => run(receiver, args, span),
                Intrinsic::Plain(_) | Intrinsic::HigherOrder(_) => unreachable!(
                    "the checker calls only a `var self` method on a place, and every \
                     prelude `var self` method is a mutating intrinsic"
                ),
            },
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

/// The arguments that take slots: each of `args` whose parameter
/// `binds`. Kept out of line, so the frame of a call does not hold the
/// iterator.
#[inline(never)]
fn binding(binds: impl Iterator<Item = bool>, args: Vec<Value>) -> Vec<Value> {
    binds
        .zip(args)
        .filter(|(binds, _)| *binds)
        .map(|(_, arg)| arg)
        .collect()
}

/// Where a function's body returns when it falls off its end: its tail
/// expression, or its closing brace when it ends with a statement.
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

/// The intrinsic named `name`.
fn intrinsic(name: &str) -> Intrinsic {
    intrinsics::lookup(name).expect("`crate::run` refuses a call to a missing intrinsic")
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
