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
use oska_check::program::{Expr, Place, Type};
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
        // Where the list is: a local, or a field of a struct made unshared.
        let (list, list_ty, held_at) = match receiver {
            Place::Local(_) | Place::SelfValue => match self.holder(receiver) {
                Some((temp, ty, _)) => (temp, ty, None),
                None => return Err(self.refuse(span, "adding to this list")),
            },
            Place::Field { base, index } => {
                let (parent, parent_ty) = self.unique_place(base, span)?;
                let ty = self.struct_field_type(&parent_ty, *index);
                let offset = layout::struct_field(*index);
                (self.load(parent, offset), ty, Some((parent, offset)))
            }
        };
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

    /// `for x in list { body }`: each item in order, borrowed from the
    /// list, which the loop holds (so a `push` in the body changes a copy).
    pub(super) fn for_list(
        &mut self,
        binds: bool,
        ty: &Type,
        iter: &Expr,
        body: &Expr,
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
    ) -> Result<Temp, Diagnostic> {
        let instance = Instance::ListEq(self.source, item_ty.clone());
        let id = self
            .lowering
            .request(instance, self.origin.unwrap_or(NOWHERE));
        self.call_id(id, vec![a, b], NOWHERE)
    }

    /// The body of [`Instance::ListEq`] for items of `item_ty`: whether
    /// its two parameters, lists, have the same length and equal items,
    /// compared in order by `Eq`, as the interpreter's `List.eq` does.
    pub(super) fn list_eq_function(&mut self, item_ty: &Type) -> Result<(), Diagnostic> {
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
        let equal = self.equal(item_ty, x, y, NOWHERE)?;
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
