//! Runtime values.
//!
//! Values are reference-counted, so copying one is cheap (ADR 0001).
//! Trust exists only at compile time: an `Untrusted<String>` is a plain
//! [`Value::Str`] here.

use std::rc::Rc;

/// A runtime value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Unit,
    Str(Rc<str>),
    /// An instance of a struct, by type name, with its fields in
    /// declaration order.
    Struct {
        ty: Rc<str>,
        fields: Rc<[Value]>,
    },
    /// A variant of an enum, with its fields in declaration order.
    Variant {
        ty: Rc<str>,
        variant: Rc<str>,
        fields: Rc<[Value]>,
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
            fields: Rc::from(fields),
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
            fields: Rc::from(vec![Value::str(message)]),
        }
    }
}
