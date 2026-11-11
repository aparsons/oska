//! `dbg`: the text of a value as the interpreter shows it (`1`, `"a"`,
//! `Point(1, 2)`, `Shape.Circle(3)`, `Option.Some(4)`, `<redacted>` for a
//! `Secret`). Text, `Int`s and `Bool`s come from `std.rt`; a struct or an
//! enum has a generated function of its own per type
//! ([`Instance::Debug`]), which handles recursive types, and so has a
//! list type (`[1, 2]`). Closures, ranges and capabilities are refused
//! (`E0390`) until they have a representation.
//!
//! Every function here returns a text that is a new reference or static;
//! the caller releases it.

use oska_check::builtins;
use oska_check::program::{IntType, Type, TypeDeclKind};
use oska_syntax::{Diagnostic, Span};

use super::{Body, Instance};
use crate::ir::{BinOp, Cond, Inst, Temp};
use crate::layout::{self, Fields};
use crate::NOWHERE;

impl Body<'_, '_> {
    /// The `dbg` text of `value`, of the concrete type `ty`.
    pub(super) fn debug_text(
        &mut self,
        ty: &Type,
        value: Temp,
        span: Span,
    ) -> Result<Temp, Diagnostic> {
        match ty {
            Type::Unit => Ok(self.text("()")),
            Type::Bool => self.call_runtime("debug_bool", vec![value], span),
            Type::Int(int) => {
                let unsigned = self.constant(i64::from(*int == IntType::U64));
                self.call_runtime("debug_int", vec![value, unsigned], span)
            }
            Type::String => self.call_runtime("debug_string", vec![value], span),
            Type::Named { name, .. } if name == builtins::SECRET => Ok(self.text("<redacted>")),
            Type::Named { name, .. }
                if !builtins::is_capability(name)
                    && !matches!(name.as_str(), "Range" | "Task" | "Scope" | "Args")
                    && self.program().types.contains_key(name) =>
            {
                let instance = Instance::Debug(self.source, ty.clone());
                let origin = self.origin.unwrap_or(span);
                let id = self.lowering.request(instance, origin);
                self.call_id(id, vec![value], span)
            }
            other => Err(self.refuse(span, &format!("`dbg` of a `{other}`"))),
        }
    }

    /// The text `acc` followed by `piece`, both released.
    fn debug_append(&mut self, acc: Temp, piece: Temp) -> Result<Temp, Diagnostic> {
        let joined = self.call_runtime("debug_concat", vec![acc, piece], NOWHERE)?;
        self.release(acc, &Type::String)?;
        self.release(piece, &Type::String)?;
        Ok(joined)
    }

    /// `acc` followed by the static text `text`.
    fn debug_append_text(&mut self, acc: Temp, text: &str) -> Result<Temp, Diagnostic> {
        let piece = self.text(text);
        self.debug_append(acc, piece)
    }

    /// `head`, then the `(f, g)` of `fields` loaded at `offset` of
    /// `object` (nothing when `bare` and there are none), as one text.
    fn debug_fields(
        &mut self,
        head: &str,
        object: Temp,
        fields: &[Type],
        offset: impl Fn(usize) -> u32,
        bare: bool,
    ) -> Result<Temp, Diagnostic> {
        let mut acc = self.text(head);
        if bare && fields.is_empty() {
            return Ok(acc);
        }
        acc = self.debug_append_text(acc, "(")?;
        for (index, field) in fields.iter().enumerate() {
            if index > 0 {
                acc = self.debug_append_text(acc, ", ")?;
            }
            let value = self.load(object, offset(index));
            let piece = self.debug_text(field, value, NOWHERE)?;
            acc = self.debug_append(acc, piece)?;
        }
        self.debug_append_text(acc, ")")
    }

    /// The body of [`Instance::Debug`] for `ty`: its one parameter is the
    /// value, and it returns the text.
    pub(super) fn debug_function(&mut self, ty: &Type) -> Result<(), Diagnostic> {
        let value = self.temp();
        let Type::Named { name, .. } = ty else {
            return Err(self.refuse(NOWHERE, &format!("`dbg` of a `{ty}`")));
        };
        match self.fields_of(ty) {
            Some(Fields::Struct(fields)) => {
                let text = self.debug_fields(name, value, &fields, layout::struct_field, false)?;
                self.push(Inst::Return(text));
            }
            Some(Fields::Enum(variants)) => {
                let decl = self.program().types.get(name);
                let names: Vec<String> = match decl.map(|d| &d.kind) {
                    Some(TypeDeclKind::Enum(vs)) => vs.iter().map(|v| v.name.clone()).collect(),
                    _ => Vec::new(),
                };
                let plain = decl.is_some_and(layout::is_fieldless_enum);
                // A fieldless enum is its variant index; any other is an
                // object whose second word is the index.
                let tag = if plain {
                    value
                } else {
                    self.load(value, layout::ENUM_TAG)
                };
                for (index, fields) in variants.iter().enumerate() {
                    let next = self.label();
                    let want = self.constant(index as i64);
                    self.push(Inst::Branch {
                        cond: Cond::Ne,
                        lhs: tag,
                        rhs: want,
                        target: next,
                    });
                    let variant = names.get(index).map_or("", String::as_str);
                    let head = format!("{name}.{variant}");
                    let text = self.debug_fields(&head, value, fields, layout::enum_field, true)?;
                    self.push(Inst::Return(text));
                    self.push(Inst::Label(next));
                }
                let none = self.text("");
                self.push(Inst::Return(none));
            }
            None => match layout::list_item(ty) {
                Some(item) => {
                    let item = item.clone();
                    let text = self.debug_list(value, &item)?;
                    self.push(Inst::Return(text));
                }
                None => return Err(self.refuse(NOWHERE, &format!("`dbg` of a `{ty}`"))),
            },
        }
        Ok(())
    }

    /// The `dbg` text of the list `list` of items of `item`: `[a, b]`.
    fn debug_list(&mut self, list: Temp, item: &Type) -> Result<Temp, Diagnostic> {
        let length = self.load(list, layout::LIST_LEN);
        let open = self.text("[");
        let acc = self.copy(open);
        let zero = self.constant(0);
        let at = self.copy(zero);
        let (head, first, exit) = (self.label(), self.label(), self.label());
        self.push(Inst::Label(head));
        self.push(Inst::Branch {
            cond: Cond::Ge,
            lhs: at,
            rhs: length,
            target: exit,
        });
        self.push(Inst::BranchImm {
            cond: Cond::Eq,
            lhs: at,
            imm: 0,
            target: first,
        });
        let joined = self.debug_append_text(acc, ", ")?;
        self.push(Inst::Copy {
            dst: acc,
            src: joined,
        });
        self.push(Inst::Label(first));
        let three = self.constant(3);
        let offset = self.binary_op(BinOp::Shl, at, three);
        let base = self.binary_op(BinOp::Add, list, offset);
        let value = self.load(base, layout::LIST_ITEMS);
        let piece = self.debug_text(item, value, NOWHERE)?;
        let joined = self.debug_append(acc, piece)?;
        self.push(Inst::Copy {
            dst: acc,
            src: joined,
        });
        let one = self.constant(1);
        self.push(Inst::Binary {
            op: BinOp::Add,
            dst: at,
            lhs: at,
            rhs: one,
        });
        self.push(Inst::Jump(head));
        self.push(Inst::Label(exit));
        self.debug_append_text(acc, "]")
    }
}
