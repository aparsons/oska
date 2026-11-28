// SPDX-License-Identifier: Apache-2.0
//! Runtime checks, library calls, `main` and `display`.

use super::*;

impl Body<'_, '_> {
    /// Panic after a [`Prelude::Checked`] call of `args` that gave `result`:
    /// the word 0 (and, for `Repeat`, -1) says the call was a bug.
    pub(super) fn check(
        &mut self,
        check: Check,
        result: Temp,
        args: &[Temp],
        span: Span,
    ) -> Result<(), Diagnostic> {
        match check {
            Check::Slice => {
                let label =
                    self.panic_text(panics::SLICE_OUT_OF_BOUNDS, "panic_slice", args, span)?;
                self.push(Inst::JumpIfZero {
                    cond: result,
                    target: label,
                });
            }
            Check::Secret => {
                // The report quotes the name, which the program made:
                // under `--max-output`, no more of it than the limit has
                // left (`std.rt`'s `cut_quoted`).
                let label = match self.lowering.limits.max_output {
                    None => {
                        self.panic_text(panics::MISSING_SECRET, "panic_missing_secret", args, span)?
                    }
                    Some(limit) => {
                        let mut with_limit = args.to_vec();
                        with_limit.push(self.limit_constant(limit));
                        self.panic_text(
                            panics::MISSING_SECRET,
                            "panic_missing_secret_cut",
                            &with_limit,
                            span,
                        )?
                    }
                };
                self.push(Inst::JumpIfZero {
                    cond: result,
                    target: label,
                });
            }
            Check::Repeat => {
                let negative = self.panic_text(
                    panics::NEGATIVE_REPEAT,
                    "panic_repeat_negative",
                    &args[1..],
                    span,
                )?;
                self.push(Inst::JumpIfZero {
                    cond: result,
                    target: negative,
                });
                let long =
                    self.panic_text(panics::TEXT_TOO_LONG, "panic_repeat_long", args, span)?;
                let zero = self.constant(0);
                self.push(Inst::Branch {
                    cond: Cond::Lt,
                    lhs: result,
                    rhs: zero,
                    target: long,
                });
            }
            Check::Zeroed | Check::SecureBytes => {
                // `Random.secure_bytes` passes the capability first.
                let args = if matches!(check, Check::SecureBytes) {
                    &args[1..]
                } else {
                    args
                };
                let negative = self.panic_text(
                    panics::BYTES_OUT_OF_RANGE,
                    "panic_bytes_negative",
                    args,
                    span,
                )?;
                self.push(Inst::JumpIfZero {
                    cond: result,
                    target: negative,
                });
                let long =
                    self.panic_text(panics::BYTES_TOO_LONG, "panic_bytes_long", args, span)?;
                let zero = self.constant(0);
                self.push(Inst::Branch {
                    cond: Cond::Lt,
                    lhs: result,
                    rhs: zero,
                    target: long,
                });
            }
            Check::BytesSelect => {
                let label =
                    self.panic_text(panics::BYTES_OUT_OF_RANGE, "panic_bytes_select", args, span)?;
                self.push(Inst::JumpIfZero {
                    cond: result,
                    target: label,
                });
            }
            Check::BytesSlice | Check::BytesIndex | Check::BytesAppend => {
                // `set`'s arguments are the bytes, the index and the byte:
                // its report names only the first two.
                let (code, function, args) = match check {
                    Check::BytesSlice => (panics::BYTES_OUT_OF_RANGE, "panic_bytes_slice", args),
                    Check::BytesIndex => {
                        (panics::INDEX_OUT_OF_BOUNDS, "panic_bytes_index", &args[..2])
                    }
                    _ => (panics::BYTES_TOO_LONG, "panic_bytes_append", args),
                };
                let label = self.panic_text(code, function, args, span)?;
                self.push(Inst::JumpIfZero {
                    cond: result,
                    target: label,
                });
            }
            Check::BytesGrow(width) => {
                let extra = self.constant(i64::from(width));
                let label = self.panic_text(
                    panics::BYTES_TOO_LONG,
                    "panic_bytes_grow",
                    &[args[0], extra],
                    span,
                )?;
                self.push(Inst::JumpIfZero {
                    cond: result,
                    target: label,
                });
            }
        }
        Ok(())
    }

    /// `Bytes.at` (`width` 1) or an endian read of `width` bytes of the
    /// buffer `bytes` at `offset`, most significant byte first when `big`:
    /// a panic, `P0002` for `at` and `P0042` for a read, unless the bytes
    /// all lie inside it.
    pub(super) fn byte_read(
        &mut self,
        bytes: Temp,
        offset: Temp,
        width: u8,
        big: bool,
        secret: bool,
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        let index = width == 1;
        let width = self.constant(i64::from(width));
        let readable = self.call_runtime("bytes_readable", vec![bytes, offset, width], NOWHERE)?;
        let label = if index {
            self.panic_text(
                panics::INDEX_OUT_OF_BOUNDS,
                "panic_bytes_index",
                &[bytes, offset],
                span,
            )?
        } else {
            self.panic_text(
                panics::BYTES_OUT_OF_RANGE,
                "panic_bytes_read",
                &[bytes, offset, width],
                span,
            )?
        };
        self.push(Inst::JumpIfZero {
            cond: readable,
            target: label,
        });
        let big = self.constant(i64::from(big));
        let read = if secret {
            "bytes_read_secret"
        } else {
            "bytes_read"
        };
        self.call_runtime(read, vec![bytes, offset, width, big], NOWHERE)
    }

