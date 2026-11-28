// SPDX-License-Identifier: Apache-2.0
//! Panic reports and the cold blocks that raise them.

use super::*;

impl Body<'_, '_> {
    /// A cold block that `build` fills: its label.
    pub(super) fn cold_block(
        &mut self,
        build: impl FnOnce(&mut Self) -> Result<(), Diagnostic>,
    ) -> Result<Label, Diagnostic> {
        let label = self.label();
        let hot = std::mem::replace(&mut self.insts, vec![Inst::Label(label)]);
        let built = build(self);
        let block = std::mem::replace(&mut self.insts, hot);
        built?;
        self.cold.extend(block);
        Ok(label)
    }

    /// The text before and after the message of a panic with `code` at
    /// `span`, as the interpreter's report prints it:
    /// `path:line:col: error[P....]: ` and ` (bytes a..b)` and a newline.
    /// A panic in `std.rt` has no file to name.
    pub(super) fn report(&self, code: &'static str, span: Span) -> (String, String) {
        let (location, after) = self.report_around(span);
        (format!("{location}error[{code}]: "), after)
    }

    /// The text before a panic's code (`path:line:col: `, or nothing in
    /// `std.rt`) and after its message, for a panic at `span`.
    pub(super) fn report_around(&self, span: Span) -> (String, String) {
        let unlocated = Diagnostic::new("", "", span);
        let located = match self.source {
            Source::User => self.lowering.locate.locate(unlocated),
            Source::Runtime => unlocated,
        };
        let before = match &located.location {
            Some(location) => format!("{location}: "),
            None => String::new(),
        };
        let newline = if self.lowering.human { "\n" } else { "" };
        let after = format!(
            " (bytes {}..{}){newline}{}",
            located.span.start,
            located.span.end,
            self.frame_text(span)
        );
        (before, after)
    }

    /// The frame of the function being lowered at `span`, as a panic's
    /// report lists it: a line, `    at f (path:line:col)`, for a person;
    /// `\x1ff\x1estart..end`, unplaced, for `oskac` to read back.
    pub(super) fn frame_text(&self, span: Span) -> String {
        let frame = Frame::new(self.lowering.frame.clone(), span);
        let frame = match self.source {
            Source::User => self.lowering.locate.locate_frame(frame),
            Source::Runtime => frame,
        };
        if self.lowering.human {
            frame.human()
        } else {
            crate::encode_frame(&frame)
        }
    }

    /// Record the call just counted, made at `span` from the function
    /// being lowered: its frame's text, in the call stack's slot for its
    /// depth (`std.rt`'s "The call stack").
    pub(super) fn record_call(&mut self, span: Span) {
        let text = self.frame_text(span);
        let site = self.text(&text);
        let globals = self.temp();
        self.push(Inst::Addr {
            dst: globals,
            symbol: Symbol::Data(0),
        });
        let stack = self.load(globals, FRAMES_WORD * layout::WORD);
        let depth = self.temp();
        self.push(Inst::Depth { dst: depth });
        let three = self.constant(3);
        let offset = self.binary_op(BinOp::Shl, depth, three);
        let slot = self.binary_op(BinOp::Add, stack, offset);
        self.store(slot, FRAMES_RECORDS, site);
    }

    /// `message`, a panic's at `span`, as the caller's locator words it:
    /// in a package, each item key in it (`pkg.util.half`) names the item
    /// as code in the module at `span` writes it (`util.half`, `half`), as
    /// the interpreter's report does once `oskac` locates it.
    pub(super) fn worded(&self, code: &'static str, message: &str, span: Span) -> String {
        match self.source {
            Source::User => {
                self.lowering
                    .locate
                    .locate(Diagnostic::new(code, message, span))
                    .message
            }
            Source::Runtime => message.to_string(),
        }
    }

    /// In [`Body::deferred`] code, a cold block that gives the caller the
    /// text `status` builds (in the cold block) as the status.
    pub(super) fn defer_panic(
        &mut self,
        status: impl FnOnce(&mut Self) -> Result<Temp, Diagnostic>,
    ) -> Result<Label, Diagnostic> {
        self.cold_block(|body| {
            let text = status(body)?;
            // Give the caller back the count it took, as the code's
            // normal end does, so its own uncounting leaves the call
            // stack as it was.
            if let Some(never) = body.recount {
                body.push(Inst::Enter { exhausted: never });
            }
            let nothing = body.constant(0);
            body.push(Inst::ReturnPair {
                value: nothing,
                second: text,
            });
            Ok(())
        })
    }

    /// A cold block that panics with the status `status` a function value's
    /// code returned, its report's middle, at the call at `span`. In
    /// [`Body::deferred`] code, it passes the status on to its own caller.
    pub(super) fn panic_status(&mut self, status: Temp, span: Span) -> Result<Label, Diagnostic> {
        if self.deferred {
            return self.defer_panic(|_| Ok(status));
        }
        let reworded = self.reworded_preconditions(span);
        let (before, after) = self.report_around(span);
        self.cold_block(|body| {
            // Read the status first: it is a hot temporary.
            let status = body.copy(status);
            let start = body.text(&before);
            let end = body.text(&after);
            for (raw, shown) in &reworded {
                let next = body.label();
                let raw = body.text(raw);
                body.push(Inst::Branch {
                    cond: Cond::Ne,
                    lhs: status,
                    rhs: raw,
                    target: next,
                });
                let shown = body.text(shown);
                let unit = body.call_runtime("panic_message", vec![start, end, shown], NOWHERE)?;
                body.push(Inst::Return(unit));
                body.push(Inst::Label(next));
            }
            let unit = body.call_runtime("panic_message", vec![start, end, status], NOWHERE)?;
            body.push(Inst::Return(unit));
            Ok(())
        })
    }

