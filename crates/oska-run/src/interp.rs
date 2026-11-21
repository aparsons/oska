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
use std::collections::HashMap;
use std::ops::ControlFlow;
use std::rc::Rc;
use std::sync::Arc;

use oska_check::builtins::{self, TraitMethod};
use oska_check::limits::{Limit, Limits};
use oska_check::program::{
    self, Arm, Callee, Clause, Expr, ExprKind, Function, Part, Pattern, Place, Program, Stmt, Visit,
};
use oska_syntax::ast::BinaryOp;
use oska_syntax::{Frame as StackFrame, Frames, Span, MAX_FRAMES};

use crate::host::Host;
use crate::intrinsics::{self, Args, Intrinsic};
use crate::memory;
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
    /// The calls of functions and closures running, outermost first:
    /// their depth, and a panic's call stack ([`Interpreter::stamped`]).
    calls: Vec<Call<'p>>,
    /// The name of the frame the run starts in: `main`, or a test's body.
    root: FrameName<'p>,
    /// The caller the next call is made from, when it is not the running
    /// function: a task's call is made from the function that spawned it.
    caller_override: Option<FrameName<'p>>,
    /// The run limits (spec #446), none by default.
    limits: Limits,
    /// The bytes the program has written to stdout and stderr so far,
    /// counted against `--max-output`.
    output_written: u64,
    /// The steps the program may take before [`Interpreter::step`] takes
    /// its slow path: under `--max-steps`, the steps left, counted down
    /// from the limit; without it, `u64::MAX`, refilled if ever spent. One
    /// decrement and compare per step, with or without a limit.
    fuel: u64,
    /// Under `--timeout`, its deadline. Boxed, so that a run without it
    /// keeps the interpreter as small as it was.
    deadline: Option<Box<Deadline>>,
    /// What calls have looked up so far, and the buffers intrinsic calls
    /// pass their arguments in. Boxed, so that the interpreter stays as
    /// small as it was: unboxed, it made the musl build's `lists`
    /// benchmark about 12% slower, with the same instructions (#505).
    calls_seen: Box<CallCache<'p>>,
    /// The texts `String.chars` gives ASCII characters, shared (#501).
    pub(crate) ascii_texts: crate::data::AsciiTexts,
    /// The texts of the string literals evaluated so far, each made once.
    literals: memory::Literals,
}

/// What calls have looked up so far, so that each is looked up once, and
/// the buffers intrinsic calls pass their arguments in (#505).
#[derive(Default)]
struct CallCache<'p> {
    /// Each function of the program called so far, by name, with the
    /// room a call gives its arguments' `Vec` ([`room`]).
    functions: HashMap<&'p str, (&'p Function, usize)>,
    /// What each trait method called so far reaches, by trait, method,
    /// and the receiver's runtime type tag ([`Interpreter::dispatch`]).
    traits: HashMap<String, HashMap<String, HashMap<String, Target<'p>>>>,
    /// Argument buffers no call is using: an intrinsic call takes one,
    /// and gives it back empty, so a call allocates none once the pool
    /// holds one for each intrinsic call running at once.
    buffers: Vec<Args>,
    /// A text an interpolation builds in, kept between interpolations so
    /// that each does not grow a `String` of its own; an interpolation
    /// inside another's hole finds it taken and builds in a new one.
    text: String,
}

/// What a call reaches once its callee is resolved.
#[derive(Clone, Copy)]
enum Target<'p> {
    /// A function of the program, with the room a call gives its
    /// arguments' `Vec` ([`room`]).
    Function(&'p Function, usize),
    Intrinsic(Intrinsic),
}

/// The name of a frame of a panic's call stack: a function's, borrowed
/// from the program, or one made at run time (a closure's, a test's).
#[derive(Debug, Clone)]
pub(crate) enum FrameName<'p> {
    Program(&'p str),
    Made(Rc<str>),
}

impl FrameName<'_> {
    pub(crate) fn as_str(&self) -> &str {
        match self {
            FrameName::Program(name) => name,
            FrameName::Made(name) => name,
        }
    }
}

/// A call that is running: the function it was made from, the one it
/// runs, and its span.
struct Call<'p> {
    caller: FrameName<'p>,
    callee: FrameName<'p>,
    span: Span,
}

/// A run's `--timeout`, as the interpreter keeps it.
struct Deadline {
    /// The instant the program must stop by.
    at: std::time::Instant,
    /// Under `--max-steps` too, the steps left beyond
    /// [`Interpreter::fuel`], which is then given out [`DEADLINE_EVERY`]
    /// steps at a time, so the slow path can look at the clock. Otherwise
    /// 0.
    steps_held: u64,
}

/// Under `--timeout`, how many steps the interpreter takes between two
/// looks at the clock: few enough that a step-bound loop stops within
/// well under a millisecond of its deadline, many enough that the look
/// costs nothing measurable.
const DEADLINE_EVERY: u64 = 1024;

/// A standard stream the program writes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

impl<'p, 'h> Interpreter<'p, 'h> {
    pub fn new(program: &'p Program, host: &'h mut dyn Host) -> Self {
        Interpreter {
            program,
            host,
            tasks: Tasks::default(),
            calls: Vec::new(),
            root: FrameName::Program("main"),
            caller_override: None,
            limits: Limits::NONE,
            output_written: 0,
            fuel: u64::MAX,
            deadline: None,
            calls_seen: Box::default(),
            ascii_texts: crate::data::AsciiTexts::default(),
            literals: memory::Literals::default(),
        }
    }

