// SPDX-License-Identifier: Apache-2.0
//! `Bytes`, natively: its mutating methods. `layout.rs` says how a bytes
//! object is laid out, and `std.rt` implements every method (`rt.osk`,
//! "Bytes"); `intrinsics/` says which function each calls.
//!
//! # Copy-on-write
//!
//! A `Bytes` is a value (ADR 0001), as in the interpreter, where it is an
//! `Rc<Vec<u8>>` changed through `Rc::make_mut`. A mutating method
//! ([`Prelude::Mutating`](crate::intrinsics::Prelude::Mutating)) is called
//! with the object the place holds, and the place's reference to it, and
//! gives back the object the place holds afterwards: the same one, changed
//! in place, when it was unshared and had room, else a copy of its own,
//! while every other holder keeps the old one. That object goes back to
//! the place, as a list's does after `push` (`lists.rs`). The word 0 in its
//! place means the call is a bug and changed nothing: the code panics.

use oska_check::program::{Expr, Place};
use oska_syntax::{Diagnostic, Span};

use super::Body;
use crate::intrinsics::Check;
use crate::ir::{Inst, Temp};
use crate::layout;
use crate::NOWHERE;

impl Body<'_, '_> {
    /// `receiver.method(args)`, a mutating method of `Bytes` that calls
    /// `std.rt`'s `function`, on the bytes held at a place: the object it
    /// gives back goes back to the place, or the call panics as `check`
    /// says.
    pub(super) fn bytes_mutation(
        &mut self,
        function: &str,
        check: Check,
        receiver: &Place,
        args: &[Expr],
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        let mut values = Vec::with_capacity(args.len());
        for arg in args {
            values.push(self.expr(arg)?);
        }
        // Where the bytes are: a local, or a field of a struct made
        // unshared.
        let (object, held_at) = match receiver {
            Place::Local(_) | Place::SelfValue => match self.holder(receiver) {
                Some((temp, _, _)) => (temp, None),
                None => return Err(self.refuse(span, "changing these bytes")),
            },
            Place::Field { base, index } => {
                let (parent, _) = self.unique_place(base, span)?;
                let offset = layout::struct_field(*index);
                (self.load(parent, offset), Some((parent, offset)))
            }
        };
        let mut call_args = vec![object];
        call_args.extend(values.iter().copied());
        let after = self.call_runtime(function, call_args.clone(), NOWHERE)?;
        self.check(check, after, &call_args, span)?;
        match held_at {
            Some((parent, offset)) => self.store(parent, offset, after),
            None => self.push(Inst::Copy {
                dst: object,
                src: after,
            }),
        }
        for value in values {
            self.consume(value)?;
        }
        Ok(self.constant(0))
    }
}
