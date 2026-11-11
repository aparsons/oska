//! How values are represented natively.
//!
//! Every value is one 64-bit word ([`crate::ir`]):
//!
//! - an integer of any type is its two's-complement bit pattern, sign- or
//!   zero-extended to 64 bits as its type says; `Bool` is 0 or 1; `()` is
//!   0; a capability carries no state, so it is 0 too;
//! - an enum whose every variant has no fields is its variant's index;
//! - anything else (text, a struct, an enum with fields) is the address
//!   of a heap **object**, whose first word is its reference count
//!   ([`STATIC`] for a static object in read-only data, which is never
//!   counted or freed):
//!   - text: `[count][length in bytes][UTF-8 bytes, padded to 8]`;
//!   - a struct: `[count][field 0][field 1]...`;
//!   - an enum with fields: `[count][variant index][field 0]...`.
//!
//! `std.rt` reads these layouts too (`std/rt.osk`), so a change here is a
//! change there.
//!
//! An object built at run time comes from `std.rt`'s allocator (`alloc`),
//! with a count of 1, and one header word before it that the allocator
//! keeps (its size class). The code keeps the count: every value of a
//! counted type ([`is_counted`]) that an expression produces is either
//! static or holds one reference of its own, which whoever consumes it
//! releases; a local holds one reference while it is in scope. When a
//! release drops the count to zero, `std.rt`'s `free` returns the
//! memory. Only text is built at run time so far: a struct or enum
//! built at run time, with fields to release before it is freed, comes
//! with #130.
//!
//! Static objects live in read-only data and are built from constants
//! that hold no address (an address in read-only data would need a
//! relocation).

use std::collections::BTreeMap;

use oska_check::builtins;
use oska_check::program::{IntType, Program, Type, TypeDecl, TypeDeclKind};

use crate::ir::IntKind;

/// The reference count of a static object.
pub const STATIC: i64 = -1;

/// The offset of a text object's length in bytes.
pub const TEXT_LEN: u32 = WORD;

/// How checked arithmetic sees `int`.
pub fn int_kind(int: IntType) -> IntKind {
    let (signed, bits) = match int {
        IntType::Int | IntType::I64 => (true, 64),
        IntType::I8 => (true, 8),
        IntType::I16 => (true, 16),
        IntType::I32 => (true, 32),
        IntType::U8 => (false, 8),
        IntType::U16 => (false, 16),
        IntType::U32 => (false, 32),
        IntType::U64 => (false, 64),
    };
    IntKind { signed, bits }
}

/// Whether a value of `ty` is an object's address, whose reference count
/// the code keeps: text, and structs and enums with fields. Integers,
/// `Bool`, `()`, fieldless enums and capabilities are not. The built-in
/// types with a handle (`List`, `Range`, `Task`, `Scope`) get their
/// representation with their tickets and are not counted until then.
pub fn is_counted(program: &Program, ty: &Type) -> bool {
    match ty {
        Type::String => true,
        Type::Named { name, .. } => {
            !builtins::is_capability(name)
                && !matches!(name.as_str(), "List" | "Range" | "Task" | "Scope" | "Args")
                && program
                    .types
                    .get(name)
                    .is_some_and(|decl| !is_fieldless_enum(decl))
        }
        _ => false,
    }
}

/// The size of a word, and of every field.
pub const WORD: u32 = 8;

/// The offset of field `index` of a struct object.
pub fn struct_field(index: usize) -> u32 {
    WORD * (1 + index as u32)
}

/// The offset of an enum object's variant index.
pub const ENUM_TAG: u32 = WORD;

/// The offset of field `index` of an enum object's variant.
pub fn enum_field(index: usize) -> u32 {
    WORD * (2 + index as u32)
}

/// Whether `decl` is an enum whose every variant has no fields: its
/// values are their variant's index, not objects.
pub fn is_fieldless_enum(decl: &TypeDecl) -> bool {
    match &decl.kind {
        TypeDeclKind::Enum(variants) => variants.iter().all(|v| v.fields.is_empty()),
        TypeDeclKind::Struct(_) => false,
    }
}

/// The index of `variant` in the enum `decl`.
pub fn variant_index(decl: &TypeDecl, variant: &str) -> Option<usize> {
    match &decl.kind {
        TypeDeclKind::Enum(variants) => variants.iter().position(|v| v.name == variant),
        TypeDeclKind::Struct(_) => None,
    }
}

/// The read-only data being laid out: static objects, each starting at a
/// multiple of 8. Equal text literals share one object.
#[derive(Debug, Default)]
pub struct Rodata {
    bytes: Vec<u8>,
    texts: BTreeMap<String, u32>,
}

impl Rodata {
    /// The offset of a static text object holding `text`.
    pub fn text(&mut self, text: &str) -> u32 {
        if let Some(&at) = self.texts.get(text) {
            return at;
        }
        let at = self.start();
        self.word(STATIC);
        self.word(text.len() as i64);
        self.bytes.extend_from_slice(text.as_bytes());
        self.texts.insert(text.to_string(), at);
        at
    }

    /// The offset of a static object of `words` after its count.
    pub fn object(&mut self, words: &[i64]) -> u32 {
        let at = self.start();
        self.word(STATIC);
        for &word in words {
            self.word(word);
        }
        at
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    fn start(&mut self) -> u32 {
        while self.bytes.len() % WORD as usize != 0 {
            self.bytes.push(0);
        }
        self.bytes.len() as u32
    }

    fn word(&mut self, word: i64) {
        self.bytes.extend_from_slice(&word.to_le_bytes());
    }
}
