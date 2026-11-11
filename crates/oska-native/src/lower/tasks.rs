//! Scopes and tasks, in the interpreter's one-at-a-time order
//! (`oska-run`'s `tasks.rs` is the model).
//!
//! # Representation
//!
//! A scope is an object `[count][closed][head][tail]` (`layout.rs`): the
//! queue of the tasks spawned and not yet started, oldest first. A task is
//! `[count][scope][runner][code][result][state][next]`: the scope it was
//! spawned on (a reference), the address of its runner, the function value
//! to call (0 once it has run), its result and whether it has run. A queued
//! task holds one reference to itself, for the queue.
//!
//! The **runner** ([`Instance::TaskRunner`]) is generated for each `spawn`
//! call: it calls the function value as the interpreter's `call_value`
//! does, counting its depth and panicking at the `spawn` call's span, keeps
//! the result, lets go of the function value and of the queue's reference.
//! **Running tasks** ([`Instance::RunTasks`]) takes tasks off a scope's
//! queue, oldest first, and calls their runners, until the task it is told
//! of has run (`join`) or the queue is empty (the end of the block).
//!
//! # Where tasks run
//!
//! - `spawn` queues, and panics with `P0005` on a closed scope.
//! - `join` runs the queue up to the joined task, then returns its result.
//! - The end of the block, and every way out of it (`return`, `?`, `break`
//!   and `continue` out of a loop around it: the handle is a [`Held`] value)
//!   runs the rest of the queue, then closes the scope.
//!
//! A task's result, and the scope, are counted values: a task handle may
//! outlive its scope.

use oska_check::program::{Expr, Type};
use oska_syntax::{Diagnostic, Span};

use super::{panics, Body, Held, Instance, Slot};
use crate::ir::{Cond, Inst, Temp};
use crate::layout;
use crate::NOWHERE;

/// The type of a scope handle.
fn scope_type() -> Type {
    Type::Named {
        name: layout::SCOPE.to_string(),
        args: Vec::new(),
    }
}

/// The type of the function value a task of result type `result` runs.
fn code_type(result: &Type) -> Type {
    Type::Fn {
        params: Vec::new(),
        ret: Box::new(result.clone()),
    }
}

impl Body<'_, '_> {
    /// `scope |s| body`: the value of `body`, which spawns tasks on the
    /// handle `s`, in the slot after the ones in scope.
    pub(super) fn scope_block(&mut self, body: &Expr) -> Result<Temp, Diagnostic> {
        let scope = self.call_runtime("scope_new", Vec::new(), NOWHERE)?;
        let ty = scope_type();
        let height = self.slots.len();
        // The block owns the handle; the slot only borrows it.
        self.slots.push(Slot {
            temp: scope,
            release: false,
            ty: ty.clone(),
        });
        self.held.push(Held {
            temp: scope,
            ty: ty.clone(),
            loops: self.loops.len(),
            scope: true,
        });
        let value = self.expr(body)?;
        self.held.pop();
        self.finish_scope(scope)?;
        self.release(scope, &ty)?;
        self.slots.truncate(height);
        Ok(value)
    }

    /// Run the tasks still queued on `scope`, then close it: its block has
    /// ended.
    pub(super) fn finish_scope(&mut self, scope: Temp) -> Result<(), Diagnostic> {
        let all = self.constant(0);
        self.run_tasks(scope, all)?;
        let closed = self.constant(1);
        self.store(scope, layout::SCOPE_CLOSED, closed);
        Ok(())
    }

    /// Run the queued tasks of `scope` until the task `until` has run, or
    /// all of them when `until` is 0.
    fn run_tasks(&mut self, scope: Temp, until: Temp) -> Result<(), Diagnostic> {
        let origin = self.origin.unwrap_or(NOWHERE);
        let id = self.lowering.request(Instance::RunTasks, origin);
        self.call_id(id, vec![scope, until], NOWHERE)?;
        Ok(())
    }

