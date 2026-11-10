//! Scopes and tasks.
//!
//! `scope |s| { ... }` is a block whose handle `s` spawns tasks. In v0
//! tasks run one at a time, in the order they were spawned, which keeps
//! the language's semantics (tasks share only immutable values) and makes
//! every run of a program give the same result (`docs/design.md`,
//! "Concurrency in v0").
//!
//! - `spawn` queues the task. Nothing runs yet, so a task never starts
//!   in the middle of another task or of the block that spawned it.
//! - `join` runs the queued tasks of the scope, oldest first, up to and
//!   including the one joined, and returns its result. A task that was
//!   already run just returns its result.
//! - When the block ends, however it ends, the tasks still queued run in
//!   order: a scope does not exit until all its tasks have finished.
//! - Then the scope is **closed**. The checker already keeps a handle
//!   from outliving its block (`E0292`), so spawning on a closed scope
//!   cannot happen in a checked program; the runtime panics (`P0005`)
//!   anyway, so work can never run outside a scope even if that check
//!   has a hole.

use std::collections::VecDeque;

use oska_syntax::Span;

use crate::interp::Interpreter;
use crate::intrinsics::HigherOrder;
use crate::panics::{codes, Panic};
use crate::value::Value;

/// Every task intrinsic, by qualified name.
pub const HIGHER_ORDER: &[(&str, HigherOrder)] =
    &[("Scope.spawn", scope_spawn), ("Task.join", task_join)];

/// The scopes and tasks of one run, indexed by the numbers
/// [`Value::Scope`] and [`Value::Task`] carry.
#[derive(Debug, Default)]
pub struct Tasks {
    scopes: Vec<ScopeState>,
    tasks: Vec<TaskState>,
}

#[derive(Debug)]
struct ScopeState {
    /// Whether its block is still running.
    open: bool,
    /// The tasks spawned and not yet started, oldest first.
    pending: VecDeque<usize>,
}

#[derive(Debug)]
struct TaskState {
    /// The scope the task was spawned on.
    scope: usize,
    /// The closure to run; `Unit` once it has run.
    code: Value,
    /// The `spawn` call, where a named function run as the task is
    /// called.
    span: Span,
    /// The closure's result, once it has run.
    result: Option<Value>,
}

impl Tasks {
    /// Open a scope, for a `scope` block that is starting.
    pub fn open(&mut self) -> usize {
        self.scopes.push(ScopeState {
            open: true,
            pending: VecDeque::new(),
        });
        self.scopes.len() - 1
    }

    /// Close `scope`: its block has ended, so its handle is invalid.
    pub fn close(&mut self, scope: usize) {
        self.scopes[scope].open = false;
    }

    /// Queue `code`, spawned by the call at `span`, on `scope`, or `None`
    /// when the scope is closed.
    fn spawn(&mut self, scope: usize, code: Value, span: Span) -> Option<usize> {
        if !self.scopes[scope].open {
            return None;
        }
        self.tasks.push(TaskState {
            scope,
            code,
            span,
            result: None,
        });
        let task = self.tasks.len() - 1;
        self.scopes[scope].pending.push_back(task);
        Some(task)
    }

    /// Take the oldest task of `scope` that has not started.
    fn next_pending(&mut self, scope: usize) -> Option<usize> {
        self.scopes[scope].pending.pop_front()
    }
}

impl Interpreter<'_, '_> {
    /// Run the tasks of `scope` that are still queued, in spawn order.
    pub(crate) fn finish_scope(&mut self, scope: usize) -> Result<(), Panic> {
        while let Some(task) = self.tasks.next_pending(scope) {
            self.run_task(task)?;
        }
        Ok(())
    }

    /// Run `task` to completion and keep its result.
    fn run_task(&mut self, task: usize) -> Result<(), Panic> {
        let code = std::mem::replace(&mut self.tasks.tasks[task].code, Value::Unit);
        let span = self.tasks.tasks[task].span;
        let result = self.call_value(&code, Vec::new(), span)?;
        self.tasks.tasks[task].result = Some(result);
        Ok(())
    }

