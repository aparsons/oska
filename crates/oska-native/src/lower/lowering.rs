// SPDX-License-Identifier: Apache-2.0
//! The lowering of a program: instances, their queue and the module.

use super::*;

impl<'p> Lowering<'p> {
    pub(super) fn program(&self, source: Source) -> &'p Program {
        match source {
            Source::User => self.user,
            Source::Runtime => self.rt,
        }
    }

    /// The id of `instance`, lowered later if it is new. `origin` is the
    /// user code that reached it.
    pub(super) fn request(&mut self, instance: Instance, origin: Span) -> FnId {
        let instance = self.counted(instance);
        if let Some(&id) = self.ids.get(&instance) {
            return id;
        }
        let id = self.reserve(qualified(&instance));
        self.ids.insert(instance.clone(), id);
        self.queue.push((instance, id, origin, self.frame.clone()));
        id
    }

    /// The name of the frame of `instance`'s code, which the function
    /// whose frame is `requester` reached first: a function's own name
    /// (a `requires` check and a thunk run as the function they check or
    /// call), a closure's `<closure in f>`, a test body's `test "name"`;
    /// generated code runs in the frame that reached it.
    pub(super) fn frame_of(&self, instance: &Instance, requester: String) -> String {
        match instance {
            Instance::Function {
                source: Source::User,
                name,
                ..
            }
            | Instance::Requires { name, .. }
            | Instance::Thunk { name, .. } => name.clone(),
            Instance::Closure(_) => Frame::closure_in(&requester),
            Instance::Test(index) => Frame::test(&self.user.tests[*index].name),
            _ => requester,
        }
    }

    /// `instance`, or under `--max-memory` for one of `std.rt`'s allocator
    /// functions, its counting one ([`COUNTED_MEMORY`]): every call of the
    /// allocator, in the generated code and in `std.rt` alike, and every
    /// function value's drop by `free`, counts. Without the limit nothing
    /// changes.
    pub(super) fn counted(&self, instance: Instance) -> Instance {
        if self.limits.max_memory.is_none() || self.uncounted {
            return instance;
        }
        match instance {
            Instance::Function {
                source: Source::Runtime,
                name,
                type_args,
            } => {
                let name = match COUNTED_MEMORY.iter().find(|(raw, _)| *raw == name) {
                    Some((_, counted)) => counted.to_string(),
                    None => name,
                };
                Instance::Function {
                    source: Source::Runtime,
                    name,
                    type_args,
                }
            }
            other => other,
        }
    }

    /// The index of `code` in [`Lowering::closures`], added if it is new.
    pub(super) fn closure_index(&mut self, code: closures::ClosureCode) -> usize {
        let key = (
            std::sync::Arc::as_ptr(&code.code) as usize,
            code.source,
            code.subst.clone(),
        );
        if let Some(&index) = self.closure_ids.get(&key) {
            return index;
        }
        self.closures.push(code);
        self.closure_ids.insert(key, self.closures.len() - 1);
        self.closures.len() - 1
    }

    /// A placeholder for the function `name`, lowered later: its id.
    pub(super) fn reserve(&mut self, name: String) -> FnId {
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

    /// `_main`, which `_start` runs on the program's stack.
    fn entry_main(&mut self, run: FnId) -> Result<(), Diagnostic> {
        let user = self.user;
        let kind = self.kind;
        let mut body = Body::new(self, Source::User, None);
        match kind {
            Entry::Main => {
                body.start_steps();
                if let Some(main) = user.functions.get("main") {
                    body.call_main(main)?;
                }
            }
            Entry::Tests => body.call_test()?,
        }
        let zero = body.constant(0);
        body.call_runtime("exit", vec![zero], NOWHERE)?;
        body.push(Inst::Return(zero));
        self.functions[run] = body.finish("_main", 0, NOWHERE)?;
        Ok(())
    }

    /// On Windows, the vectored exception handler's entry, which Windows
    /// calls with `EXCEPTION_POINTERS *` and which returns to it (ADR
    /// 0020): `std.rt`'s `on_exception`. `None` elsewhere.
    fn exception_callback(&mut self, start_kind: Start) -> Result<Option<FnId>, Diagnostic> {
        if start_kind != Start::Windows {
            return Ok(None);
        }
        let callback = self.reserve("_on_exception".to_string());
        let mut body = Body::new(self, Source::User, None);
        let pointers = body.temp();
        let result = body.call_runtime("on_exception", vec![pointers], NOWHERE)?;
        body.push(Inst::Return(result));
        self.functions[callback] = body.finish("_on_exception", 1, NOWHERE)?;
        Ok(Some(callback))
    }

    /// The entry point, `_start`: map the stack and run `_main` on it,
    /// which calls `main` with its capabilities, reports an `Err` it
    /// returns, and exits. The stack's size is a placeholder until every
    /// function is lowered ([`Lowering::size_stack`]).
    pub(super) fn entry(&mut self) -> Result<FnId, Diagnostic> {
        let start = self.reserve("_start".to_string());
        let run = self.reserve("_main".to_string());

        self.entry_main(run)?;

        let start_kind = self.target.platform().start;
        let callback = self.exception_callback(start_kind)?;
        self.callback = callback;
        let mut body = Body::new(self, Source::User, None);
        // On macOS, `_start` is called as `main(argc, argv)`: its
        // parameters.
        let params = match start_kind {
            Start::Stack | Start::Windows => 0,
            Start::Main => {
                body.temp();
                body.temp();
                2
            }
        };
        let size = body.constant(0);
        // `std.rt`'s fault handler, which the kernel calls with the
        // signal's number as its one argument, as the backend's own
        // calling convention passes it; on Windows, the entry above.
        let handler = match callback {
            Some(callback) => body.function_address(callback),
            None => {
                let on_fault = body.lowering.request(
                    Instance::Function {
                        source: Source::Runtime,
                        name: "on_fault".to_string(),
                        type_args: Vec::new(),
                    },
                    NOWHERE,
                );
                body.function_address(on_fault)
            }
        };
        let (argc, argv) = match start_kind {
            // The kernel's initial stack holds argc, then the argv
            // pointers.
            Start::Stack => {
                let sp = body.temp();
                body.push(Inst::EntrySp { dst: sp });
                let argc = body.load(sp, 0);
                let eight = body.constant(i64::from(layout::WORD));
                (argc, body.binary_op(BinOp::Add, sp, eight))
            }
            Start::Main => (0, 1),
            // `std.rt` reads the command line itself.
            Start::Windows => {
                let zero = body.constant(0);
                (zero, zero)
            }
        };
        body.call_runtime("process_init", vec![argc, argv, handler], NOWHERE)?;
        // The call stack, and how a report's frames end.
        let human = body.lowering.human;
        let depth = body.constant(i64::from(MAX_CALL_DEPTH));
        let (end, before, after) = if human {
            let before = oska_syntax::OMITTED_BEFORE;
            (
                "",
                before.to_string(),
                oska_syntax::OMITTED_AFTER.to_string(),
            )
        } else {
            ("\n", "\x1f+".to_string(), String::new())
        };
        let end = body.text(end);
        let before = body.text(&before);
        let after = body.text(&after);
        let most = body.constant(MAX_FRAMES as i64 - 1);
        body.call_runtime(
            "frames_init",
            vec![depth, end, before, after, most],
            NOWHERE,
        )?;
        // On Windows the program runs on the thread's own stack, which
        // the executable's header reserves.
        let stack = if start_kind == Start::Windows {
            body.constant(0)
        } else {
            body.call_runtime("stack_new", vec![size], NOWHERE)?
        };
        body.push(Inst::CallOnStack {
            stack,
            depth: MAX_CALL_DEPTH,
            callee: run,
        });
        body.push(Inst::Return(size));
        self.functions[start] = body.finish("_start", params, NOWHERE)?;
        Ok(start)
    }

    /// Fill in the size of the stack `_start` maps: [`MAX_CALL_DEPTH`]
    /// levels of the program's largest frame and two of the largest frame
    /// that may run between two counted calls (each frame its temporaries, the
    /// callee-saved registers and the frame record), and
    /// [`STACK_RESERVE`]. A task recursing through `join` stacks a queue's
    /// and a runner's frame for every two counted calls, the function's
    /// and the closure's.
    pub(super) fn size_stack(&mut self, start: FnId) -> u64 {
        let largest = |ids: &[FnId]| {
            ids.iter()
                .filter_map(|&id| self.functions.get(id))
                .map(|f| 8 * u64::from(f.temps) + 8 * 9 + 32)
                .max()
                .unwrap_or(0)
        };
        // Up to two of the generated frames per level: a thunk, then the
        // `requires` check of the function it calls.
        let level = largest(&self.user_functions) + 2 * largest(&self.between_functions);
        let size = (u64::from(MAX_CALL_DEPTH) * level + STACK_RESERVE).next_multiple_of(1 << 16);
        if let Some(Inst::Const { value, .. }) = self.functions[start].insts.first_mut() {
            *value = size as i64;
        }
        size
    }

    /// Lower an instance that is not a function of the program: the
    /// drop, debug, equality, closure, test and task functions the backend
    /// generates.
    fn generated_function(
        &mut self,
        instance: &Instance,
        origin: Span,
    ) -> Result<ir::Function, Diagnostic> {
        match instance {
            Instance::Drop(source, ty) => {
                let mut body = Body::new(self, *source, Some(origin));
                body.drop_function(ty)?;
                body.finish(&qualified(instance), 1, NOWHERE)
            }
            Instance::Debug(source, ty) => {
                let mut body = Body::new(self, *source, Some(origin));
                body.debug_function(ty)?;
                body.finish(&qualified(instance), 1, NOWHERE)
            }
            Instance::Unshare(source, ty) => {
                let mut body = Body::new(self, *source, Some(origin));
                body.unshare_function(ty)?;
                body.finish(&qualified(instance), 1, NOWHERE)
            }
            Instance::ListEq(source, ty, at) => {
                let mut body = Body::new(self, *source, Some(origin));
                body.list_eq_function(ty, at.unwrap_or(NOWHERE))?;
                body.finish(&qualified(instance), 2, NOWHERE)
            }
            Instance::Equal(source, ty, at) => {
                let runtime_origin = (*source == Source::Runtime).then_some(origin);
                let mut body = Body::new(self, *source, runtime_origin);
                body.equal_function(ty, at.unwrap_or(origin))?;
                body.finish(&qualified(instance), 2, NOWHERE)
            }
            Instance::Hash(source, ty, at) => {
                let runtime_origin = (*source == Source::Runtime).then_some(origin);
                let mut body = Body::new(self, *source, runtime_origin);
                body.hash_function(ty, at.unwrap_or(origin))?;
                body.finish(&qualified(instance), 1, NOWHERE)
            }
            Instance::Compare(source, ty, at) => {
                let runtime_origin = (*source == Source::Runtime).then_some(origin);
                let mut body = Body::new(self, *source, runtime_origin);
                body.compare_function(ty, at.unwrap_or(origin))?;
                body.finish(&qualified(instance), 2, NOWHERE)
            }
            Instance::Closure(index) => {
                let source = self.closures[*index].source;
                let runtime_origin = (source == Source::Runtime).then_some(origin);
                let mut body = Body::new(self, source, runtime_origin);
                let (params, span) = body.closure_function(*index)?;
                body.finish(&qualified(instance), params, span)
            }
            Instance::ClosureDrop(index) => {
                let source = self.closures[*index].source;
                let runtime_origin = (source == Source::Runtime).then_some(origin);
                let mut body = Body::new(self, source, runtime_origin);
                body.closure_drop_function(*index)?;
                body.finish(&qualified(instance), 1, NOWHERE)
            }
            Instance::Test(index) => {
                let mut body = Body::new(self, Source::User, None);
                let span = body.test_function(*index)?;
                body.finish(&qualified(instance), 1, span)
            }
            Instance::TaskRunner { result, start, end } => {
                let mut body = Body::new(self, Source::User, None);
                let at = Span {
                    start: *start,
                    end: *end,
                };
                body.task_runner(result, at)?;
                body.finish(&qualified(instance), 1, NOWHERE)
            }
            Instance::RunTasks => {
                let mut body = Body::new(self, Source::User, None);
                body.run_tasks_function()?;
                body.finish(&qualified(instance), 2, NOWHERE)
            }
            Instance::Thunk {
                source,
                intrinsic,
                name,
                type_args,
                ty,
            } => {
                let runtime_origin = (*source == Source::Runtime).then_some(origin);
                let mut body = Body::new(self, *source, runtime_origin);
                let params = body.thunk_function(*intrinsic, name, type_args, ty, origin)?;
                body.finish(&qualified(instance), params, NOWHERE)
            }
            Instance::Function { .. } | Instance::Requires { .. } => {
                unreachable!("a function of the program is lowered by `function`")
            }
        }
    }

    /// Lower the function `instance` names.
    pub(super) fn function(
        &mut self,
        instance: &Instance,
        origin: Span,
    ) -> Result<ir::Function, Diagnostic> {
        let (source, name, type_args) = match instance {
            Instance::Function {
                source,
                name,
                type_args,
            }
            | Instance::Requires {
                source,
                name,
                type_args,
            } => (*source, name, type_args),
            _ => return self.generated_function(instance, origin),
        };
        let program = self.program(source);
        let runtime_origin = (source == Source::Runtime).then_some(origin);
        let Some(function) = program.functions.get(name) else {
            return Err(refusal(
                origin,
                &format!("a call of `{name}`"),
                runtime_origin.is_some(),
            ));
        };
        let at = |span: Span| runtime_origin.unwrap_or(span);
        if function.generics.len() != type_args.len() {
            return Err(refusal(
                at(function.body.span),
                &format!(
                    "the generic function `{}` without its type arguments",
                    function.name
                ),
                runtime_origin.is_some(),
            ));
        }
        if function.params.len() + usize::from(function.receiver) > ir::MAX_ARGS {
            return Err(refusal(
                at(function.body.span),
                &format!("a function with more than {} parameters", ir::MAX_ARGS),
                runtime_origin.is_some(),
            ));
        }
        let checks_requires = matches!(instance, Instance::Requires { .. });
        let mut body = Body::new(self, source, runtime_origin);
        body.ensuring =
            (!checks_requires && !function.contracts.ensures.is_empty()).then_some(function);
        body.subst = function
            .generics
            .iter()
            .cloned()
            .zip(type_args.clone())
            .collect();
        body.ret = body.ty(&function.ret);
        if function.receiver {
            let receiver = body.temp();
            let ty = function
                .self_ty
                .as_ref()
                .map_or(Type::Unit, |ty| body.ty(ty));
            body.receiver = Some(Receiver {
                temp: receiver,
                // A mutating method owns its receiver, and gives it back.
                owned: data::writes_self(function) && !checks_requires,
                ty,
            });
        }
        for param in &function.params {
            let temp = body.temp();
            body.param_temps.push(temp);
            if param.binds {
                // Borrowed from the caller: never released here.
                let ty = body.ty(&param.ty);
                body.slots.push(Slot {
                    temp,
                    release: false,
                    ty,
                });
            }
        }
        let params = body.temps;
        if checks_requires {
            body.requires_function(function)?;
        } else {
            let value = body.expr(&function.body)?;
            body.return_value(value, body_end(&function.body))?;
        }
        body.finish(&qualified(instance), params, function.body.span)
    }
}