    /// `Scope.spawn` and `Task.join`, with `values` evaluated, for `expr`.
    /// `None` for any other intrinsic.
    pub(super) fn task_intrinsic(
        &mut self,
        name: &str,
        values: &[Temp],
        expr: &Expr,
    ) -> Result<Option<Temp>, Diagnostic> {
        match (name, values) {
            ("Scope.spawn", &[scope, code]) => {
                let closed = self.load(scope, layout::SCOPE_CLOSED);
                let finished = self.panic_static(
                    panics::SPAWN_ON_FINISHED_SCOPE,
                    panics::FINISHED_SCOPE,
                    expr.span,
                )?;
                let none = self.constant(0);
                self.push(Inst::Branch {
                    cond: Cond::Ne,
                    lhs: closed,
                    rhs: none,
                    target: finished,
                });
                let Type::Named { args, .. } = self.ty(&expr.ty) else {
                    return Err(self.refuse(expr.span, "a `spawn` that gives no task"));
                };
                let result = args.into_iter().next().unwrap_or(Type::Unit);
                let origin = self.origin.unwrap_or(expr.span);
                let runner = self.lowering.request(
                    Instance::TaskRunner {
                        result,
                        start: expr.span.start,
                        end: expr.span.end,
                    },
                    origin,
                );
                let runner = self.function_address(runner);
                let task = self.call_runtime("task_new", vec![scope, runner, code], NOWHERE)?;
                // The task takes over the function value's reference.
                self.owned.remove(&code);
                self.consume(scope)?;
                self.own(task, &expr.ty);
                Ok(Some(task))
            }
            ("Task.join", &[task]) => {
                let scope = self.load(task, layout::TASK_SCOPE);
                self.run_tasks(scope, task)?;
                let result = self.load(task, layout::TASK_RESULT);
                // The task keeps its own reference to the result.
                if self.counted(&expr.ty) {
                    self.retain(result);
                }
                self.consume(task)?;
                self.own(result, &expr.ty);
                Ok(Some(result))
            }
            _ => Ok(None),
        }
    }

    /// The body of [`Instance::TaskRunner`] for a task that gives `result`,
    /// spawned by the call at `span`. Its parameter is the task, which the
    /// queue's reference keeps alive until the end.
    pub(super) fn task_runner(&mut self, result: &Type, span: Span) -> Result<(), Diagnostic> {
        let task = self.temp();
        let code = self.load(task, layout::TASK_CODE);
        let value = self.invoke(code, &[], span)?;
        self.store(task, layout::TASK_RESULT, value);
        let done = self.constant(1);
        self.store(task, layout::TASK_STATE, done);
        let nothing = self.constant(0);
        self.store(task, layout::TASK_CODE, nothing);
        self.release(code, &code_type(result))?;
        let ty = Type::Named {
            name: layout::TASK.to_string(),
            args: vec![result.clone()],
        };
        self.release(task, &ty)?;
        self.push(Inst::Return(nothing));
        Ok(())
    }

    /// The body of [`Instance::RunTasks`]: its parameters are a scope and
    /// a task of it (or 0).
    pub(super) fn run_tasks_function(&mut self) -> Result<(), Diagnostic> {
        let scope = self.temp();
        let until = self.temp();
        let (next, pop, done) = (self.label(), self.label(), self.label());
        self.push(Inst::Label(next));
        self.push(Inst::JumpIfZero {
            cond: until,
            target: pop,
        });
        let state = self.load(until, layout::TASK_STATE);
        let none = self.constant(0);
        self.push(Inst::Branch {
            cond: Cond::Ne,
            lhs: state,
            rhs: none,
            target: done,
        });
        self.push(Inst::Label(pop));
        let task = self.call_runtime("scope_pop", vec![scope], NOWHERE)?;
        self.push(Inst::JumpIfZero {
            cond: task,
            target: done,
        });
        let runner = self.load(task, layout::TASK_RUNNER);
        let dst = self.temp();
        self.push(Inst::CallIndirect {
            dst,
            target: runner,
            args: vec![task],
        });
        self.push(Inst::Jump(next));
        self.push(Inst::Label(done));
        let unit = self.constant(0);
        self.push(Inst::Return(unit));
        Ok(())
    }

    /// The body of [`Instance::Drop`] for the task type `ty`, before the
    /// object is freed: let go of its scope, its function value if it has
    /// not run, its result if it has.
    pub(super) fn task_drop(&mut self, object: Temp, ty: &Type) -> Result<(), Diagnostic> {
        let Type::Named { args, .. } = ty else {
            return Ok(());
        };
        let result = args.first().cloned().unwrap_or(Type::Unit);
        let scope = self.load(object, layout::TASK_SCOPE);
        self.release(scope, &scope_type())?;
        let code = self.load(object, layout::TASK_CODE);
        let ran = self.label();
        self.push(Inst::JumpIfZero {
            cond: code,
            target: ran,
        });
        self.release(code, &code_type(&result))?;
        self.push(Inst::Label(ran));
        if self.counted(&result) {
            let state = self.load(object, layout::TASK_STATE);
            let skip = self.label();
            self.push(Inst::JumpIfZero {
                cond: state,
                target: skip,
            });
            let value = self.load(object, layout::TASK_RESULT);
            self.release(value, &result)?;
            self.push(Inst::Label(skip));
        }
        Ok(())
    }
}