    /// This interpreter, under the run `limits`.
    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self.fuel = limits.max_steps.unwrap_or(u64::MAX);
        // A deadline past what the clock can count (on some hosts, a few
        // hundred years) is never reached: no deadline.
        let deadline = limits.timeout.and_then(|ms| {
            std::time::Instant::now().checked_add(std::time::Duration::from_millis(ms))
        });
        if let Some(at) = deadline {
            let steps = self.fuel;
            self.fuel = steps.min(DEADLINE_EVERY);
            let steps_held = match limits.max_steps {
                Some(_) => steps - self.fuel,
                None => 0,
            };
            self.deadline = Some(Box::new(Deadline { at, steps_held }));
        }
        self
    }

    /// Take one **step** (ADR 0021), at `span`: a call of a function,
    /// method or closure written in Oska, or a loop's back-edge. Every
    /// step of the program passes through here, so this is the hook for
    /// whatever a run checks as it goes: under `--max-steps`, the step
    /// past the limit panics with `P0038` instead of being taken; under
    /// `--timeout`, a step past the deadline panics with `P0039`.
    ///
    /// The fast path only spends fuel; everything else is in
    /// [`Interpreter::out_of_fuel`]. Under `--timeout` the fuel is given
    /// out [`DEADLINE_EVERY`] steps at a time, so the deadline is checked
    /// there, and a run without it pays nothing for it.
    #[inline(always)]
    fn step(&mut self, span: Span) -> Result<(), Panic> {
        match self.fuel.checked_sub(1) {
            Some(fuel) => {
                self.fuel = fuel;
                Ok(())
            }
            None => self.out_of_fuel(span),
        }
    }

    /// The step at `span` when [`Interpreter::step`] has no fuel left:
    /// under `--timeout`, a look at the clock, past the deadline a `P0039`
    /// panic, else [`DEADLINE_EVERY`] steps more; under `--max-steps`,
    /// with no steps held back, the step past the limit, a `P0038` panic;
    /// without either, after `u64::MAX` steps, a refill.
    #[cold]
    #[inline(never)]
    fn out_of_fuel(&mut self, span: Span) -> Result<(), Panic> {
        self.check_deadline(span)?;
        if let Some(deadline) = &mut self.deadline {
            let more = match self.limits.max_steps {
                Some(_) => {
                    let more = deadline.steps_held.min(DEADLINE_EVERY);
                    deadline.steps_held -= more;
                    more
                }
                None => DEADLINE_EVERY,
            };
            if more > 0 {
                // This step spends one of them.
                self.fuel = more - 1;
                return Ok(());
            }
        }
        match self.limits.max_steps {
            Some(limit) => Err(Panic::new(
                Limit::Steps.code(),
                Limit::Steps.message(limit),
                span,
            )),
            None => {
                self.fuel = u64::MAX - 1;
                Ok(())
            }
        }
    }

    /// Under `--timeout`, a `P0039` panic at `span` once the deadline has
    /// passed.
    fn check_deadline(&self, span: Span) -> Result<(), Panic> {
        match &self.deadline {
            Some(deadline) if std::time::Instant::now() >= deadline.at => {
                Err(self.time_limit(span))
            }
            _ => Ok(()),
        }
    }

    /// The `P0039` panic at `span`: the program has run past `--timeout`.
    fn time_limit(&self, span: Span) -> Panic {
        let ms = self.limits.timeout.unwrap_or_default();
        Panic::new(Limit::Time.code(), Limit::Time.message(ms), span)
    }

    /// Read a line from standard input for the call at `span`. Under
    /// `--timeout` the wait ends at the deadline, with a `P0039` panic.
    pub(crate) fn read_line(
        &mut self,
        span: Span,
    ) -> Result<std::io::Result<Option<String>>, Panic> {
        let Some(deadline) = self.deadline.as_ref().map(|d| d.at) else {
            return Ok(self.host.read_line());
        };
        self.check_deadline(span)?;
        // `None`: the deadline passed while the program waited.
        self.host
            .read_line_before(deadline)
            .ok_or_else(|| self.time_limit(span))
    }

    /// The host the program runs on.
    pub(crate) fn host(&mut self) -> &mut dyn Host {
        self.host
    }

    /// Under `--max-output`, the bytes the program may still write.
    pub(crate) fn output_left(&self) -> Option<u64> {
        let limit = self.limits.max_output?;
        Some(limit - self.output_written)
    }

    /// Write `text`, which the program writes, to `stream`. Under
    /// `--max-output`, a write that would take stdout and stderr together
    /// past the limit writes nothing and panics with `P0041` at `span`.
    pub(crate) fn write_output(
        &mut self,
        stream: Stream,
        text: &str,
        span: Span,
    ) -> Result<std::io::Result<()>, Panic> {
        if let Some(limit) = self.limits.max_output {
            let bytes = u64::try_from(text.len()).unwrap_or(u64::MAX);
            if bytes > limit - self.output_written {
                return Err(Panic::new(
                    Limit::Output.code(),
                    Limit::Output.message(limit),
                    span,
                ));
            }
            self.output_written += bytes;
        }
        Ok(match stream {
            Stream::Stdout => self.host.write_stdout(text),
            Stream::Stderr => self.host.write_stderr(text),
        })
    }

    /// Enter a call of a function or closure, made at `span`: a step
    /// ([`Interpreter::step`]), then a panic with `P0021` when calls
    /// already nest [`MAX_CALL_DEPTH`] deep. Every call of a function,
    /// method or closure written in Oska enters here, and nothing else
    /// does (`main` is not called by the program, so it is no step). Each
    /// successful entry is paired with [`Interpreter::leave`].
    fn enter(&mut self, callee: FrameName<'p>, span: Span) -> Result<(), Panic> {
        let caller = self.caller_override.take();
        self.step(span)?;
        if self.calls.len() >= MAX_CALL_DEPTH || stack_exhausted() {
            return Err(Panic::new(
                codes::CALL_DEPTH_EXCEEDED,
                "calls nest too deep: is this recursion missing its base case?",
                span,
            ));
        }
        let caller = caller.unwrap_or_else(|| self.current());
        self.calls.push(Call {
            caller,
            callee,
            span,
        });
        Ok(())
    }

    fn leave(&mut self) {
        self.calls.pop();
    }

    /// The name of the frame running now: the innermost call's function,
    /// or the root's.
    pub(crate) fn current(&self) -> FrameName<'p> {
        self.innermost(&self.calls)
    }

    /// The name of the frame running when `calls`, the outermost of the
    /// running calls, are all that run: the last one's function, or the
    /// root's.
    fn innermost(&self, calls: &[Call<'p>]) -> FrameName<'p> {
        calls
            .last()
            .map_or_else(|| self.root.clone(), |call| call.callee.clone())
    }

    /// Run a test's body as the root frame, named `name`.
    pub(crate) fn with_root(mut self, name: &str) -> Self {
        self.root = FrameName::Made(Rc::from(name));
        self
    }

    /// Make the next call from `caller` instead of the running function.
    pub(crate) fn call_from(&mut self, caller: FrameName<'p>) {
        self.caller_override = Some(caller);
    }

    /// Forget a caller [`Interpreter::call_from`] set that no call took.
    pub(crate) fn clear_caller(&mut self) {
        self.caller_override = None;
    }

    /// `panic` with its call stack, unless it has one already: the frame
    /// running now at the panic's span, then each running call, innermost
    /// first, as the function it was made from at its span. At most
    /// [`MAX_FRAMES`], with a count of the rest. `skip` innermost calls
    /// are left out: a broken `requires` clause (`P0006`) is the caller's.
    pub(crate) fn stamped_skipping(&self, mut panic: Panic, skip: usize) -> Panic {
        if panic.frames.is_some() {
            return panic;
        }
        panic.frames = Some(self.frames_skipping(panic.span, skip));
        panic
    }

    /// The call stack at `span` in the frame running now, as a panic there
    /// would have it ([`Interpreter::stamped_skipping`]), `skip` innermost
    /// calls left out.
    fn frames_skipping(&self, span: Span, skip: usize) -> Frames {
        let calls = &self.calls[..self.calls.len().saturating_sub(skip)];
        let current = self.innermost(calls);
        let mut frames = vec![StackFrame::new(current.as_str(), span)];
        frames.extend(
            calls
                .iter()
                .rev()
                .take(MAX_FRAMES - 1)
                .map(|call| StackFrame::new(call.caller.as_str(), call.span)),
        );
        let omitted = calls.len() + 1 - frames.len();
        Frames { frames, omitted }
    }

    /// The call stack at `span` in the frame running now, as a panic there
    /// would report it: a snapshot assertion's (`crate::snapshots`).
    pub(crate) fn frames_at(&self, span: Span) -> Frames {
        self.frames_skipping(span, 0)
    }

    /// [`Interpreter::stamped_skipping`] with nothing skipped: where a
    /// panic leaves a call, or the root, while the stack is as it was
    /// when the panic happened.
    pub(crate) fn stamped(&self, panic: Panic) -> Panic {
        self.stamped_skipping(panic, 0)
    }

    /// Call `function` with `args`: the receiver first, for a method, then
    /// one per parameter. `span` is the call's. Only a panic unwinds out
    /// of a call.
    pub fn call(
        &mut self,
        function: &Function,
        mut args: Vec<Value>,
        span: Span,
    ) -> Result<Value, Panic> {
        // Taken out of the arguments' own `Vec`, which then becomes the
        // frame (`call_with`): one `Vec` per call.
        let receiver = if function.receiver && !args.is_empty() {
            Some(args.remove(0))
        } else {
            None
        };
        let (value, _) = self.call_with(function, receiver, args, span)?;
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
            Err(Unwind::Panic(panic)) => Err(self.stamped(panic)),
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
                let panic = Panic::new(
                    codes::PRECONDITION_VIOLATED,
                    format!(
                        "precondition violated: `{}` requires `{}`",
                        function.name, clause.text
                    ),
                    span,
                );
                // The caller broke it: the call is not on the stack yet.
                return Err(self.stamped_skipping(panic, 1));
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
            ExprKind::Call { callee, args, .. } => self.call_expr(callee, args, expr.span, frame),
            ExprKind::Block { stmts, value } => self.block(stmts, value.as_deref(), frame),
            _ => self.eval_other(expr, frame),
        }
    }

    /// The value of `expr`, of a kind [`Interpreter::eval`] does not
    /// handle itself.
    #[inline(never)]
    fn eval_other(&mut self, expr: &Expr, frame: &mut Frame) -> Result<Value, Unwind> {
        match &expr.kind {
            ExprKind::Str(text) => Ok(self.literals.text(text, expr.span)?),
            ExprKind::Interpolated(parts) => self.interpolated(parts, expr.span, frame),
            ExprKind::SelfValue => Ok(frame
                .receiver
                .clone()
                .expect("the checker allows `self` only in a method")),
            ExprKind::Neg { operand, int } => {
                let operand = self.eval(operand, frame)?;
                Ok(ops::neg(operand, *int, expr.span)?)
            }
            ExprKind::Not(operand) => Ok(Value::Bool(!self.eval(operand, frame)?.as_bool())),
            ExprKind::BitNot { operand, int } => Ok(ops::bit_not(self.eval(operand, frame)?, *int)),
            ExprKind::Select { .. } => unreachable!(
                "the checker never makes a `Select`: only `secrets::expand` does, for the \
                 native backend, and the interpreter runs `Secret.ct_select` as an intrinsic"
            ),
            ExprKind::Range { start, end } => {
                let start = self.eval(start, frame)?;
                let end = self.eval(end, frame)?;
                Ok(memory::range(start, end, expr.span)?)
            }
            ExprKind::List(items) => {
                let items = self.eval_all(items, frame)?;
                Ok(memory::list(items, expr.span)?)
            }
            ExprKind::Construct { ctor, fields } => {
                let fields = self.eval_all(fields, frame)?;
                Ok(memory::construct(ctor, fields, expr.span)?)
            }
            ExprKind::Field { base, index } => Ok(self.eval(base, frame)?.field(*index)),
            ExprKind::Closure(code) => Ok(self.closure(code, expr.span, frame)?),
            ExprKind::CallValue { callee, args } => {
                self.call_value_expr(callee, args, expr.span, frame)
            }
            ExprKind::MutatingCall {
                callee,
                receiver,
                args,
                ..
            } => self.mutating_call_expr(callee, receiver, args, expr.span, frame),
            ExprKind::Match { scrutinee, arms } => {
                let scrutinee = self.eval(scrutinee, frame)?;
                self.match_arms(&scrutinee, arms, frame)
            }
            ExprKind::Try {
                operand, convert, ..
            } => self.try_expr(operand, convert.as_ref(), expr.span, frame),
            ExprKind::FnValue { callee, .. } => Ok(Value::Function(Rc::new(callee.clone()))),
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
    fn closure(
        &mut self,
        code: &Arc<program::Closure>,
        span: Span,
        frame: &Frame,
    ) -> Result<Value, Panic> {
        debug_assert_eq!(
            frame.slots.len(),
            code.height,
            "the frame's slots match the checker's locals"
        );
        let closure = ClosureValue {
            code: Arc::clone(code),
            captured: code
                .captures
                .iter()
                .map(|&slot| frame.slots[slot].clone())
                .collect(),
            receiver: frame.receiver.clone().filter(|_| code.captures_self),
            frame: Rc::from(StackFrame::closure_in(self.current().as_str())),
        };
        memory::closure(closure, span)
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
        match callee {
            Callee::Function(name) => {
                let (function, values) = self.function_args(name, 0, args, frame)?;
                Ok(self.call_function(function, values, span)?)
            }
            Callee::Intrinsic(name) => {
                let intrinsic = intrinsics::resolved(name);
                let mut buffer = self.buffer();
                let result = match self.eval_onto(&mut buffer, args, frame) {
                    Ok(()) => self
                        .run_intrinsic(intrinsic, &mut buffer, span)
                        .map_err(Unwind::from),
                    Err(unwind) => Err(unwind),
                };
                self.give_back(buffer);
                result
            }
            Callee::Trait { .. } => self.trait_call_expr(callee, args, span, frame),
        }
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
        if let Callee::Intrinsic(name) = callee {
            let Intrinsic::Mutating(run) = intrinsics::resolved(name) else {
                unreachable!(
                    "the checker calls only a `var self` method on a place, and every \
                     prelude `var self` method is a mutating intrinsic"
                )
            };
            let mut buffer = self.buffer();
            let result = match self.eval_onto(&mut buffer, args, frame) {
                Ok(()) => {
                    let before = metered_place(receiver).then(|| field_path(receiver, frame));
                    let place = place_mut(receiver, frame);
                    let value = run(place, &mut buffer, span);
                    if let Some(before) = before {
                        relocate_path(receiver, frame, &before);
                    }
                    value.map_err(Unwind::from)
                }
                Err(unwind) => Err(unwind),
            };
            self.give_back(buffer);
            return result;
        }
        let args = match callee {
            // The receiver is not among `args`, and takes no slot.
            Callee::Function(name) => self.function_args(name, 1, args, frame)?.1,
            _ => self.eval_all(args, frame)?,
        };
        let before = metered_place(receiver).then(|| field_path(receiver, frame));
        let mut taken = std::mem::replace(place_mut(receiver, frame), Value::Unit);
        let value = self.call_mutating(callee, &mut taken, args, span);
        *place_mut(receiver, frame) = taken;
        if let Some(before) = before {
            relocate_path(receiver, frame, &before);
        }
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
                    Some(convert) => self.call_with_one(convert, error, span)?,
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
            Pattern::Bind(_) => {
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

    /// Each of `exprs`, in order, in a `Vec` of exactly their number: a
    /// list or struct literal's, which `--max-memory` charges by its
    /// capacity.
    fn eval_all(&mut self, exprs: &[Expr], frame: &mut Frame) -> Result<Vec<Value>, Unwind> {
        let mut values = Vec::with_capacity(exprs.len());
        for expr in exprs {
            values.push(self.eval(expr, frame)?);
        }
        Ok(values)
    }

    /// Each of `exprs`, in order, pushed onto `values`.
    fn eval_onto(
        &mut self,
        values: &mut Vec<Value>,
        exprs: &[Expr],
        frame: &mut Frame,
    ) -> Result<(), Unwind> {
        for expr in exprs {
            values.push(self.eval(expr, frame)?);
        }
        Ok(())
    }

    /// A call of the trait method `callee`, made at `span`: its receiver,
    /// the first of `exprs`, is evaluated first, to find what the call
    /// reaches, and the rest go straight into the `Vec` that wants them.
    #[inline(never)]
    fn trait_call_expr(
        &mut self,
        callee: &Callee,
        exprs: &[Expr],
        span: Span,
        frame: &mut Frame,
    ) -> Result<Value, Unwind> {
        let (first, rest) = exprs.split_first().expect("a trait method has a receiver");
        let receiver = self.eval(first, frame)?;
        match self.dispatch(callee, &receiver) {
            Target::Function(function, room) => {
                let mut values = Vec::with_capacity(room.max(exprs.len()));
                values.push(receiver);
                self.eval_onto(&mut values, rest, frame)?;
                Ok(self.call_function(function, values, span)?)
            }
            Target::Intrinsic(intrinsic) => {
                let mut buffer = self.buffer();
                buffer.push(receiver);
                let result = match self.eval_onto(&mut buffer, rest, frame) {
                    Ok(()) => self
                        .run_intrinsic(intrinsic, &mut buffer, span)
                        .map_err(Unwind::from),
                    Err(unwind) => Err(unwind),
                };
                self.give_back(buffer);
                result
            }
        }
    }

    /// An argument buffer for an intrinsic call: one from the pool, or a
    /// new one. Give it back with [`Interpreter::give_back`].
    fn buffer(&mut self) -> Args {
        self.calls_seen
            .buffers
            .pop()
            .unwrap_or_else(|| Args(Vec::new()))
    }

    /// Return `buffer` to the pool, emptied: what the intrinsic left in
    /// it is dropped now, as it was when an intrinsic owned its `Vec`.
    fn give_back(&mut self, mut buffer: Args) {
        buffer.clear();
        self.calls_seen.buffers.push(buffer);
    }

    /// Run `intrinsic` with `args`, the receiver first for a method,
    /// made at `span`. A mutating method runs on its receiver taken out
    /// of `args`, a temporary such as a call's result: the change is
    /// dropped with it.
    fn run_intrinsic(
        &mut self,
        intrinsic: Intrinsic,
        args: &mut Args,
        span: Span,
    ) -> Result<Value, Panic> {
        match intrinsic {
            Intrinsic::Plain(run) => run(self.host, args, span),
            Intrinsic::HigherOrder(run) => run(self, args, span),
            Intrinsic::Mutating(run) => {
                let mut receiver = args.remove(0);
                run(&mut receiver, args, span)
            }
        }
    }

    /// Call `callee` with the one argument `arg`, made at `span`: a
    /// `Display` implementation in an interpolation, or a `From`
    /// conversion behind `?`. The argument goes straight into the `Vec`
    /// the callee wants.
    fn call_with_one(&mut self, callee: &Callee, arg: Value, span: Span) -> Result<Value, Panic> {
        let target = match callee {
            Callee::Function(name) => {
                let (function, room) = self.function(name);
                Target::Function(function, room)
            }
            Callee::Intrinsic(name) => Target::Intrinsic(intrinsics::resolved(name)),
            Callee::Trait { .. } => self.dispatch(callee, &arg),
        };
        match target {
            Target::Function(function, room) => {
                let mut values = Vec::with_capacity(room.max(1));
                values.push(arg);
                self.call_function(function, values, span)
            }
            Target::Intrinsic(intrinsic) => {
                let mut buffer = self.buffer();
                buffer.push(arg);
                let result = self.run_intrinsic(intrinsic, &mut buffer, span);
                self.give_back(buffer);
                result
            }
        }
    }

    /// The program function `name`, and the values of the arguments
    /// `exprs` of a call of it, in a `Vec` with room for its frame
    /// ([`room`]), less `taken`: the slots of the room that are not
    /// among `exprs`, `1` for a mutating method's receiver. Out of line,
    /// so a call of an intrinsic does not carry it.
    #[inline(never)]
    fn function_args(
        &mut self,
        name: &str,
        taken: usize,
        exprs: &[Expr],
        frame: &mut Frame,
    ) -> Result<(&'p Function, Vec<Value>), Unwind> {
        let (function, room) = self.function(name);
        let mut values = Vec::with_capacity(room.saturating_sub(taken).max(exprs.len()));
        for expr in exprs {
            values.push(self.eval(expr, frame)?);
        }
        Ok((function, values))
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
            Stmt::Let { binds, value, .. } => {
                let value = self.eval(value, frame)?;
                if *binds {
                    frame.slots.push(value);
                }
            }
            Stmt::Assign { place, value, .. } => {
                let value = self.eval(value, frame)?;
                // The old value is dropped: a copy elsewhere keeps its own
                // reference to it.
                if metered_place(place) {
                    assign_metered(place, value, frame);
                } else {
                    *place_mut(place, frame) = value;
                }
            }
            Stmt::Return { value, span } => {
                let value = match value {
                    Some(value) => self.eval(value, frame)?,
                    None => Value::Unit,
                };
                return Err(Unwind::Return(value, *span));
            }
            Stmt::Break(_) => return Err(Unwind::Break),
            Stmt::Continue(_) => return Err(Unwind::Continue),
            Stmt::While { cond, body, span } => self.while_loop(cond, body, *span, frame)?,
            Stmt::For {
                binds,
                iter,
                body,
                span,
                ..
            } => self.for_loop(*binds, iter, body, *span, frame)?,
        }
        Ok(())
    }

    /// `while`, the statement at `span`.
    #[inline(never)]
    fn while_loop(
        &mut self,
        cond: &Expr,
        body: &Expr,
        span: Span,
        frame: &mut Frame,
    ) -> Result<(), Unwind> {
        while self.eval(cond, frame)?.as_bool() {
            if !self.loop_body(body, span, frame)? {
                break;
            }
        }
        Ok(())
    }

    /// `for` over a range or a list, the statement at `span`. Iterating a
    /// list holds a reference to it, so the body writing the `var` it
    /// came from changes a copy.
    #[inline(never)]
    fn for_loop(
        &mut self,
        binds: bool,
        iter: &Expr,
        body: &Expr,
        span: Span,
        frame: &mut Frame,
    ) -> Result<(), Unwind> {
        let iterated = match &iter.kind {
            // A range written in place is never built.
            ExprKind::Range { start, end } => {
                let start = self.eval(start, frame)?;
                Value::range(start, self.eval(end, frame)?)
            }
            _ => self.eval(iter, frame)?,
        };
        let items: Box<dyn Iterator<Item = Value>> = match iterated {
            Value::Struct { ty, fields } if &*ty == builtins::RANGE => {
                let (Value::Int(start, int), Value::Int(end, _)) = (&fields[0], &fields[1]) else {
                    unreachable!("the checker allows `for` only over a range of integers")
                };
                let int = *int;
                Box::new((*start..*end).map(move |n| Value::Int(n, int)))
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
            let go_on = self.loop_body(body, span, frame);
            frame.slots.truncate(height);
            if !go_on? {
                break;
            }
        }
        Ok(())
    }

    /// Run one iteration of the body of the loop at `span`. Returns
    /// whether the loop goes on: `false` after a `break`. Going on, back
    /// to the condition or the next item, is the loop's back-edge, a
    /// step ([`Interpreter::step`]) spanning the loop: one for each
    /// iteration that ends without `break`, the last one included.
    fn loop_body(&mut self, body: &Expr, span: Span, frame: &mut Frame) -> Result<bool, Unwind> {
        match self.eval(body, frame) {
            Ok(_) | Err(Unwind::Continue) => {
                self.step(span)?;
                Ok(true)
            }
            Err(Unwind::Break) => Ok(false),
            Err(other) => Err(other),
        }
    }

    /// The function the program defines as `name`, with the room a call
    /// gives its arguments ([`room`]).
    fn function(&mut self, name: &str) -> (&'p Function, usize) {
        if let Some(&found) = self.calls_seen.functions.get(name) {
            return found;
        }
        let (name, function) = self
            .program
            .functions
            .get_key_value(name)
            .expect("`crate::run` refuses a call to a function the program lacks");
        let found = (function, room(function));
        self.calls_seen.functions.insert(name, found);
        found
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
                let (function, room) = self.function(name);
                let mut args = args;
                args.reserve_exact(room.saturating_sub(args.len()));
                self.call_function(function, args, span)
            }
            Callee::Intrinsic(name) => {
                self.run_intrinsic(intrinsics::resolved(name), &mut Args(args), span)
            }
            Callee::Trait { .. } => match self.dispatch(callee, &args[0]) {
                Target::Function(function, room) => {
                    let mut args = args;
                    args.reserve_exact(room.saturating_sub(args.len()));
                    self.call_function(function, args, span)
                }
                Target::Intrinsic(intrinsic) => {
                    self.run_intrinsic(intrinsic, &mut Args(args), span)
                }
            },
        }
    }

    /// Call the program's `function` with `args`, the receiver first for
    /// a method, made at `span`.
    fn call_function(
        &mut self,
        function: &'p Function,
        args: Vec<Value>,
        span: Span,
    ) -> Result<Value, Panic> {
        self.enter(FrameName::Program(&function.name), span)?;
        let result = self.call(function, args, span);
        let result = result.map_err(|panic| self.stamped(panic));
        self.leave();
        result
    }

    /// The implementation `callee` reaches for `receiver`: a trait's
    /// method found by the receiver's runtime type tag (ADR 0007), the
    /// function `<Tag as Trait>.method` when the program has one and the
    /// prelude's intrinsic `Tag.method` otherwise. Any other callee is
    /// already resolved.
    ///
    /// What it finds is kept by trait, method and tag, so a call looks it
    /// up, and makes its names, only the first time.
    fn dispatch(&mut self, callee: &Callee, receiver: &Value) -> Target<'p> {
        let Callee::Trait { trait_name, method } = callee else {
            unreachable!("only a trait method is dispatched")
        };
        let tag = receiver.type_tag();
        let seen = self
            .calls_seen
            .traits
            .get(trait_name.as_str())
            .and_then(|methods| methods.get(method.as_str()))
            .and_then(|tags| tags.get(tag));
        if let Some(&target) = seen {
            return target;
        }
        let name = program::trait_method(tag, trait_name, method);
        let target = if self.program.functions.contains_key(&name) {
            let (function, room) = self.function(&name);
            Target::Function(function, room)
        } else {
            let intrinsic = intrinsics::lookup(&format!("{tag}.{method}"))
                .expect("`crate::run` refuses a call to a missing intrinsic");
            Target::Intrinsic(intrinsic)
        };
        self.calls_seen
            .traits
            .entry(trait_name.clone())
            .or_default()
            .entry(method.clone())
            .or_default()
            .insert(tag.to_string(), target);
        target
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
        match self.dispatch(&callee, &receiver) {
            Target::Function(function, room) => {
                let mut values = Vec::with_capacity(room.max(1 + args.len()));
                values.push(receiver);
                values.extend(args);
                self.call_function(function, values, span)
            }
            Target::Intrinsic(intrinsic) => {
                let mut buffer = self.buffer();
                buffer.push(receiver);
                buffer.extend(args);
                let result = self.run_intrinsic(intrinsic, &mut buffer, span);
                self.give_back(buffer);
                result
            }
        }
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
        let mut slots = Vec::with_capacity(closure.code.height + args.len());
        slots.resize(closure.code.height, Value::Unit);
        for (&slot, value) in closure.code.captures.iter().zip(&closure.captured) {
            slots[slot] = value.clone();
        }
        slots.append(&mut binding(
            closure.code.params.iter().map(|p| p.binds),
            args,
        ));
        let mut frame = Frame {
            slots,
            receiver: closure.receiver.clone(),
        };
        self.enter(FrameName::Made(Rc::clone(&closure.frame)), span)?;
        let result = self.eval(&closure.code.body, &mut frame);
        let result = match result {
            Err(Unwind::Panic(panic)) => Err(Unwind::Panic(self.stamped(panic))),
            other => other,
        };
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
                let (function, room) = self.function(name);
                self.call_mutating_function(function, room, receiver, args, span)
            }
            Callee::Intrinsic(name) => match intrinsics::resolved(name) {
                Intrinsic::Mutating(run) => run(receiver, &mut Args(args), span),
                Intrinsic::Plain(_) | Intrinsic::HigherOrder(_) => unreachable!(
                    "the checker calls only a `var self` method on a place, and every \
                     prelude `var self` method is a mutating intrinsic"
                ),
            },
            Callee::Trait { .. } => match self.dispatch(callee, receiver) {
                Target::Function(function, room) => {
                    self.call_mutating_function(function, room, receiver, args, span)
                }
                Target::Intrinsic(Intrinsic::Mutating(run)) => run(receiver, &mut Args(args), span),
                Target::Intrinsic(_) => unreachable!(
                    "the checker calls only a `var self` method on a place, and every \
                     prelude `var self` method is a mutating intrinsic"
                ),
            },
        }
    }

    /// Call the program's mutating method `function` on `receiver`, which
    /// it changes, with `args`. `room` is its frame's ([`room`]), the
    /// receiver's slot included.
    fn call_mutating_function(
        &mut self,
        function: &'p Function,
        room: usize,
        receiver: &mut Value,
        mut args: Vec<Value>,
        span: Span,
    ) -> Result<Value, Panic> {
        args.reserve_exact(room.saturating_sub(1 + args.len()));
        let taken = std::mem::replace(receiver, Value::Unit);
        self.enter(FrameName::Program(&function.name), span)?;
        let result = self.call_with(function, Some(taken), args, span);
        let result = result.map_err(|panic| self.stamped(panic));
        self.leave();
        let (value, after) = result?;
        *receiver = after.expect("a method keeps its receiver");
        Ok(value)
    }

    /// An interpolated string: each hole displayed through the
    /// implementation the checker chose.
    fn interpolated(
        &mut self,
        parts: &[Part],
        span: Span,
        frame: &mut Frame,
    ) -> Result<Value, Unwind> {
        if memory::metered() {
            return self.interpolated_metered(parts, span, frame);
        }
        let mut text = std::mem::take(&mut self.calls_seen.text);
        let built = self.interpolate_into(&mut text, parts, frame);
        let value = built.map(|()| Value::str(&text));
        // A text kept for the next is cleared; a long one is let go of,
        // so a page built once is not held for the rest of the run.
        if text.capacity() <= 1 << 16 {
            text.clear();
            self.calls_seen.text = text;
        }
        value
    }

    /// Push each of `parts` onto `text`, each hole displayed.
    fn interpolate_into(
        &mut self,
        text: &mut String,
        parts: &[Part],
        frame: &mut Frame,
    ) -> Result<(), Unwind> {
        for part in parts {
            match part {
                Part::Text(t) => text.push_str(t),
                Part::Hole { value, display, .. } => {
                    let span = value.span;
                    let value = self.eval(value, frame)?;
                    let shown = self.call_with_one(display, value, span)?;
                    text.push_str(shown.as_str());
                }
            }
        }
        Ok(())
    }

    /// [`Interpreter::interpolated`] under `--max-memory`: each hole is
    /// displayed first, and the whole text charged before it is built.
    #[cold]
    #[inline(never)]
    fn interpolated_metered(
        &mut self,
        parts: &[Part],
        span: Span,
        frame: &mut Frame,
    ) -> Result<Value, Unwind> {
        let mut shown = Vec::with_capacity(parts.len());
        for part in parts {
            if let Part::Hole { value, display, .. } = part {
                let at = value.span;
                let value = self.eval(value, frame)?;
                shown.push(self.call_with_one(display, value, at)?);
            }
        }
        let len = parts
            .iter()
            .map(|part| match part {
                Part::Text(t) => t.len(),
                Part::Hole { .. } => 0,
            })
            .sum::<usize>()
            + shown.iter().map(|s| s.as_str().len()).sum::<usize>();
        let text = memory::text_of_len(len, span, || {
            let mut text = String::with_capacity(len);
            let mut holes = shown.iter();
            for part in parts {
                match part {
                    Part::Text(t) => text.push_str(t),
                    Part::Hole { .. } => {
                        text.push_str(holes.next().map_or("", Value::as_str));
                    }
                }
            }
            text
        })?;
        Ok(text)
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

/// Whether a write at `place` is one the meter follows: to a field, under
/// `--max-memory`. A write to a local never is, with or without it.
#[inline]
fn metered_place(place: &Place) -> bool {
    matches!(place, Place::Field { .. }) && memory::metered()
}

/// Each struct's fields a write at `place` goes through, outermost first,
/// with whether they are shared: what [`relocate_path`] needs after the
/// write.
#[cold]
#[inline(never)]
fn field_path(place: &Place, frame: &Frame) -> Vec<(usize, bool)> {
    let mut lists = Vec::new();
    field_lists(place, frame, &mut lists);
    lists
        .iter()
        .map(|fields| (Rc::as_ptr(fields) as usize, Rc::strong_count(fields) > 1))
        .collect()
}

/// `*place = value` for a write the meter follows ([`metered_place`]).
#[cold]
#[inline(never)]
fn assign_metered(place: &Place, value: Value, frame: &mut Frame) {
    let before = field_path(place, frame);
    *place_mut(place, frame) = value;
    relocate_path(place, frame, &before);
}

/// After a write at `place`, which went through the fields `before` named
/// ([`field_path`]): the meter follows each that was copied or moved.
#[cold]
#[inline(never)]
fn relocate_path(place: &Place, frame: &Frame, before: &[(usize, bool)]) {
    let mut lists = Vec::new();
    field_lists(place, frame, &mut lists);
    for (&(at, shared), fields) in before.iter().zip(lists) {
        memory::relocated(at, shared, fields);
    }
}

/// The fields of each struct a write at `place` goes through, outermost
/// first.
fn field_lists<'f>(place: &Place, frame: &'f Frame, out: &mut Vec<&'f Rc<Vec<Value>>>) {
    if let Place::Field { base, .. } = place {
        field_lists(base, frame, out);
        if let Value::Struct { fields, .. } = place_ref(base, frame) {
            out.push(fields);
        }
    }
}

