// SPDX-License-Identifier: Apache-2.0
//! Calls: functions, runtime functions and function values.

use super::*;

impl Body<'_, '_> {
    /// Call the function `id` with `args`.
    pub(super) fn call_id(
        &mut self,
        callee: FnId,
        args: Vec<Temp>,
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        if args.len() > ir::MAX_ARGS {
            let what = format!("a call with more than {} arguments", ir::MAX_ARGS);
            return Err(self.refuse(span, &what));
        }
        let dst = self.temp();
        self.push(Inst::Call { dst, callee, args });
        Ok(dst)
    }

    /// Call the function `name` of `source`, instantiated with the
    /// concrete `type_args`.
    pub(super) fn call_function(
        &mut self,
        source: Source,
        name: &str,
        type_args: Vec<Type>,
        args: Vec<Temp>,
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        let origin = self.origin.unwrap_or(span);
        let instance = Instance::Function {
            source,
            name: name.to_string(),
            type_args,
        };
        let id = self.lowering.request(instance, origin);
        self.call_id(id, args, span)
    }

    /// Call the `std.rt` function `name`.
    pub(super) fn call_runtime(
        &mut self,
        name: &str,
        args: Vec<Temp>,
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        // A call with the wrong number of arguments would leave a
        // parameter unset, or pass one that is never read: refused, so a
        // slip in the lowering never compiles.
        if let Some(function) = self.lowering.rt.functions.get(name) {
            if function.params.len() != args.len() {
                return Err(self.refuse(
                    span,
                    &format!(
                        "a call of `std.rt`'s `{name}` with {} arguments, not {}",
                        args.len(),
                        function.params.len()
                    ),
                ));
            }
        }
        self.call_function(Source::Runtime, name, Vec::new(), args, span)
    }

    /// Call the function `name` of the program being lowered with the
    /// concrete `type_args`. A call of a user function from user code
    /// counts its depth: at [`MAX_CALL_DEPTH`] it panics with `P0021` at
    /// `span` instead.
    pub(super) fn call_program(
        &mut self,
        name: &str,
        type_args: Vec<Type>,
        args: Vec<Temp>,
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        if self.source == Source::Runtime {
            return self.call_function(Source::Runtime, name, type_args, args, span);
        }
        self.step(span)?;
        let exhausted = self.panic_static(panics::CALL_DEPTH_EXCEEDED, panics::TOO_DEEP, span)?;
        self.push(Inst::Enter { exhausted });
        self.record_call(span);
        self.check_requires(name, &type_args, &args, span)?;
        let result = self.call_function(Source::User, name, type_args, args, span)?;
        self.push(Inst::Leave);
        Ok(result)
    }

    /// The second result of the call just made: a mutating method's
    /// receiver as it left it.
    pub(super) fn second_result(&mut self) -> Temp {
        let dst = self.temp();
        self.push(Inst::Second { dst });
        dst
    }

    /// A call of `callee`, instantiated with `type_args` (not yet
    /// substituted), with `args`, evaluated already, at `span`, whose
    /// result is of type `ret`. The call releases the arguments that hold
    /// a reference of their own, and its result holds one when `ret` is
    /// counted.
    pub(super) fn call(
        &mut self,
        callee: &Callee,
        type_args: &[Type],
        args: Vec<Temp>,
        ret: &Type,
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        let result = match callee {
            Callee::Function(name) if self.program().functions.contains_key(name) => {
                self.program_call(name, type_args, &args, span)?
            }
            Callee::Function(name) => match intrinsics::runtime(name) {
                // Only `std.rt` may name an intrinsic (the checker's
                // `E0209`), and the backend holds it to that too.
                Some(runtime) if self.source == Source::Runtime => {
                    self.runtime_intrinsic(runtime, &args)?
                }
                _ => return Err(self.refuse(span, &format!("a call of `{name}`"))),
            },
            // `dbg(value)` writes the value's text to stderr and returns
            // the value: its reference passes on.
            Callee::Intrinsic(name) if name == "dbg" => {
                return self.dbg_call(&args, type_args, span);
            }
            Callee::Intrinsic(name) => match intrinsics::prelude(name) {
                // The argument is the result: its reference passes on.
                Some(Prelude::Identity) => match args.first() {
                    Some(&value) => return Ok(value),
                    None => return Err(self.refuse(span, &format!("`{name}`"))),
                },
                Some(prelude) => self.prelude_call(name, prelude, &args, ret, span)?,
                // Called on a value, not a place: `mutating_call` handles
                // a place, and only a place can be `var`.
                None => return Err(self.refuse(span, &format!("`{name}`"))),
            },
            Callee::Trait { trait_name, method } => {
                return Err(self.refuse(
                    span,
                    &format!("a call of `{trait_name}.{method}` on a value of a type parameter"),
                ))
            }
        };
        for arg in args {
            self.consume(arg)?;
        }
        self.own(result, ret);
        Ok(result)
    }

