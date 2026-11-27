// SPDX-License-Identifier: Apache-2.0
//! Lists: literals, the prelude's `List` methods, `for` over a list, list
//! equality, and the drop function of a list type. `layout.rs` says how a
//! list object is laid out. `List.eq` is `traits.rs`'s `Body::equal`,
//! which calls the function generated per item type here
//! ([`Body::list_eq_function`]).
//!
//! # Copy-on-write
//!
//! A list is a value (ADR 0001), as in the interpreter, where it is an
//! `Rc<Vec>` changed through `Rc::make_mut`. `push` adds the item in
//! place when the list object is unshared (its count is 1) and has room.
//! Otherwise it first calls `std.rt`'s `list_reserve`, which gives an
//! unshared object with room: the same items in a new object, retained if
//! they hold references, while every other holder keeps the old one. The
//! new object goes back where the list is held, as a struct copied for a
//! field update does (`data.rs`).
//!
//! # Items
//!
//! Every item is one word, of any type. Reading one retains it, as
//! reading a field does; `for` borrows each item from the list it holds
//! while the loop runs. The last reference to a list releases its items
//! (the type's drop function, [`Body::drop_list`]).

use oska_check::builtins;
use oska_check::program::{Callee, Expr, ExprKind, Place, Type};
use oska_syntax::{Diagnostic, Span};

use super::{constant_word, panics, Body, Held, Instance, Slot};
use crate::ir::{BinOp, Cond, Inst, Label, Temp};
use crate::layout;
use crate::NOWHERE;

impl Body<'_, '_> {
    /// The address of item `index` of the list object `list`, less
    /// [`layout::LIST_ITEMS`].
    pub(super) fn item_base(&mut self, list: Temp, index: Temp) -> Temp {
        let three = self.constant(3);
        let offset = self.binary_op(BinOp::Shl, index, three);
        self.binary_op(BinOp::Add, list, offset)
    }

    /// Item `index` of the list object `list`, touching no count.
    pub(super) fn item(&mut self, list: Temp, index: Temp) -> Temp {
        let base = self.item_base(list, index);
        self.load(base, layout::LIST_ITEMS)
    }

    /// A static list object holding the words `items`.
    fn static_list(&mut self, items: &[i64]) -> Temp {
        let at = self.lowering.rodata.list(items);
        self.rodata(at)
    }

    /// `[a, b, c]`: a static list when every item is a constant that
    /// holds no address, else a new object, which takes over the items'
    /// references.
    pub(super) fn list_literal(&mut self, expr: &Expr, items: &[Expr]) -> Result<Temp, Diagnostic> {
        let program = self.program();
        let constants: Option<Vec<i64>> = items.iter().map(|i| constant_word(program, i)).collect();
        if let Some(constants) = constants {
            return Ok(self.static_list(&constants));
        }
        let mut values = Vec::with_capacity(items.len());
        for item in items {
            values.push(self.expr(item)?);
        }
        let length = values.len() as i64;
        let object = self.alloc(i64::from(layout::LIST_ITEMS) + 8 * length)?;
        let count = self.constant(length);
        self.store(object, layout::LIST_LEN, count);
        self.store(object, layout::LIST_CAPACITY, count);
        for (index, &value) in values.iter().enumerate() {
            self.store(object, layout::LIST_ITEMS + 8 * index as u32, value);
            self.owned.remove(&value);
        }
        self.own(object, &expr.ty);
        Ok(object)
    }

