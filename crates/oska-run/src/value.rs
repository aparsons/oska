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

use crate::host::RootHandle;
use std::rc::Rc;
use std::sync::Arc;

use oska_check::builtins;
use oska_check::program::{Callee, Closure, Construct, IntType};

/// A runtime value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Unit,
    Bool(bool),
    /// An integer, with its type: every integer type fits `i128`.
    /// Arithmetic checks each result against its type (`ops.rs`), and the
    /// type is the value's runtime type tag, so generic code dispatches a
    /// `U8` to `U8`'s impl.
    Int(i128, IntType),
    Str(Rc<str>),
    /// `start..end`, excluding `end`, over integers of type `int`.
    Range {
        start: i128,
        end: i128,
        int: IntType,
    },
    /// A closure, with what it captured.
    Closure(Rc<ClosureValue>),
    /// A named function used as a value: calling it calls `callee`.
    Function(Rc<Callee>),
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

/// A closure value: its code, and what it captured where it was made, by
/// value: the slots its code lists in [`Closure::captures`], in that
/// order, and the receiver when it reads `self`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosureValue {
    pub code: Arc<Closure>,
    pub captured: Vec<Value>,
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
    /// `Fs`, `FsRead`, or `FsWrite`, with its attenuation state: its
    /// root is always [`Root::Anywhere`], since `restrict_to` gives a
    /// directory capability.
    Fs(FsCap),
    /// A directory capability, `Dir`, `DirRead`, or `DirWrite`: its root
    /// is never [`Root::Anywhere`]. Only attenuation makes one.
    Dir(FsCap),
    /// A file capability, `FileRead` or `FileWrite`: its root is a single
    /// file, or nowhere, and never [`Root::Anywhere`]. It reads or writes
    /// but not both. Only attenuation makes one.
    File(FsCap),
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
    /// Anywhere the process may: the `Fs` `main` declares. It only attenuates:
    /// a directory or file capability never has this root.
    Anywhere,
    /// Only beneath this directory, or only this file, as the host opened
    /// it: the root of a directory capability is a directory, and the
    /// root of a file capability is a file, its parent's handle with its
    /// name. On Unix it holds an OS handle, released with the last copy.
    Dir(RootHandle),
    /// Nowhere: the result of restricting to a root that could not be
    /// opened, because it is missing, a symlink, or outside the current
    /// root. Every operation fails with this reason, which names the root.
    Nowhere(Rc<str>),
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

/// What a capability is, apart from a filesystem's root: what decides
/// the name of its type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Stdin,
    Stdout,
    Stderr,
    Env,
    Clock,
    Random,
    Fs(Access),
    Dir(Access),
    File(Access),
}

/// Each capability the runtime implements, by the name of its type: the
/// one place the runtime names them. A built-in capability the checker
/// knows ([`builtins::capabilities`]) that is missing here has no runtime
/// yet, and a program given one is refused (`E0370`).
const IMPLEMENTED: &[(&str, Kind)] = &[
    ("Stdin", Kind::Stdin),
    ("Stdout", Kind::Stdout),
    ("Stderr", Kind::Stderr),
    ("Env", Kind::Env),
    ("Clock", Kind::Clock),
    ("Random", Kind::Random),
    ("Fs", Kind::Fs(Access::ReadWrite)),
    ("FsRead", Kind::Fs(Access::Read)),
    ("FsWrite", Kind::Fs(Access::Write)),
    ("Dir", Kind::Dir(Access::ReadWrite)),
    ("DirRead", Kind::Dir(Access::Read)),
    ("DirWrite", Kind::Dir(Access::Write)),
    ("FileRead", Kind::File(Access::Read)),
    ("FileWrite", Kind::File(Access::Write)),
];

impl Capability {
    /// Whether the capability named `ty` has a runtime: its methods have
    /// intrinsics, whether or not `main` can be granted one.
    pub fn implemented(ty: &str) -> bool {
        IMPLEMENTED.iter().any(|(name, _)| *name == ty)
    }

    /// The capability `main` declares with the type named `ty`, if the
    /// runtime can grant it: `Fs`, `FsRead` or `FsWrite`. A directory or
    /// file capability has a runtime but is never granted, since only
    /// attenuation gives it a root.
    pub fn granted(ty: &str) -> Option<Capability> {
        let (_, kind) = IMPLEMENTED.iter().find(|(name, _)| *name == ty)?;
        Some(match *kind {
            Kind::Stdin => Capability::Stdin,
            Kind::Stdout => Capability::Stdout,
            Kind::Stderr => Capability::Stderr,
            Kind::Env => Capability::Env,
            Kind::Clock => Capability::Clock,
            Kind::Random => Capability::Random,
            Kind::Fs(access) => Capability::Fs(FsCap {
                root: Root::Anywhere,
                access,
            }),
            Kind::Dir(_) | Kind::File(_) => return None,
        })
    }

