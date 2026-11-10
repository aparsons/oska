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

use std::path::Path;
use std::rc::Rc;

use oska_check::program::{Closure, Construct};

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
    /// A closure, with what it captured.
    Closure(Rc<ClosureValue>),
    /// A `List`'s items.
    List(Rc<Vec<Value>>),
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
    /// The program's command-line arguments, the program path first: a
    /// program input `main` receives, not a capability.
    Args(Rc<[Rc<str>]>),
    /// The handle a `scope` block binds: the index of its scope in the
    /// interpreter (`tasks.rs`). It is only a name for the scope, so it
    /// stays a valid value after the block ends, but no longer a valid
    /// thing to spawn on.
    Scope(usize),
    /// A task `Scope.spawn` returned: the index of its task in the
    /// interpreter (`tasks.rs`).
    Task(usize),
}

/// A closure value: its code, and the slots and receiver in scope where
/// it was made, captured by value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosureValue {
    pub code: Rc<Closure>,
    pub slots: Vec<Value>,
    pub receiver: Option<Value>,
}

/// A built-in capability. Its type in the program (`Fs`, `FsRead`, ...)
/// is the checker's business; at runtime it carries only what decides
/// what it may do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Capability {
    Stdin,
    Stdout,
    Stderr,
    Env,
    Clock,
    Random,
    /// `Fs`, `FsRead`, or `FsWrite`, with its attenuation state.
    Fs(FsCap),
}

/// A filesystem capability's attenuation state: where it may reach, and
/// which operations it may perform. Attenuating produces a new one that
/// is never wider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsCap {
    pub root: Root,
    pub access: Access,
}

/// Where a filesystem capability may reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Root {
    /// Anywhere the process may: the `Fs` `main` declares, unrestricted.
    Anywhere,
    /// Only under this directory: canonical, so absolute and free of
    /// symlinks (`confine.rs`).
    Dir(Rc<Path>),
    /// Nowhere: the result of restricting to a directory that does not
    /// exist or lies outside the current root. Every operation fails.
    Nowhere,
}

/// Which operations a filesystem capability may perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    ReadWrite,
    Read,
    Write,
}

impl Access {
    pub fn reads(self) -> bool {
        matches!(self, Access::ReadWrite | Access::Read)
    }

    pub fn writes(self) -> bool {
        matches!(self, Access::ReadWrite | Access::Write)
    }
}

impl Capability {
    /// The capability `main` declares with the type named `ty`, if it has
    /// a runtime.
    pub fn granted(ty: &str) -> Option<Capability> {
        let fs = |access| {
            Some(Capability::Fs(FsCap {
                root: Root::Anywhere,
                access,
            }))
        };
        match ty {
            "Stdin" => Some(Capability::Stdin),
            "Stdout" => Some(Capability::Stdout),
            "Stderr" => Some(Capability::Stderr),
            "Env" => Some(Capability::Env),
            "Clock" => Some(Capability::Clock),
            "Random" => Some(Capability::Random),
            "Fs" => fs(Access::ReadWrite),
            "FsRead" => fs(Access::Read),
            "FsWrite" => fs(Access::Write),
            _ => None,
        }
    }
}

/// The capabilities the language declares that have no runtime yet. A
/// program whose `main` declares one is refused before it starts
/// (`E0370`).
pub const UNIMPLEMENTED_CAPABILITIES: &[&str] =
    &["Net", "NetConnect", "NetListen", "Spawn", "Sched"];

/// The `std` modules that have no runtime yet. A program that imports one
/// is refused before it starts (`E0371`).
pub const UNIMPLEMENTED_MODULES: &[&str] = &["std.proc", "std.sql", "std.html"];

/// The type name of a runtime `Secret`.
pub const SECRET: &str = "Secret";

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

    pub fn some(value: Value) -> Value {
        Value::variant("Option", "Some", vec![value])
    }

    pub fn none() -> Value {
        Value::variant("Option", "None", Vec::new())
    }

    /// `Option` from a Rust option.
    pub fn option(value: Option<Value>) -> Value {
        value.map_or_else(Value::none, Value::some)
    }

    /// A `Secret` wrapping `value`. It stays a wrapper at runtime, so
    /// `dbg` can redact it.
    pub fn secret(value: Value) -> Value {
        Value::Struct {
            ty: Rc::from(SECRET),
            fields: Rc::new(vec![value]),
        }
    }

    /// A `Path` holding `raw`.
    pub fn path(raw: Value) -> Value {
        Value::Struct {
            ty: Rc::from("Path"),
            fields: Rc::new(vec![raw]),
        }
    }

    /// The fields of the struct or variant the checker guarantees this is.
    pub fn fields(&self) -> &[Value] {
        match self {
            Value::Struct { fields, .. } | Value::Variant { fields, .. } => fields,
            other => unreachable!("the checker guarantees a struct or enum, found {other:?}"),
        }
    }

    /// Whether this is the variant named `name`.
    pub fn is_variant(&self, name: &str) -> bool {
        matches!(self, Value::Variant { variant, .. } if &**variant == name)
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

    /// The items of the `List` the checker guarantees this is.
    pub fn as_list(&self) -> &Rc<Vec<Value>> {
        match self {
            Value::List(items) => items,
            other => unreachable!("the checker guarantees a list, found {other:?}"),
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