    /// The prelude's `List` methods that read a list, inline: `name`
    /// applied to `values`, evaluated from `args`, for `expr`. `None` for
    /// any other intrinsic (`List.eq` is `Body::eq_intrinsic`'s;
    /// `List.map` calls `closures.rs`'s `Body::list_map`).
    pub(super) fn list_intrinsic(
        &mut self,
        name: &str,
        args: &[Expr],
        values: &[Temp],
        expr: &Expr,
    ) -> Result<Option<Temp>, Diagnostic> {
        if name == "List.new" {
            return Ok(Some(self.static_list(&[])));
        }
        let (Some(&list), Some(list_expr)) = (values.first(), args.first()) else {
            return Ok(None);
        };
        let list_ty = self.ty(&list_expr.ty);
        let item_ty = layout::list_item(&list_ty).cloned().unwrap_or(Type::Unit);
        let result = match (name, values.get(1)) {
            ("List.len" | "List.count", _) => self.load(list, layout::LIST_LEN),
            ("List.at", Some(&index)) => {
                let length = self.load(list, layout::LIST_LEN);
                let out_of_bounds = self.panic_binary(
                    panics::INDEX_OUT_OF_BOUNDS,
                    (
                        "index ",
                        index,
                        " is out of bounds for a list of length ",
                        length,
                        "",
                    ),
                    false,
                    expr.span,
                )?;
                // Unsigned, so a negative index is out of bounds too.
                self.push(Inst::Branch {
                    cond: Cond::AboveEq,
                    lhs: index,
                    rhs: length,
                    target: out_of_bounds,
                });
                let item = self.item(list, index);
                if self.counted(&item_ty) {
                    self.retain(item);
                }
                item
            }
            ("List.get", Some(&index)) => self.list_get(list, index, &item_ty, expr.span)?,
            ("List.map", Some(&function)) => {
                let mapped = self.list_map(list, function, expr.span)?;
                self.consume(function)?;
                mapped
            }
            _ => return Ok(None),
        };
        self.consume(list)?;
        self.own(result, &expr.ty);
        Ok(Some(result))
    }

    /// `list.get(index)`: `Some` of the item, retained, when `index` is in
    /// range, else the static `None`.
    fn list_get(
        &mut self,
        list: Temp,
        index: Temp,
        item_ty: &Type,
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        let some = self.variant(builtins::OPTION, builtins::SOME, span)? as i64;
        let none = self.variant(builtins::OPTION, builtins::NONE, span)? as i64;
        let result = self.temp();
        let (missing, end) = (self.label(), self.label());
        let length = self.load(list, layout::LIST_LEN);
        self.push(Inst::Branch {
            cond: Cond::AboveEq,
            lhs: index,
            rhs: length,
            target: missing,
        });
        let item = self.item(list, index);
        if self.counted(item_ty) {
            self.retain(item);
        }
        let object = self.alloc(layout::variant_size(1))?;
        let tag = self.constant(some);
        self.store(object, layout::ENUM_TAG, tag);
        self.store(object, layout::enum_field(0), item);
        self.push(Inst::Copy {
            dst: result,
            src: object,
        });
        self.push(Inst::Jump(end));
        self.push(Inst::Label(missing));
        let at = self.lowering.rodata.object(&[none]);
        let nothing = self.rodata(at);
        self.push(Inst::Copy {
            dst: result,
            src: nothing,
        });
        self.push(Inst::Label(end));
        Ok(result)
    }

    /// Where the list at `receiver` is held: its temporary, its type, and
    /// the struct object and offset that hold it when it is a field,
    /// which is made unshared. `what` names the change for a refusal.
    fn list_place(
        &mut self,
        receiver: &Place,
        span: Span,
        what: &str,
    ) -> Result<(Temp, Type, Option<(Temp, u32)>), Diagnostic> {
        Ok(match receiver {
            Place::Local(_) | Place::SelfValue => match self.holder(receiver) {
                Some((temp, ty, _)) => (temp, ty, None),
                None => return Err(self.refuse(span, what)),
            },
            Place::Field { base, index } => {
                let (parent, parent_ty) = self.unique_place(base, span)?;
                let ty = self.struct_field_type(&parent_ty, *index);
                let offset = layout::struct_field(*index);
                (self.load(parent, offset), ty, Some((parent, offset)))
            }
        })
    }