    /// Every `requires` status a function value's code may return
    /// (`error[P0006]: precondition violated: ...`, naming the function by
    /// its key) that the report at `span` words differently, with that
    /// wording: in a package, the module at `span` names an item as its
    /// code writes it, as the interpreter's located report does. Texts are
    /// shared, so a status is the same object as its text here, and
    /// [`Body::panic_status`] picks the wording by comparing addresses.
    pub(super) fn reworded_preconditions(&self, span: Span) -> Vec<(String, String)> {
        let mut reworded = Vec::new();
        if self.source != Source::User {
            return reworded;
        }
        let code = panics::PRECONDITION_VIOLATED;
        for function in self.program().functions.values() {
            for clause in &function.contracts.requires {
                let message = format!(
                    "precondition violated: `{}` requires `{}`",
                    function.name, clause.text
                );
                let shown = self.worded(code, &message, span);
                if shown != message {
                    reworded.push((
                        format!("error[{code}]: {message}"),
                        format!("error[{code}]: {shown}"),
                    ));
                }
            }
        }
        reworded
    }

    /// A cold block that panics with `code` and the fixed `message`.
    pub(super) fn panic_static(
        &mut self,
        code: &'static str,
        message: &str,
        span: Span,
    ) -> Result<Label, Diagnostic> {
        if self.deferred {
            let status = format!("error[{code}]: {message}");
            return self.defer_panic(|body| Ok(body.text(&status)));
        }
        let (before, after) = self.report(code, span);
        self.cold_block(|body| {
            let report = body.text(&format!("{before}{message}{after}"));
            let unit = body.call_runtime("panic", vec![report], NOWHERE)?;
            body.push(Inst::Return(unit));
            Ok(())
        })
    }

    /// A cold block that panics with `code`, by the `std.rt` function
    /// `function`, which writes the message between the report's start and
    /// end texts, which it is given first, then `args`.
    pub(super) fn panic_text(
        &mut self,
        code: &'static str,
        function: &str,
        args: &[Temp],
        span: Span,
    ) -> Result<Label, Diagnostic> {
        if self.deferred {
            let Some(message) = function.strip_prefix("panic_") else {
                return Err(self.refuse(span, &format!("`{function}` in a function value")));
            };
            let builder = format!("{message}_message");
            return self.defer_panic(|body| {
                let head = body.text(&format!("error[{code}]: "));
                let mut all = vec![head];
                all.extend_from_slice(args);
                body.call_runtime(&builder, all, NOWHERE)
            });
        }
        let (before, after) = self.report(code, span);
        self.cold_block(|body| {
            let start = body.text(&before);
            let end = body.text(&after);
            let mut all = vec![start, end];
            all.extend_from_slice(args);
            let unit = body.call_runtime(function, all, NOWHERE)?;
            body.push(Inst::Return(unit));
            Ok(())
        })
    }

    /// A cold block that panics with `code` and the message `head`, the
    /// integer `a`, then `tail`.
    pub(super) fn panic_unary(
        &mut self,
        code: &'static str,
        (head, a, tail): (&str, Temp, &str),
        unsigned: bool,
        span: Span,
    ) -> Result<Label, Diagnostic> {
        if self.deferred {
            return self.defer_panic(|body| {
                let start = body.text(&format!("error[{code}]: {head}"));
                let end = body.text(tail);
                let unsigned = body.constant(i64::from(unsigned));
                body.call_runtime("unary_message", vec![start, a, end, unsigned], NOWHERE)
            });
        }
        let (before, after) = self.report(code, span);
        self.cold_block(|body| {
            let start = body.text(&format!("{before}{head}"));
            let end = body.text(&format!("{tail}{after}"));
            let unsigned = body.constant(i64::from(unsigned));
            let unit = body.call_runtime("panic_unary", vec![start, a, end, unsigned], NOWHERE)?;
            body.push(Inst::Return(unit));
            Ok(())
        })
    }

    /// A cold block that panics with `code` and the message `head`, the
    /// integer `a`, `mid`, the integer `b`, then `tail`.
    pub(super) fn panic_binary(
        &mut self,
        code: &'static str,
        (head, a, mid, b, tail): (&str, Temp, &str, Temp, &str),
        unsigned: bool,
        span: Span,
    ) -> Result<Label, Diagnostic> {
        if self.deferred {
            return self.defer_panic(|body| {
                let start = body.text(&format!("error[{code}]: {head}"));
                let mid = body.text(mid);
                let end = body.text(tail);
                let unsigned = body.constant(i64::from(unsigned));
                body.call_runtime(
                    "binary_message",
                    vec![start, a, mid, b, end, unsigned],
                    NOWHERE,
                )
            });
        }
        let (before, after) = self.report(code, span);
        self.cold_block(|body| {
            let start = body.text(&format!("{before}{head}"));
            let mid = body.text(mid);
            let end = body.text(&format!("{tail}{after}"));
            let unsigned = body.constant(i64::from(unsigned));
            let unit = body.call_runtime(
                "panic_binary",
                vec![start, a, mid, b, end, unsigned],
                NOWHERE,
            )?;
            body.push(Inst::Return(unit));
            Ok(())
        })
    }

    // ---- Calls -------------------------------------------------------------
}