    /// The result of `task`, running the tasks queued before it first.
    fn join(&mut self, task: usize) -> Result<Value, Panic> {
        let scope = self.tasks.tasks[task].scope;
        while self.tasks.tasks[task].result.is_none() {
            let next = self.tasks.next_pending(scope).expect(
                "a task without a result is queued: a task can only join tasks spawned \
                 before it, which have started",
            );
            self.run_task(next)?;
        }
        Ok(self.tasks.tasks[task]
            .result
            .clone()
            .expect("the loop above ran the task"))
    }
}

/// `Scope.spawn(self, task)`: queue `task`. Panics with `P0005` when the
/// scope's block has ended.
fn scope_spawn(
    interp: &mut Interpreter<'_, '_>,
    args: Vec<Value>,
    span: Span,
) -> Result<Value, Panic> {
    let Value::Scope(scope) = args[0] else {
        unreachable!("the checker guarantees a scope handle, found {:?}", args[0]);
    };
    match interp.tasks.spawn(scope, args[1].clone(), span) {
        Some(task) => Ok(Value::Task(task)),
        None => Err(Panic::new(
            codes::SPAWN_ON_FINISHED_SCOPE,
            "`spawn` was called on a scope whose block has already ended",
            span,
        )),
    }
}

/// `Task.join(self)`: the task's result.
fn task_join(interp: &mut Interpreter<'_, '_>, args: Vec<Value>, _: Span) -> Result<Value, Panic> {
    let Value::Task(task) = args[0] else {
        unreachable!("the checker guarantees a task, found {:?}", args[0]);
    };
    interp.join(task)
}

#[cfg(test)]
mod tests {
    //! The checker (`E0292`) keeps a handle inside its block, so no
    //! source program can spawn on a finished scope. The runtime guard is
    //! tested on a checked program built by hand, in which a handle
    //! escapes anyway.

    use std::rc::Rc;

    use oska_check::program::{Callee, Closure, Expr, ExprKind, Function, Program, Stmt};

    use super::*;
    use crate::host::FakeHost;

    fn at(start: usize, end: usize, kind: ExprKind) -> Expr {
        Expr {
            kind,
            span: Span { start, end },
        }
    }

    /// `fn main() { let h = scope |s| { s }; h.spawn(|| ()) }`, with the
    /// `spawn` call spanning bytes 40..52.
    fn spawning_on_a_finished_scope() -> Program {
        let escaped = at(
            10,
            30,
            ExprKind::Scope(Box::new(at(
                20,
                30,
                ExprKind::Block {
                    stmts: Vec::new(),
                    value: Some(Box::new(at(22, 23, ExprKind::Local(0)))),
                },
            ))),
        );
        let task = at(
            47,
            51,
            ExprKind::Closure(Rc::new(Closure {
                params: Vec::new(),
                body: at(50, 51, ExprKind::Unit),
            })),
        );
        let spawn = at(
            40,
            52,
            ExprKind::Call {
                callee: Callee::Intrinsic("Scope.spawn".to_string()),
                args: vec![at(40, 41, ExprKind::Local(0)), task],
            },
        );
        let body = at(
            0,
            60,
            ExprKind::Block {
                stmts: vec![Stmt::Let {
                    binds: true,
                    value: escaped,
                }],
                value: Some(Box::new(spawn)),
            },
        );
        let mut program = Program::default();
        program.functions.insert(
            "main".to_string(),
            Function {
                name: "main".to_string(),
                receiver: false,
                params: Vec::new(),
                body,
            },
        );
        program
    }

    #[test]
    fn spawning_on_a_finished_scope_panics_with_p0005_at_the_call() {
        let program = spawning_on_a_finished_scope();
        let main = &program.functions["main"];
        let mut host = FakeHost::new();
        let panic = Interpreter::new(&program, &mut host)
            .call(main, Vec::new())
            .expect_err("the scope is finished");
        assert_eq!(panic.code, codes::SPAWN_ON_FINISHED_SCOPE);
        assert_eq!(panic.span, Span { start: 40, end: 52 });
    }

    #[test]
    fn a_scope_accepts_tasks_only_while_it_is_open() {
        let mut tasks = Tasks::default();
        let scope = tasks.open();
        assert_eq!(
            tasks.spawn(scope, Value::Unit, Span { start: 0, end: 0 }),
            Some(0)
        );
        tasks.close(scope);
        assert_eq!(
            tasks.spawn(scope, Value::Unit, Span { start: 0, end: 0 }),
            None
        );
    }
}