    /// Make the list object in `list` unshared before an item of it
    /// changes, without room for more: a copy of exactly its items when it
    /// is shared (`std.rt`'s `list_unshare`), which goes back to where the
    /// list is held.
    fn list_unshare(
        &mut self,
        list: Temp,
        item_ty: &Type,
        held_at: Option<(Temp, u32)>,
    ) -> Result<(), Diagnostic> {
        let ready = self.label();
        let count = self.load(list, 0);
        self.push(Inst::BranchImm {
            cond: Cond::Eq,
            lhs: count,
            imm: 1,
            target: ready,
        });
        let counted = self.constant(i64::from(self.counted(item_ty)));
        let own = self.call_runtime("list_unshare", vec![list, counted], NOWHERE)?;
        self.push(Inst::Copy {
            dst: list,
            src: own,
        });
        if let Some((parent, offset)) = held_at {
            self.store(parent, offset, list);
        }
        self.push(Inst::Label(ready));
        Ok(())
    }

    /// `receiver.set(index, item)`, `args` holding both: `P0002` when the
    /// index is out of bounds, else the item replaces the one there in an
    /// unshared list, which releases the old one.
    pub(super) fn list_set(
        &mut self,
        receiver: &Place,
        args: &[Expr],
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        let [index_expr, item_expr] = args else {
            return Err(self.refuse(span, "`List.set` without an index and an item"));
        };
        let index = self.expr(index_expr)?;
        let item = self.expr(item_expr)?;
        let (list, list_ty, held_at) =
            self.list_place(receiver, span, "changing an item of this list")?;
        let item_ty = layout::list_item(&list_ty).cloned().unwrap_or(Type::Unit);
        let length = self.load(list, layout::LIST_LEN);
        let out_of_bounds = self.panic_binary(
            panics::INDEX_OUT_OF_BOUNDS,
            (
                "index ",
                index,
                " is out of bounds for a list of length ",
                length,
                "",
            ),
            false,
            span,
        )?;
        // Unsigned, so a negative index is out of bounds too.
        self.push(Inst::Branch {
            cond: Cond::AboveEq,
            lhs: index,
            rhs: length,
            target: out_of_bounds,
        });
        self.list_unshare(list, &item_ty, held_at)?;
        let base = self.item_base(list, index);
        if self.counted(&item_ty) {
            let old = self.load(base, layout::LIST_ITEMS);
            self.store(base, layout::LIST_ITEMS, item);
            self.release(old, &item_ty)?;
        } else {
            self.store(base, layout::LIST_ITEMS, item);
        }
        // The list takes over the item's reference.
        self.owned.remove(&item);
        Ok(self.constant(0))
    }

    /// `receiver.pop()`: `None` for an empty list, else `Some` of the last
    /// item, which the list gives up, in an unshared list.
    pub(super) fn list_pop(&mut self, receiver: &Place, span: Span) -> Result<Temp, Diagnostic> {
        let (list, list_ty, held_at) =
            self.list_place(receiver, span, "removing an item of this list")?;
        let item_ty = layout::list_item(&list_ty).cloned().unwrap_or(Type::Unit);
        let some = self.variant(builtins::OPTION, builtins::SOME, span)? as i64;
        let none = self.variant(builtins::OPTION, builtins::NONE, span)? as i64;
        let result = self.temp();
        let (missing, end) = (self.label(), self.label());
        let length = self.load(list, layout::LIST_LEN);
        self.push(Inst::JumpIfZero {
            cond: length,
            target: missing,
        });
        self.list_unshare(list, &item_ty, held_at)?;
        let one = self.constant(1);
        let shorter = self.binary_op(BinOp::Sub, length, one);
        self.store(list, layout::LIST_LEN, shorter);
        let item = self.item(list, shorter);
        let object = self.alloc(layout::variant_size(1))?;
        let tag = self.constant(some);
        self.store(object, layout::ENUM_TAG, tag);
        self.store(object, layout::enum_field(0), item);
        self.push(Inst::Copy {
            dst: result,
            src: object,
        });
        self.push(Inst::Jump(end));
        self.push(Inst::Label(missing));
        let at = self.lowering.rodata.object(&[none]);
        let nothing = self.rodata(at);
        self.push(Inst::Copy {
            dst: result,
            src: nothing,
        });
        self.push(Inst::Label(end));
        Ok(result)
    }

