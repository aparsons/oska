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
//!   - an enum with fields: `[count][variant index][field 0]...`;
//!   - a list: `[count][length][capacity][item 0][item 1]...`, room for
//!     `capacity` items of which the first `length` are there.
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
//! release drops the count to zero, the object's fields that hold
//! references are released first, by a drop function generated for its
//! type, and then `std.rt`'s `free` returns the memory.
//!
//! An object is never changed while it is shared: writing a field of a
//! struct whose count is not 1 (shared, or static) first copies it, as
//! the interpreter's `Rc::make_mut` does, so a changed copy never changes
//! the original (ADR 0001). Enum objects are never written after they
//! are built. The generic types of a struct or enum lay out the same for
//! every type argument: every field is one word.
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
/// the code keeps: text, lists, and structs and enums with fields.
/// Integers, `Bool`, `()`, fieldless enums and capabilities are not. The
/// other built-in types with a handle (`Range`, `Task`, `Scope`) get their
/// representation with their tickets and are not counted until then.
pub fn is_counted(program: &Program, ty: &Type) -> bool {
    match ty {
        Type::String => true,
        Type::Named { name, .. } if name == builtins::LIST => true,
        Type::Named { name, .. } => {
            !builtins::is_capability(name)
                && !matches!(name.as_str(), "Range" | "Task" | "Scope" | "Args")
                && program
                    .types
                    .get(name)
                    .is_some_and(|decl| !is_fieldless_enum(decl))
        }
        _ => false,
    }
}

/// The type of the items of `ty`, when it is a list.
pub fn list_item(ty: &Type) -> Option<&Type> {
    match ty {
        Type::Named { name, args } if name == builtins::LIST => args.first(),
        _ => None,
    }
}

/// The offset of a list object's length.
pub const LIST_LEN: u32 = WORD;

/// The offset of a list object's capacity.
pub const LIST_CAPACITY: u32 = 2 * WORD;

/// The offset of a list object's first item.
pub const LIST_ITEMS: u32 = 3 * WORD;

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

/// The fields of an object of a concrete struct or enum type, their
/// types with the type's arguments substituted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fields {
    /// A struct's fields, in order.
    Struct(Vec<Type>),
    /// Each variant's fields, by variant index.
    Enum(Vec<Vec<Type>>),
}

impl Fields {
    /// Every field type of every variant.
    pub fn all(&self) -> impl Iterator<Item = &Type> {
        let (one, many) = match self {
            Fields::Struct(fields) => (Some(fields), None),
            Fields::Enum(variants) => (None, Some(variants)),
        };
        one.into_iter()
            .flatten()
            .chain(many.into_iter().flatten().flatten())
    }
}

/// The fields of `ty`, a struct or enum of the program with its type
/// arguments, as its objects hold them. `None` for any other type.
pub fn fields(program: &Program, ty: &Type) -> Option<Fields> {
    let Type::Named { name, args } = ty else {
        return None;
    };
    // The prelude declares `List` with a handle field; its objects are
    // laid out as lists.
    if name == builtins::LIST {
        return None;
    }
    let decl = program.types.get(name)?;
    let subst: Vec<(String, Type)> = decl.generics.iter().cloned().zip(args.clone()).collect();
    let types = |fields: &[oska_check::program::Field]| -> Vec<Type> {
        fields.iter().map(|f| f.ty.substitute(&subst)).collect()
    };
    Some(match &decl.kind {
        TypeDeclKind::Struct(fields) => Fields::Struct(types(fields)),
        TypeDeclKind::Enum(variants) => {
            Fields::Enum(variants.iter().map(|v| types(&v.fields)).collect())
        }
    })
}

/// Whether releasing the last reference to an object of `ty` must
/// release references its fields hold before freeing it.
pub fn holds_counted(program: &Program, ty: &Type) -> bool {
    if let Some(item) = list_item(ty) {
        return is_counted(program, item);
    }
    is_counted(program, ty)
        && fields(program, ty).is_some_and(|f| f.all().any(|t| is_counted(program, t)))
}

/// The bytes of a struct object with `fields` fields, its count included.
pub fn struct_size(fields: usize) -> i64 {
    i64::from(WORD) * (1 + fields as i64)
}

/// The bytes of an enum object whose variant has `fields` fields, its
/// count and variant index included.
pub fn variant_size(fields: usize) -> i64 {
    i64::from(WORD) * (2 + fields as i64)
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
    lists: BTreeMap<Vec<i64>, u32>,
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

    /// The offset of a static list object holding `items`.
    /// Equal lists share one object.
    pub fn list(&mut self, items: &[i64]) -> u32 {
        if let Some(&at) = self.lists.get(items) {
            return at;
        }
        let length = items.len() as i64;
        let mut words = vec![length, length];
        words.extend_from_slice(items);
        let at = self.object(&words);
        self.lists.insert(items.to_vec(), at);
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