/// The value at `place`, to read.
fn place_ref<'f>(place: &Place, frame: &'f Frame) -> &'f Value {
    match place {
        Place::Local(slot) => &frame.slots[*slot],
        Place::SelfValue => frame
            .receiver
            .as_ref()
            .expect("the checker allows `self` only in a method"),
        Place::Field { base, index } => match place_ref(base, frame) {
            Value::Struct { fields, .. } => &fields[*index],
            other => unreachable!("the checker guarantees a struct, found {other:?}"),
        },
    }
}

/// The arguments that take slots: each of `args` whose parameter
/// `binds`, kept in `args`' own `Vec` (no second one per call). Kept out
/// of line, so the frame of a call does not hold the iterator.
#[inline(never)]
fn binding(mut binds: impl Iterator<Item = bool>, mut args: Vec<Value>) -> Vec<Value> {
    // `retain` visits the arguments in order, one parameter each; an
    // argument past the last parameter takes no slot.
    args.retain(|_| binds.next().unwrap_or(false));
    args
}

/// The room a call of `function` gives its arguments' `Vec`, which
/// becomes its frame ([`Interpreter::call`]): the receiver, for a method,
/// then the slots its body reads. A frame grows one `let` at a time, so
/// without the room it would grow by `realloc`.
///
/// The slots are the highest one the body reads, outside its closures
/// (which number their own). A slot bound and never read is not counted:
/// a frame that reaches it grows once, as it would have.
///
/// It tells a closure's body from the rest by span, which assumes that a
/// closure's span holds everything inside it and that `walk` visits each
/// expression before what is inside it, so whatever it visits after a
/// closure's body starts at or past the closure's end. Lowering keeps
/// spans so; if it did not, `room` would only be wrong as a capacity: a
/// frame given too little grows as it did before #505, one given too
/// much keeps a few spare slots. What a call computes never depends on
/// it.
fn room(function: &Function) -> usize {
    let mut slots = function.params.iter().filter(|p| p.binds).count();
    // The spans of the closures around the expression visited now.
    let mut closures: Vec<Span> = Vec::new();
    let _ = program::walk(&function.body, &mut |visit| {
        if let Visit::Expr(expr) = visit {
            while closures.last().is_some_and(|c| c.end <= expr.span.start) {
                closures.pop();
            }
            match expr.kind {
                ExprKind::Closure(_) => closures.push(expr.span),
                ExprKind::Local(slot) if closures.is_empty() => slots = slots.max(slot + 1),
                _ => {}
            }
        }
        ControlFlow::<()>::Continue(())
    });
    usize::from(function.receiver) + slots
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

#[cfg(test)]
mod tests {
    use super::room;

    /// The room of the function `name` in `src`.
    fn room_of(src: &str, name: &str) -> usize {
        let program = oska_check::check_module(src)
            .program
            .expect("the test program checks");
        room(&program.functions[name])
    }

    #[test]
    fn the_room_counts_the_frames_own_slots_and_not_a_closures() {
        // `a`, `g` and `b` are the frame's slots 0 to 2. The closure's
        // body numbers its own from 2 (`x`, `y`, `z` are 2 to 4), and
        // they are not the frame's.
        let src = "fn f(a: Int) -> Int {
    let g = |x: Int| {
        let y = x + 1
        let z = y * 2
        z
    }
    let b = a + 1
    g(b)
}

fn main() {
    let _ = f(1)
}
";
        assert_eq!(room_of(src, "f"), 3);
    }

    #[test]
    fn a_method_has_room_for_its_receiver_and_a_skipped_parameter_takes_none() {
        let src = "struct P { x: Int }

impl P {
    fn add(self, _: Int, n: Int) -> Int {
        let sum = self.x + n
        sum
    }
}

fn main() {
    let _ = P(x: 1).add(0, 2)
}
";
        // The receiver, then `n` (slot 0) and `sum` (slot 1).
        assert_eq!(room_of(src, "P.add"), 3);
    }
}