    /// `receiver.truncate(n)`: the items from `n` on released and dropped
    /// from an unshared list, when `n` is below the length; a negative `n`
    /// is 0.
    pub(super) fn list_truncate(
        &mut self,
        receiver: &Place,
        args: &[Expr],
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        let [n_expr] = args else {
            return Err(self.refuse(span, "`List.truncate` without a length"));
        };
        let n = self.expr(n_expr)?;
        let (list, list_ty, held_at) =
            self.list_place(receiver, span, "removing items of this list")?;
        let item_ty = layout::list_item(&list_ty).cloned().unwrap_or(Type::Unit);
        let keep = self.copy(n);
        let (positive, done) = (self.label(), self.label());
        self.push(Inst::BranchImm {
            cond: Cond::Ge,
            lhs: keep,
            imm: 0,
            target: positive,
        });
        let zero = self.constant(0);
        self.push(Inst::Copy {
            dst: keep,
            src: zero,
        });
        self.push(Inst::Label(positive));
        let length = self.load(list, layout::LIST_LEN);
        self.push(Inst::Branch {
            cond: Cond::Ge,
            lhs: keep,
            rhs: length,
            target: done,
        });
        self.list_unshare(list, &item_ty, held_at)?;
        self.store(list, layout::LIST_LEN, keep);
        if self.counted(&item_ty) {
            let at = self.copy(keep);
            let (head, exit) = (self.label(), self.label());
            self.push(Inst::Label(head));
            self.push(Inst::Branch {
                cond: Cond::Ge,
                lhs: at,
                rhs: length,
                target: exit,
            });
            let item = self.item(list, at);
            self.release(item, &item_ty)?;
            let one = self.constant(1);
            self.push(Inst::Binary {
                op: BinOp::Add,
                dst: at,
                lhs: at,
                rhs: one,
            });
            self.push(Inst::Jump(head));
            self.push(Inst::Label(exit));
        }
        self.push(Inst::Label(done));
        Ok(self.constant(0))
    }

    /// `receiver.push(item)`, `args` holding the item, on a list held at a
    /// place: the item is added in place when the list object is unshared
    /// and has room; otherwise `std.rt`'s `list_reserve` gives one that
    /// is, which goes back to the place first.
    pub(super) fn list_push(
        &mut self,
        receiver: &Place,
        args: &[Expr],
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        let Some(item_expr) = args.first() else {
            return Err(self.refuse(span, "`List.push` without an item"));
        };
        let item = self.expr(item_expr)?;
        let (list, list_ty, held_at) = self.list_place(receiver, span, "adding to this list")?;
        let item_ty = layout::list_item(&list_ty).cloned().unwrap_or(Type::Unit);
        let (reserve, ready) = (self.label(), self.label());
        let count = self.load(list, 0);
        self.push(Inst::BranchImm {
            cond: Cond::Ne,
            lhs: count,
            imm: 1,
            target: reserve,
        });
        let length = self.load(list, layout::LIST_LEN);
        let capacity = self.load(list, layout::LIST_CAPACITY);
        self.push(Inst::Branch {
            cond: Cond::Lt,
            lhs: length,
            rhs: capacity,
            target: ready,
        });
        self.push(Inst::Label(reserve));
        let counted = self.constant(i64::from(self.counted(&item_ty)));
        let room = self.call_runtime("list_reserve", vec![list, counted], NOWHERE)?;
        self.push(Inst::Copy {
            dst: list,
            src: room,
        });
        if let Some((parent, offset)) = held_at {
            self.store(parent, offset, list);
        }
        self.push(Inst::Label(ready));
        let length = self.load(list, layout::LIST_LEN);
        let base = self.item_base(list, length);
        self.store(base, layout::LIST_ITEMS, item);
        let one = self.constant(1);
        let longer = self.binary_op(BinOp::Add, length, one);
        self.store(list, layout::LIST_LEN, longer);
        // The list takes over the item's reference.
        self.owned.remove(&item);
        Ok(self.constant(0))
    }