    fn kind(&self) -> Kind {
        match self {
            Capability::Stdin => Kind::Stdin,
            Capability::Stdout => Kind::Stdout,
            Capability::Stderr => Kind::Stderr,
            Capability::Env => Kind::Env,
            Capability::Clock => Kind::Clock,
            Capability::Random => Kind::Random,
            Capability::Fs(fs) => Kind::Fs(fs.access),
            Capability::Dir(dir) => Kind::Dir(dir.access),
            Capability::File(file) => Kind::File(file.access),
        }
    }

    /// The name of its type, such as `FsRead` for a read-only filesystem.
    pub fn type_name(&self) -> &'static str {
        let kind = self.kind();
        IMPLEMENTED
            .iter()
            .find(|(_, k)| *k == kind)
            .map(|(name, _)| *name)
            .expect("every capability the runtime makes is in IMPLEMENTED")
    }
}

impl Value {
    /// The runtime type tag a trait call in generic code dispatches on:
    /// the name of the value's type without its arguments, such as
    /// `Point`, `List`, or `String`.
    ///
    /// An integer's tag is its own type's name, `Int` or a sized one
    /// such as `U8`.
    pub fn type_tag(&self) -> &str {
        match self {
            Value::Unit => "()",
            Value::Bool(_) => builtins::BOOL,
            Value::Int(_, int) => int.name(),
            Value::Str(_) => builtins::STRING,
            Value::Range { .. } => builtins::RANGE,
            // No impl can name either, but generic code can tell them
            // apart.
            Value::Closure(_) => "closure",
            Value::Function(_) => "fn",
            Value::List(_) => builtins::LIST,
            Value::Struct { ty, .. } | Value::Variant { ty, .. } => ty,
            Value::Capability(capability) => capability.type_name(),
            Value::Args(_) => builtins::ARGS,
            Value::Scope(_) => builtins::SCOPE,
            Value::Task(_) => builtins::TASK,
        }
    }

    /// `n`, an `Int`.
    pub fn int(n: i128) -> Value {
        Value::Int(n, IntType::Int)
    }

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
        Value::variant(builtins::OPTION, builtins::SOME, vec![value])
    }

    pub fn none() -> Value {
        Value::variant(builtins::OPTION, builtins::NONE, Vec::new())
    }

    /// `Option` from a Rust option.
    pub fn option(value: Option<Value>) -> Value {
        value.map_or_else(Value::none, Value::some)
    }

    /// A `Secret` wrapping `value`. It stays a wrapper at runtime, so
    /// `dbg` can redact it.
    pub fn secret(value: Value) -> Value {
        Value::Struct {
            ty: Rc::from(builtins::SECRET),
            fields: Rc::new(vec![value]),
        }
    }

    /// A `Path` holding `raw`.
    pub fn path(raw: Value) -> Value {
        Value::Struct {
            ty: Rc::from(builtins::PATH),
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
        Value::variant(builtins::RESULT, builtins::OK, vec![value])
    }

    pub fn err(value: Value) -> Value {
        Value::variant(builtins::RESULT, builtins::ERR, vec![value])
    }

    /// An `IoError` with `message`.
    pub fn io_error(message: &str) -> Value {
        Value::Struct {
            ty: Rc::from(builtins::IO_ERROR),
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
            Value::Int(n, _) => *n,
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

#[cfg(test)]
mod tests {
    use std::rc::Rc;
    use std::sync::Arc;

    use oska_check::builtins;
    use oska_check::program::{Callee, Closure, Expr, ExprKind};
    use oska_syntax::Span;

    use super::{Capability, ClosureValue, FsCap, Kind, Root, Value, IMPLEMENTED};

    #[test]
    fn a_closure_and_a_named_function_have_different_tags() {
        let code = Arc::new(Closure {
            params: Vec::new(),
            captures: Vec::new(),
            captures_self: false,
            height: 0,
            body: Expr {
                kind: ExprKind::Unit,
                span: Span { start: 0, end: 0 },
            },
        });
        let closure = Value::Closure(Rc::new(ClosureValue {
            code,
            captured: Vec::new(),
            receiver: None,
        }));
        let function = Value::Function(Rc::new(Callee::Function("f".to_string())));
        assert_eq!(closure.type_tag(), "closure");
        assert_eq!(function.type_tag(), "fn");
    }

    #[test]
    fn every_capability_the_runtime_implements_is_one_the_checker_knows() {
        for (name, kind) in IMPLEMENTED {
            assert!(
                builtins::is_capability(name),
                "`{name}` is not a capability"
            );
            let capability = match *kind {
                Kind::Dir(access) => Capability::Dir(FsCap {
                    root: Root::Nowhere(Rc::from("no root")),
                    access,
                }),
                Kind::File(access) => Capability::File(FsCap {
                    root: Root::Nowhere(Rc::from("no root")),
                    access,
                }),
                _ => Capability::granted(name).expect("granted"),
            };
            assert_eq!(capability.type_name(), *name);
        }
    }

    #[test]
    fn a_directory_or_file_capability_is_never_granted() {
        for name in ["Dir", "DirRead", "DirWrite", "FileRead", "FileWrite"] {
            assert!(Capability::implemented(name));
            assert_eq!(Capability::granted(name), None);
        }
    }
}
