//! Runtime values.
//!
//! Values have value semantics with reference counting (ADR 0001):
//!
//! - Scalars (`Unit`, `Bool`, `Int`, ranges, capabilities) are copied.
//! - Text and compound values sit behind an [`Rc`], so copying one, into
//!   a binding, an argument, or a field, is a reference-count increment.
//! - Writing goes through a slot that owns its value. Assigning a slot
//!   replaces its value; the old one stays alive wherever else it was
//!   copied. Writing inside a compound value goes through
//!   [`Rc::make_mut`], which writes in place when the slot holds the only
//!   reference and copies the value first when it is shared, so changing
//!   a copy never changes the original.
//!
//! Every value carries its runtime type: the variant, and for structs and
//! enums the type's name. Trust exists only at compile time: an
//! `Untrusted<String>` is a plain [`Value::Str`] here.

use std::rc::Rc;

use oska_check::program::Construct;

/// A runtime value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Unit,
    Bool(bool),
    /// An integer of any type: every integer type fits `i128`. Arithmetic
    /// checks each result against its type (`ops.rs`).
    Int(i128),
    Str(Rc<str>),
    /// `start..end`, excluding `end`.
    Range {
        start: i128,
        end: i128,
    },
    /// An instance of a struct, by type name, with its fields in
    /// declaration order.
    Struct {
        ty: Rc<str>,
        fields: Rc<Vec<Value>>,
    },
    /// A variant of an enum, with its fields in declaration order.
    Variant {
        ty: Rc<str>,
        variant: Rc<str>,
        fields: Rc<Vec<Value>>,
    },
    /// A built-in capability, carrying its attenuation state.
    Capability(Capability),
}

/// A built-in capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Stdout,
}

impl Capability {
    /// The capability `main` declares with the type named `ty`, if it has
    /// a runtime.
    pub fn granted(ty: &str) -> Option<Capability> {
        match ty {
            "Stdout" => Some(Capability::Stdout),
            _ => None,
        }
    }
}

impl Value {
    pub fn str(text: &str) -> Value {
        Value::Str(Rc::from(text))
    }

    pub fn variant(ty: &str, variant: &str, fields: Vec<Value>) -> Value {
        Value::Variant {
            ty: Rc::from(ty),
            variant: Rc::from(variant),
            fields: Rc::new(fields),
        }
    }

    /// What `ctor` builds from `fields`.
    pub fn construct(ctor: &Construct, fields: Vec<Value>) -> Value {
        match &ctor.variant {
            Some(variant) => Value::variant(&ctor.ty, variant, fields),
            None => Value::Struct {
                ty: Rc::from(ctor.ty.as_str()),
                fields: Rc::new(fields),
            },
        }
    }

    /// Field `index` of the struct the checker guarantees this is.
    pub fn field(&self, index: usize) -> Value {
        match self {
            Value::Struct { fields, .. } => fields[index].clone(),
            other => unreachable!("the checker guarantees a struct, found {other:?}"),
        }
    }

    pub fn ok(value: Value) -> Value {
        Value::variant("Result", "Ok", vec![value])
    }

    pub fn err(value: Value) -> Value {
        Value::variant("Result", "Err", vec![value])
    }

    /// An `IoError` with `message`.
    pub fn io_error(message: &str) -> Value {
        Value::Struct {
            ty: Rc::from("IoError"),
            fields: Rc::new(vec![Value::str(message)]),
        }
    }

    /// The `Bool` the checker guarantees this is.
    pub fn as_bool(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            other => unreachable!("the checker guarantees a `Bool`, found {other:?}"),
        }
    }

    /// The integer the checker guarantees this is.
    pub fn as_int(&self) -> i128 {
        match self {
            Value::Int(n) => *n,
            other => unreachable!("the checker guarantees an integer, found {other:?}"),
        }
    }

    /// The text the checker guarantees this is.
    pub fn as_str(&self) -> &str {
        match self {
            Value::Str(s) => s,
            other => unreachable!("the checker guarantees a string, found {other:?}"),
        }
    }
}