    /// `for x in list { body }`, the statement at `span`: each item in
    /// order, borrowed from the list, which the loop holds (so a `push` in
    /// the body changes a copy).
    pub(super) fn for_list(
        &mut self,
        binds: bool,
        ty: &Type,
        iter: &Expr,
        body: &Expr,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let list = self.expr(iter)?;
        let held = self.owned.remove(&list);
        if let Some(held_ty) = &held {
            self.held.push(Held {
                temp: list,
                ty: held_ty.clone(),
                loops: self.loops.len(),
                scope: false,
            });
        }
        let length = self.load(list, layout::LIST_LEN);
        let zero = self.constant(0);
        let at = self.copy(zero);
        let (head, next, exit) = (self.label(), self.label(), self.label());
        self.push(Inst::Label(head));
        self.push(Inst::Branch {
            cond: Cond::Ge,
            lhs: at,
            rhs: length,
            target: exit,
        });
        if binds {
            let item = self.item(list, at);
            let temp = self.copy(item);
            let ty = self.ty(ty);
            self.slots.push(Slot {
                temp,
                release: false,
                ty,
            });
        }
        self.loop_body(next, exit, body)?;
        if binds {
            self.slots.pop();
        }
        self.push(Inst::Label(next));
        self.step(span)?;
        let one = self.constant(1);
        self.push(Inst::Binary {
            op: BinOp::Add,
            dst: at,
            lhs: at,
            rhs: one,
        });
        self.push(Inst::Jump(head));
        self.push(Inst::Label(exit));
        if let Some(held_ty) = held {
            self.held.pop();
            self.release(list, &held_ty)?;
        }
        Ok(())
    }