    /// A call of the platform library's function `import` with the
    /// intrinsic's `args`, whose result is `std.rt`'s: the function's,
    /// or `-errno` when it failed (ADR 0015). Nothing but the call and
    /// the read of `errno` is added around it.
    pub(super) fn library_call(&mut self, import: &Import, args: &[Temp]) -> Temp {
        let platform = self.lowering.target.platform();
        let slot = platform.slot(import).unwrap_or_default();
        let mut args: Vec<Temp> = args.iter().copied().take(import.args.into()).collect();
        for &value in import.extra_args {
            args.push(self.constant(value));
        }
        // `errno`'s address, for a call that clears it first.
        let errno = match import.fails {
            Fails::NullKeepsErrno => {
                let errno = self.errno_address();
                let zero = self.constant(0);
                self.push(Inst::Store {
                    width: Width::W32,
                    base: errno,
                    offset: 0,
                    src: zero,
                });
                Some(errno)
            }
            _ => None,
        };
        let dst = self.temp();
        self.push(Inst::LibCall {
            dst,
            import: slot,
            args,
            fixed: import.fixed,
            wide: import.wide,
        });
        let failed = match import.fails {
            Fails::MinusOne => -1,
            Fails::Null | Fails::NullKeepsErrno | Fails::Zero => 0,
            Fails::Status => return self.status_error(dst),
            Fails::Never => return dst,
        };
        let done = self.label();
        let failure = self.constant(failed);
        self.push(Inst::Branch {
            cond: Cond::Ne,
            lhs: dst,
            rhs: failure,
            target: done,
        });
        let errno = match errno {
            Some(errno) => errno,
            None => self.errno_address(),
        };
        let code = match platform.error {
            ErrorRead::Value => errno,
            ErrorRead::Address => {
                let code = self.temp();
                self.push(Inst::Load {
                    width: Width::W32,
                    dst: code,
                    base: errno,
                    offset: 0,
                });
                code
            }
        };
        let zero = self.constant(0);
        self.push(Inst::Binary {
            op: BinOp::Sub,
            dst,
            lhs: zero,
            rhs: code,
        });
        self.push(Inst::Label(done));
        dst
    }

    /// `status`, an `ntdll` function's `NTSTATUS`, as `std.rt` takes a
    /// result: as it is unless negative, else `-RtlNtStatusToDosError`
    /// of it (ADR 0020).
    pub(super) fn status_error(&mut self, status: Temp) -> Temp {
        let platform = self.lowering.target.platform();
        let slot = platform
            .slot(&intrinsics::NT_STATUS_TO_ERROR)
            .unwrap_or_default();
        let done = self.label();
        let zero = self.constant(0);
        self.push(Inst::Branch {
            cond: Cond::Ge,
            lhs: status,
            rhs: zero,
            target: done,
        });
        let code = self.temp();
        self.push(Inst::LibCall {
            dst: code,
            import: slot,
            args: vec![status],
            fixed: 1,
            wide: false,
        });
        let zero = self.constant(0);
        self.push(Inst::Binary {
            op: BinOp::Sub,
            dst: status,
            lhs: zero,
            rhs: code,
        });
        self.push(Inst::Label(done));
        status
    }

    /// The address of the thread's `errno`, or on Windows its last
    /// error itself ([`ErrorRead`]), from the platform library.
    pub(super) fn errno_address(&mut self) -> Temp {
        let platform = self.lowering.target.platform();
        let import = platform.imports[usize::from(platform.errno)];
        let dst = self.temp();
        self.push(Inst::LibCall {
            dst,
            import: platform
                .slot_of(usize::from(platform.errno))
                .unwrap_or_default(),
            args: Vec::new(),
            fixed: 0,
            wide: import.wide,
        });
        dst
    }