    /// A call of a function of the program, with the receiver a mutating
    /// method gave back released when the call was on a value.
    fn program_call(
        &mut self,
        name: &str,
        type_args: &[Type],
        args: &[Temp],
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        let type_args: Vec<Type> = type_args.iter().map(|t| self.ty(t)).collect();
        let mutated = self.mutated_receiver(name, &type_args);
        let result = self.call_program(name, type_args, args.to_vec(), span)?;
        if let (Some(ty), Some(&receiver)) = (mutated, args.first()) {
            // A mutating method called on a value, not a place:
            // it took the receiver's reference and gives back the
            // receiver it changed, which is released here.
            let after = self.second_result();
            self.owned.remove(&receiver);
            if self.counted(&ty) {
                self.owned.insert(after, ty);
            }
            self.consume(after)?;
        }
        Ok(result)
    }

    /// `dbg(value)` writes the value's text to stderr and returns the
    /// value: its reference passes on.
    fn dbg_call(
        &mut self,
        args: &[Temp],
        type_args: &[Type],
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        let (Some(&value), Some(ty)) = (args.first(), type_args.first()) else {
            return Err(self.refuse(span, "`dbg`"));
        };
        let ty = self.ty(ty);
        let text = self.debug_text(&ty, value, span)?;
        // `[dbg] `, the text and a newline.
        self.spend_output(text, 7, span)?;
        self.call_runtime("dbg_write", vec![text], span)?;
        self.release(text, &Type::String)?;
        Ok(value)
    }

    /// A call of a prelude intrinsic the runtime supplies, other than the
    /// identity one: the result, not yet owned.
    fn prelude_call(
        &mut self,
        name: &str,
        prelude: Prelude,
        args: &[Temp],
        ret: &Type,
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        Ok(match prelude {
            Prelude::Runtime(function) => self.call_runtime(function, args.to_vec(), span)?,
            Prelude::WriteLine(function) => {
                let Some(&line) = args.get(1) else {
                    return Err(self.refuse(span, &format!("`{name}`")));
                };
                // The line and a newline.
                self.spend_output(line, 1, span)?;
                self.call_runtime(function, args.to_vec(), span)?
            }
            Prelude::Confirm => {
                let &[_, summary, details] = args else {
                    return Err(self.refuse(span, &format!("`{name}`")));
                };
                let question =
                    self.call_runtime("approval_prompt", vec![summary, details], span)?;
                self.spend_output(question, 0, span)?;
                let mut with_question = args.to_vec();
                with_question.push(question);
                let result = self.call_runtime("approver_confirm", with_question, span)?;
                self.release(question, &Type::String)?;
                result
            }
            Prelude::Field(index) => match args.first() {
                Some(&value) => {
                    let field = self.load(value, layout::struct_field(index));
                    if self.counted(ret) {
                        self.retain(field);
                    }
                    field
                }
                None => return Err(self.refuse(span, &format!("`{name}`"))),
            },
            Prelude::Checked(function, check) => {
                let result = self.call_runtime(function, args.to_vec(), span)?;
                self.check(check, result, args, span)?;
                result
            }
            Prelude::ByteRead { width, big, secret } => {
                let &[bytes, offset] = args else {
                    return Err(self.refuse(span, &format!("`{name}`")));
                };
                self.byte_read(bytes, offset, width, big, secret, span)?
            }
            Prelude::Identity | Prelude::Mutating(..) => {
                return Err(self.refuse(span, &format!("`{name}`")))
            }
        })
    }

    /// For a mutating method of the program, `name` (one that may change
    /// its receiver, `data::writes_self`), instantiated with the concrete
    /// `type_args`: its receiver's type. `None` for any other function.
    pub(super) fn mutated_receiver(&self, name: &str, type_args: &[Type]) -> Option<Type> {
        let function = self.program().functions.get(name)?;
        if !data::writes_self(function) {
            return None;
        }
        let subst: Vec<(String, Type)> = function
            .generics
            .iter()
            .cloned()
            .zip(type_args.iter().cloned())
            .collect();
        Some(function.self_ty.as_ref()?.substitute(&subst))
    }

    /// Under `--max-output`, count the bytes of the text `text` and
    /// `extra` more, which the program is about to write to stdout or
    /// stderr, against the limit (`std.rt`'s `output_allows`): a write
    /// that would pass it panics with `P0041` at `span` instead. Without
    /// the limit nothing is generated.
    pub(super) fn spend_output(
        &mut self,
        text: Temp,
        extra: i64,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let Some(limit) = self.lowering.limits.max_output else {
            return Ok(());
        };
        let over = self.panic_static(Limit::Output.code(), &Limit::Output.message(limit), span)?;
        let extra = self.constant(extra);
        let limit = self.limit_constant(limit);
        let allowed = self.call_runtime("output_allows", vec![text, extra, limit], span)?;
        self.push(Inst::JumpIfZero {
            cond: allowed,
            target: over,
        });
        Ok(())
    }
}