    /// The text whose characters `iter` lists, when it is a call of
    /// `String.chars`: `for` over it walks the text instead
    /// ([`Body::for_chars`]).
    pub(super) fn chars_of<'e>(&self, iter: &'e Expr) -> Option<&'e Expr> {
        let ExprKind::Call {
            callee,
            type_args,
            args,
        } = &iter.kind
        else {
            return None;
        };
        let [text] = args.as_slice() else {
            return None;
        };
        let (callee, _) = self
            .resolve(callee, type_args, Some(&text.ty), iter.span)
            .ok()?;
        matches!(&callee, Callee::Intrinsic(name) if name == "String.chars").then_some(text)
    }

    /// `for c in text.chars() { body }`, `text` the receiver of `chars`:
    /// each character made as the loop reaches it (`std.rt`'s
    /// `char_text`), with no list of them all, so a loop that stops early
    /// reads only the characters it looks at. The current character is
    /// held, released when the next is made and when the loop is left;
    /// the binding borrows it.
    pub(super) fn for_chars(
        &mut self,
        binds: bool,
        ty: &Type,
        text_expr: &Expr,
        body: &Expr,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let text = self.expr(text_expr)?;
        let loops = self.loops.len();
        let held = self.owned.remove(&text);
        if let Some(held_ty) = &held {
            self.held.push(Held {
                temp: text,
                ty: held_ty.clone(),
                loops,
                scope: false,
            });
        }
        let length = self.load(text, layout::TEXT_LEN);
        let zero = self.constant(0);
        let at = self.copy(zero);
        // A static text until the first character: releasing it is nothing.
        let empty = self.text("");
        let item = self.copy(empty);
        self.held.push(Held {
            temp: item,
            ty: Type::String,
            loops,
            scope: false,
        });
        let (head, next, exit) = (self.label(), self.label(), self.label());
        self.push(Inst::Label(head));
        self.push(Inst::Branch {
            cond: Cond::Ge,
            lhs: at,
            rhs: length,
            target: exit,
        });
        self.release(item, &Type::String)?;
        let made = self.call_runtime("char_text", vec![text, at], NOWHERE)?;
        self.push(Inst::Copy {
            dst: item,
            src: made,
        });
        if binds {
            let temp = self.copy(item);
            let ty = self.ty(ty);
            self.slots.push(Slot {
                temp,
                release: false,
                ty,
            });
        }
        self.loop_body(next, exit, body)?;
        if binds {
            self.slots.pop();
        }
        self.push(Inst::Label(next));
        self.step(span)?;
        let size = self.load(item, layout::TEXT_LEN);
        self.push(Inst::Binary {
            op: BinOp::Add,
            dst: at,
            lhs: at,
            rhs: size,
        });
        self.push(Inst::Jump(head));
        self.push(Inst::Label(exit));
        self.held.pop();
        self.release(item, &Type::String)?;
        if let Some(held_ty) = held {
            self.held.pop();
            self.release(text, &held_ty)?;
        }
        Ok(())
    }

    /// The part of the drop function of the list type whose items are of
    /// type `item_ty` (which hold references) that releases each item of
    /// `list`.
    pub(super) fn drop_list(&mut self, list: Temp, item_ty: &Type) -> Result<(), Diagnostic> {
        let length = self.load(list, layout::LIST_LEN);
        let zero = self.constant(0);
        let at = self.copy(zero);
        let (head, exit) = (self.label(), self.label());
        self.push(Inst::Label(head));
        self.push(Inst::Branch {
            cond: Cond::Ge,
            lhs: at,
            rhs: length,
            target: exit,
        });
        let item = self.item(list, at);
        self.release(item, item_ty)?;
        let one = self.constant(1);
        self.push(Inst::Binary {
            op: BinOp::Add,
            dst: at,
            lhs: at,
            rhs: one,
        });
        self.push(Inst::Jump(head));
        self.push(Inst::Label(exit));
        Ok(())
    }

    /// Whether the lists `a` and `b`, of items of the concrete type
    /// `item_ty`, are equal, by the function generated for it: what
    /// `Body::equal` (`traits.rs`) does for a list.
    pub(super) fn list_equal(
        &mut self,
        item_ty: &Type,
        a: Temp,
        b: Temp,
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        let instance = Instance::ListEq(self.source, item_ty.clone(), self.step_site(span));
        let id = self
            .lowering
            .request(instance, self.origin.unwrap_or(NOWHERE));
        self.call_id(id, vec![a, b], NOWHERE)
    }

    /// The body of [`Instance::ListEq`] for items of `item_ty`: whether
    /// its two parameters, lists, have the same length and equal items,
    /// compared in order by `Eq`, as the interpreter's `List.eq` does,
    /// each at `span`.
    pub(super) fn list_eq_function(
        &mut self,
        item_ty: &Type,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let (a, b) = (self.temp(), self.temp());
        let (yes, no) = (self.label(), self.label());
        self.push(Inst::Branch {
            cond: Cond::Eq,
            lhs: a,
            rhs: b,
            target: yes,
        });
        let length = self.load(a, layout::LIST_LEN);
        let other = self.load(b, layout::LIST_LEN);
        self.push(Inst::Branch {
            cond: Cond::Ne,
            lhs: length,
            rhs: other,
            target: no,
        });
        let zero = self.constant(0);
        let at = self.copy(zero);
        let head = self.label();
        self.push(Inst::Label(head));
        self.push(Inst::Branch {
            cond: Cond::Ge,
            lhs: at,
            rhs: length,
            target: yes,
        });
        let x = self.item(a, at);
        let y = self.item(b, at);
        let equal = self.equal(item_ty, x, y, span)?;
        self.push(Inst::JumpIfZero {
            cond: equal,
            target: no,
        });
        let one = self.constant(1);
        self.push(Inst::Binary {
            op: BinOp::Add,
            dst: at,
            lhs: at,
            rhs: one,
        });
        self.push(Inst::Jump(head));
        self.answer(yes, 1);
        self.answer(no, 0);
        Ok(())
    }

    /// At `label`, return the `Bool` `value`.
    fn answer(&mut self, label: Label, value: i64) {
        self.push(Inst::Label(label));
        let value = self.constant(value);
        self.push(Inst::Return(value));
    }
}