    /// A `std.rt` intrinsic applied to `args`. The checker has matched
    /// their number to its declaration.
    pub(super) fn runtime_intrinsic(
        &mut self,
        runtime: Runtime,
        args: &[Temp],
    ) -> Result<Temp, Diagnostic> {
        let arg = |i: usize| args.get(i).copied().unwrap_or_default();
        Ok(match runtime {
            Runtime::Syscall(call) => match (self.lowering.target.platform().call)(call) {
                OsCall::Trap { number, extra_args } => {
                    let mut args = args.to_vec();
                    for &value in extra_args {
                        args.push(self.constant(value));
                    }
                    let dst = self.temp();
                    self.push(Inst::Syscall { dst, number, args });
                    dst
                }
                OsCall::Library(import) => self.library_call(import, args),
                OsCall::Same => self.copy(arg(0)),
                OsCall::Absent => self.constant(self.lowering.target.platform().absent),
            },
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
            Runtime::Binary(op) => self.binary_op(op, arg(0), arg(1)),
            // A copy: the argument's reference, if it holds one, is
            // released after the call, and the word stays.
            Runtime::Identity => self.copy(arg(0)),
            Runtime::Conceal => self.mark_secret(arg(0)),
            Runtime::Expose => {
                let dst = self.temp();
                self.push(Inst::Expose { dst, src: arg(0) });
                dst
            }
            Runtime::Globals => {
                let dst = self.temp();
                self.push(Inst::Addr {
                    dst,
                    symbol: Symbol::Data(0),
                });
                dst
            }
            Runtime::Depth => {
                let dst = self.temp();
                self.push(Inst::Depth { dst });
                dst
            }
            Runtime::TestHost => self.test_host(),
            Runtime::DnsTestHooks => self.constant(i64::from(self.lowering.dns_test_hooks)),
            // A call of the target platform's table, a static object.
            Runtime::PlatformTable => {
                let table = self.lowering.target.platform().table;
                self.call_runtime(table, Vec::new(), NOWHERE)?
            }
        })
    }

    /// Call `main` with its capabilities and, when it returns `Err(e)`,
    /// hand `e`'s text to `std.rt`'s `main_failed`, which exits. As in
    /// the interpreter, the call of `main` is not counted.
    pub(super) fn call_main(&mut self, main: &Function) -> Result<(), Diagnostic> {
        let span = main.body.span;
        // Under `--max-memory`, the limit and its report, which spans
        // `main`'s body: the runtime cannot tell which code allocated.
        if let Some(limit) = self.lowering.limits.max_memory {
            let (before, after) = self.report(Limit::Memory.code(), span);
            let report = self.text(&format!("{before}{}{after}", Limit::Memory.message(limit)));
            let limit = self.limit_constant(limit);
            self.call_runtime("memory_limit_start", vec![limit, report], NOWHERE)?;
        }
        let mut args = Vec::new();
        for param in &main.params {
            if !is_stateless_input(&param.ty) {
                return Err(self.refuse(span, &format!("a `main` given `{}`", param.ty)));
            }
            args.push(self.constant(0));
        }
        let result = self.call_function(Source::User, "main", Vec::new(), args, span)?;
        match &main.ret {
            Type::Unit => Ok(()),
            Type::Named { name, args } if name == builtins::RESULT && args.len() == 2 => {
                let err = self.variant(builtins::RESULT, builtins::ERR, span)?;
                let tag = self.load(result, layout::ENUM_TAG);
                let err = self.constant(err as i64);
                let ok = self.label();
                self.push(Inst::Branch {
                    cond: Cond::Ne,
                    lhs: tag,
                    rhs: err,
                    target: ok,
                });
                let error = self.load(result, layout::enum_field(0));
                let text = self.display(&args[1], error, span)?;
                // The text and a newline, which the program wrote.
                self.spend_output(text, 1, span)?;
                // `main_failed` exits, so the text is never released.
                self.call_runtime("main_failed", vec![text], span)?;
                self.push(Inst::Label(ok));
                Ok(())
            }
            other => Err(self.refuse(span, &format!("a `main` that returns `{other}`"))),
        }
    }

    /// The `Display` text of `value`, of the concrete type `ty`: through
    /// the program's impl, generic or not, or the prelude's.
    pub(super) fn display(
        &mut self,
        ty: &Type,
        value: Temp,
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        let tag = match ty {
            Type::String => return Ok(value),
            Type::Int(int) => int.name(),
            Type::Bool => "Bool",
            Type::Named { name, .. } => name,
            _ => return Err(self.refuse(span, &format!("the `Display` text of a `{ty}`"))),
        };
        let user = trait_method(tag, "Display", "to_string");
        let (callee, type_args) = match self.program().functions.get(&user) {
            Some(function) => {
                let mut bound = Vec::new();
                if let Some(self_ty) = &function.self_ty {
                    self_ty.bind(ty, &mut bound);
                }
                let type_args = function
                    .generics
                    .iter()
                    .map(|g| {
                        bound
                            .iter()
                            .find(|(name, _)| name == g)
                            .map_or(Type::Unit, |(_, t)| t.clone())
                    })
                    .collect();
                (Callee::Function(user), type_args)
            }
            None => (Callee::intrinsic(format!("{tag}.to_string")), Vec::new()),
        };
        self.call(&callee, &type_args, vec![value], &Type::String, span)
    }

    /// The index of `variant` in the enum `ty`.
    pub(super) fn variant(&self, ty: &str, variant: &str, span: Span) -> Result<usize, Diagnostic> {
        self.program()
            .types
            .get(ty)
            .and_then(|decl| layout::variant_index(decl, variant))
            .ok_or_else(|| self.refuse(span, &format!("the variant `{ty}.{variant}`")))
    }

    // ---- Expressions -------------------------------------------------------
}
