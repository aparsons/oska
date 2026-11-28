// SPDX-License-Identifier: Apache-2.0
//! String interpolation.

use super::*;

impl Body<'_, '_> {
    /// An interpolated string: each part's text, each hole's through the
    /// `Display` implementation the checker chose, copied into one new
    /// text object. An `Int` hole is written straight into it (std.rt
    /// `int_length`, `int_put`), with no text of its own to allocate,
    /// copy and free: most interpolations format numbers.
    pub(super) fn interpolated(&mut self, parts: &[Part]) -> Result<Temp, Diagnostic> {
        /// A part: a text object, a short literal (its static object and
        /// length), or an `Int` and the length of its text.
        enum Piece {
            Text(Temp),
            Literal(Temp, u32),
            Int(Temp, Temp),
        }
        /// The longest literal copied inline, a word at a time.
        const INLINE: usize = 32;
        let mut pieces = Vec::with_capacity(parts.len());
        // Whether a literal shorter than a word is copied as a whole word,
        // writing up to 7 bytes past it: the next part overwrites them,
        // and after the last there must be room.
        let mut slack = false;
        for part in parts {
            match part {
                Part::Text(text) if text.is_empty() => {}
                Part::Text(text) if text.len() <= INLINE => {
                    slack |= text.len() < 8;
                    let object = self.text(text);
                    pieces.push(Piece::Literal(object, text.len() as u32));
                }
                Part::Text(text) => pieces.push(Piece::Text(self.text(text))),
                Part::Hole {
                    value,
                    display,
                    type_args,
                } => {
                    let shown = self.expr(value)?;
                    let (display, type_args) =
                        self.resolve(display, type_args, Some(&value.ty), value.span)?;
                    if matches!(&display, Callee::Intrinsic(name) if name == "Int.to_string") {
                        let length = self.call_runtime("int_length", vec![shown], NOWHERE)?;
                        pieces.push(Piece::Int(shown, length));
                        continue;
                    }
                    let text =
                        self.call(&display, &type_args, vec![shown], &Type::String, value.span)?;
                    pieces.push(Piece::Text(text));
                }
            }
        }
        let mut length = self.constant(0);
        for piece in &pieces {
            let part = match *piece {
                Piece::Text(text) => self.load(text, layout::TEXT_LEN),
                Piece::Literal(_, size) => self.constant(i64::from(size)),
                Piece::Int(_, length) => length,
            };
            length = self.binary_op(BinOp::Add, length, part);
        }
        let object = if slack {
            let seven = self.constant(7);
            let room = self.binary_op(BinOp::Add, length, seven);
            let object = self.call_runtime("text_new", vec![room], NOWHERE)?;
            self.store(object, layout::TEXT_LEN, length);
            object
        } else {
            self.call_runtime("text_new", vec![length], NOWHERE)?
        };
        let mut at = self.constant(0);
        for piece in &pieces {
            at = match *piece {
                Piece::Text(text) => {
                    self.call_runtime("text_put", vec![object, at, text], NOWHERE)?
                }
                Piece::Literal(text, size) => {
                    self.put_literal(object, at, text, size);
                    let size = self.constant(i64::from(size));
                    self.binary_op(BinOp::Add, at, size)
                }
                Piece::Int(value, length) => {
                    self.call_runtime("int_put", vec![object, at, value, length], NOWHERE)?
                }
            };
        }
        for piece in pieces {
            if let Piece::Text(text) = piece {
                self.consume(text)?;
            }
        }
        self.own(object, &Type::String);
        Ok(object)
    }

    /// Copy the `size` bytes of the static text object `text` into the
    /// text object `object` from its byte `at`, a word at a time: the last
    /// word overlaps the one before it, or for fewer than 8 bytes, one
    /// whole word is written, up to 7 bytes past them (the caller leaves
    /// room). Rodata texts are padded to a word, so every load is inside.
    pub(super) fn put_literal(&mut self, object: Temp, at: Temp, text: Temp, size: u32) {
        let data = layout::TEXT_LEN + layout::WORD;
        let to = self.binary_op(BinOp::Add, object, at);
        let mut offsets: Vec<u32> = (0..size / 8).map(|k| data + 8 * k).collect();
        if size < 8 {
            offsets.push(data);
        } else if size % 8 != 0 {
            offsets.push(data + size - 8);
        }
        for offset in offsets {
            let word = self.load(text, offset);
            self.store(to, offset, word);
        }
    }

    // ---- Statements --------------------------------------------------------
}
