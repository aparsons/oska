//! `test` blocks: the test executable's entry, each test's body, and the
//! `TestContext` methods.
//!
//! A test executable ([`Entry::Tests`], `crate::build_tests`) runs one
//! test per process, the one its argument names (`std.rt`'s
//! `test_selected`). Before the test's body runs, `std.rt`'s `test_begin`
//! records which host it runs on, in the runtime's writable word
//! [`TEST_HOST_WORD`]: 1, the test host in `std.rt`, for a plain test, or
//! 2, the real OS, for an `#[integration]` one. Which it is, is fixed here,
//! from the test's attribute.
//!
//! `std.rt` asks which host it runs on with the intrinsic
//! `rt_mem_test_host` wherever the test host differs from the OS (stdout,
//! stderr, panics, the clock, the filesystem). In a program's executable
//! ([`Entry::Main`]) that intrinsic is the constant 0, the real OS, so no
//! program can ever reach the test host, whatever its memory holds; in a
//! test executable it reads the word, which `test_begin` sets before any
//! of the test's code runs.
//!
//! `t`, the `TestContext`, is a static object: its fields are stateless
//! capabilities, words 0, which reach whatever host the process runs on.
//! A failed assertion, `t.fail` and any other panic write the panic's
//! report to stdout, unlocated (`std.rt`'s `report_fd`), for `oskac` to
//! read back (`crate::test_failure`).

use oska_check::builtins;
use oska_check::program::Type;
use oska_syntax::{Diagnostic, Span};

use super::{panics, Body, Entry, Instance, Slot};
use crate::ir::{Cond, Inst, Symbol, Temp};
use crate::NOWHERE;

/// The runtime's writable word that says which host a test executable's
/// test runs on (`std.rt`'s "Memory" section).
const TEST_HOST_WORD: u32 = 32;

/// The interpreter's messages for the assertions.
const CONDITION_FALSE: &str = "assertion failed: the condition is false";

impl Body<'_, '_> {
    /// `rt_mem_test_host()`: which host the process runs on.
    pub(super) fn test_host(&mut self) -> Temp {
        match self.lowering.kind {
            Entry::Main => self.constant(0),
            Entry::Tests => {
                let globals = self.temp();
                self.push(Inst::Addr {
                    dst: globals,
                    symbol: Symbol::Data(0),
                });
                self.load(globals, TEST_HOST_WORD * crate::layout::WORD)
            }
        }
    }

    /// A test executable's `_main`: run the test the argument selects, on
    /// its host, with `t`. `_main` exits afterwards.
    pub(super) fn call_test(&mut self) -> Result<(), Diagnostic> {
        let user = self.lowering.user;
        let count = self.constant(user.tests.len() as i64);
        let selected = self.call_runtime("test_selected", vec![count], NOWHERE)?;
        let fields = vec![0; builtins::test_context_fields().len()];
        for (index, test) in user.tests.iter().enumerate() {
            let next = self.label();
            let this = self.constant(index as i64);
            self.push(Inst::Branch {
                cond: Cond::Ne,
                lhs: selected,
                rhs: this,
                target: next,
            });
            let host = self.constant(if test.integration { 2 } else { 1 });
            let prefix = self.text(&format!("[{}] ", escape_control(&test.name)));
            self.call_runtime("test_begin", vec![host, prefix], NOWHERE)?;
            let at = self.lowering.rodata.object(&fields);
            let t = self.rodata(at);
            // As the interpreter runs a test body, not counted as a call.
            let id = self.lowering.request(Instance::Test(index), test.span);
            self.call_id(id, vec![t], test.span)?;
            self.push(Inst::Label(next));
        }
        Ok(())
    }

    /// The body of [`Instance::Test`]: the test's block, with `t`
    /// (borrowed) in slot 0. Its value is dropped. The span of the body.
    pub(super) fn test_function(&mut self, index: usize) -> Result<Span, Diagnostic> {
        let user = self.lowering.user;
        let Some(test) = user.tests.get(index) else {
            return Err(self.refuse(NOWHERE, "a test that is not there"));
        };
        let t = self.temp();
        self.slots.push(Slot {
            temp: t,
            release: false,
            ty: Type::Named {
                name: builtins::TEST_CONTEXT.to_string(),
                args: Vec::new(),
            },
        });
        let value = self.expr(&test.body)?;
        self.consume(value)?;
        let unit = self.constant(0);
        self.return_value(unit, test.body.span)?;
        Ok(test.body.span)
    }

    /// The `TestContext` method `name` that panics when it fails, called
    /// at `span` with `values` (`t` first), evaluated already and released
    /// here: its result, or `None` for any other intrinsic.
    pub(super) fn test_intrinsic(
        &mut self,
        name: &str,
        type_args: &[Type],
        values: &[Temp],
        span: Span,
    ) -> Result<Option<Temp>, Diagnostic> {
        match (name, values) {
            ("TestContext.assert", &[_, cond]) => {
                let failed = self.panic_static(panics::ASSERT_FAILED, CONDITION_FALSE, span)?;
                self.push(Inst::JumpIfZero {
                    cond,
                    target: failed,
                });
            }
            ("TestContext.fail", &[_, message]) => {
                let failed =
                    self.panic_text(panics::TEST_FAILED, "panic_message", &[message], span)?;
                self.push(Inst::Jump(failed));
            }
            ("TestContext.assert_eq" | "TestContext.assert_ne", &[_, actual, other]) => {
                let Some(ty) = type_args.first() else {
                    return Err(self.refuse(span, &format!("`{name}`")));
                };
                let ty = self.ty(ty);
                let equal = self.equal(&ty, actual, other, span)?;
                if name == "TestContext.assert_eq" {
                    let failed = self.cold_block(|body| {
                        // Read where the block starts: a hot temporary
                        // may sit in a register the first call clobbers,
                        // and a cold block's own temporaries are kept
                        // in the frame.
                        let (actual, other) = (body.copy(actual), body.copy(other));
                        let (start, end) = body.report(panics::ASSERT_EQ_FAILED, span);
                        let (start, end) = (body.text(&start), body.text(&end));
                        let actual = body.debug_text(&ty, actual, span)?;
                        let expected = body.debug_text(&ty, other, span)?;
                        let args = vec![start, end, actual, expected];
                        let unit = body.call_runtime("panic_assert_eq", args, NOWHERE)?;
                        body.push(Inst::Return(unit));
                        Ok(())
                    })?;
                    self.push(Inst::JumpIfZero {
                        cond: equal,
                        target: failed,
                    });
                } else {
                    let failed = self.cold_block(|body| {
                        let (start, end) = body.report(panics::ASSERT_NE_FAILED, span);
                        let (start, end) = (body.text(&start), body.text(&end));
                        let both = body.debug_text(&ty, actual, span)?;
                        let args = vec![start, end, both];
                        let unit = body.call_runtime("panic_assert_ne", args, NOWHERE)?;
                        body.push(Inst::Return(unit));
                        Ok(())
                    })?;
                    let zero = self.constant(0);
                    self.push(Inst::Branch {
                        cond: Cond::Ne,
                        lhs: equal,
                        rhs: zero,
                        target: failed,
                    });
                }
            }
            _ => return Ok(None),
        }
        for &value in values {
            self.consume(value)?;
        }
        Ok(Some(self.constant(0)))
    }
}

/// `text` with each control character escaped, as `\n`, `\r`, `\t`, or
/// `\u{..}`: the interpreter's `oska_run::testing::escape_control`, which
/// names a test in its stderr prefix.
fn escape_control(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{{{:x}}}", u32::from(c))),
            c => out.push(c),
        }
    }
    out
}
