//! The checker's type representation, used while checking bodies.
//!
//! Signatures are lowered from syntax into [`Ty`] once (see `items.rs`);
//! bodies are then checked against them with unification (`infer.rs`).

use std::fmt;

use crate::program::IntType;

/// A type, as the body checker sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    /// A named type with its arguments: a primitive (`Int`, `String`), a
    /// prelude or user struct or enum, or a built-in capability. Its
    /// [`Trust`] matters only for text (see [`Ty::is_text`]); every other
    /// type is always [`Trust::Trusted`].
    Named(String, Vec<Ty>, Trust),
    /// `()`.
    Unit,
    /// `fn(T) -> U`, or `cap fn(T) -> U` when `cap` is [`Cap::Bearing`].
    ///
    /// `scoped` marks a closure that captures a `Scope` handle, or a value
    /// holding one: like the handle, it cannot outlive its `scope` block.
    /// It has no syntax. A slot whose type is written in a signature or a
    /// field is unscoped, except a function-typed parameter of a prelude
    /// or `std` function, which accepts a scoped closure (see
    /// [`crate::items::Lower::signature`]).
    Fn {
        cap: Cap,
        scoped: bool,
        params: Vec<Ty>,
        ret: Box<Ty>,
    },
    /// A generic parameter of the enclosing declaration, or `Self` inside
    /// a trait. Rigid: it only unifies with itself.
    Param(String),
    /// An inference variable, solved by unification within one body.
    Var(u32),
    /// The type of an expression that never produces a value, such as a
    /// block ending in `return`. It fits wherever a value is expected.
    Never,
    /// The type of an expression that failed to check. It unifies with
    /// everything and suppresses follow-on errors.
    Error,
}

/// The trust flag on a text type (ADR 0006). `Untrusted<String>` is
/// written syntax for an untrusted `String`, not a generic type. Trusted
/// text may flow into an untrusted slot; untrusted text never flows into
/// a trusted one (see `infer.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Trust {
    #[default]
    Trusted,
    Untrusted,
    /// A **trust variable**, solved within one body (see `infer.rs`). It
    /// stands for the join of the trust of everything that flows into it:
    /// untrusted as soon as any untrusted text does. Inference variables
    /// and trust-polymorphic prelude operations introduce them.
    Var(u32),
}

/// The capability flag on a function type: a plain `fn` may flow into a
/// `cap fn` slot; a `cap fn` never flows into a plain `fn` one (see
/// `infer.rs`). Inference treats it like [`Trust`], so a plain `fn` and a
/// `cap fn` meeting in one position join to `cap fn`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Cap {
    /// A plain `fn`: pure.
    #[default]
    Plain,
    /// A `cap fn`: it may carry authority.
    Bearing,
    /// A **capability variable**, solved within one body (see
    /// `infer.rs`). It stands for the join of everything that flows into
    /// it: a `cap fn` as soon as any `cap fn` does. Inference variables
    /// introduce them.
    Var(u32),
}

impl Cap {
    /// `Bearing` when `bearing`, otherwise `Plain`.
    pub fn of(bearing: bool) -> Cap {
        if bearing {
            Cap::Bearing
        } else {
            Cap::Plain
        }
    }
}

// Prelude types and traits the checker gives special treatment, by name.
// Every comparison with one of these names goes through a constant here.

pub const BOOL: &str = "Bool";
pub const INT: &str = "Int";
pub const STRING: &str = "String";
/// The syntax for untrusted text, as in `Untrusted<String>`.
pub const UNTRUSTED: &str = "Untrusted";
/// The types that carry a trust flag.
const TEXT: &[&str] = &[STRING];

/// Whether the type named `name` carries a trust flag.
pub fn is_text_name(name: &str) -> bool {
    TEXT.contains(&name)
}
pub const OPTION: &str = "Option";
pub const RESULT: &str = "Result";
pub const LIST: &str = "List";
pub const RANGE: &str = "Range";
/// The only non-capability type `main` may take.
pub const ARGS: &str = "Args";
/// The handle `scope |s| { ... }` binds, which cannot outlive its block.
pub const SCOPE: &str = "Scope";
/// The type of `t` in a test body.
pub const TEST_CONTEXT: &str = "TestContext";
/// `==` and `!=` need it.
pub const EQ: &str = "Eq";
/// `?` converts errors with it.
pub const FROM: &str = "From";
/// `main`'s error type needs it.
pub const DISPLAY: &str = "Display";
/// A secret value: no `Display`, no `Eq`, no interpolation.
pub const SECRET: &str = "Secret";
/// The name of `Self`, inside a trait or `impl`.
pub const SELF: &str = "Self";

impl Ty {
    /// A named type without arguments, such as `Int`.
    pub fn named(name: &str) -> Ty {
        Ty::Named(name.to_string(), Vec::new(), Trust::Trusted)
    }

    /// A named type with arguments, such as `Result<T, E>`. Trusted.
    pub fn generic(name: &str, args: Vec<Ty>) -> Ty {
        Ty::Named(name.to_string(), args, Trust::Trusted)
    }

    pub fn int() -> Ty {
        Ty::named(INT)
    }

    pub fn bool() -> Ty {
        Ty::named(BOOL)
    }

    /// Trusted `String`.
    pub fn string() -> Ty {
        Ty::named(STRING)
    }

    /// A type that carries a trust flag.
    pub fn is_text(&self) -> bool {
        matches!(self, Ty::Named(name, _, _) if is_text_name(name))
    }

    /// This text type with trust `trust`; any other type unchanged.
    pub fn with_trust(self, trust: Trust) -> Ty {
        match self {
            Ty::Named(name, args, _) if is_text_name(&name) => Ty::Named(name, args, trust),
            other => other,
        }
    }

    /// This type with the trust flag of every text type in it, at any
    /// depth, replaced by `map` of it. The one walk over trust flags:
    /// type arguments are mapped before the type that holds them.
    pub fn map_trust(&self, map: &mut impl FnMut(Trust) -> Trust) -> Ty {
        match self {
            Ty::Named(name, args, trust) => {
                let args = args.iter().map(|a| a.map_trust(map)).collect();
                let trust = if is_text_name(name) {
                    map(*trust)
                } else {
                    *trust
                };
                Ty::Named(name.clone(), args, trust)
            }
            Ty::Fn {
                cap,
                scoped,
                params,
                ret,
            } => Ty::Fn {
                cap: *cap,
                scoped: *scoped,
                params: params.iter().map(|p| p.map_trust(map)).collect(),
                ret: Box::new(ret.map_trust(map)),
            },
            other => other.clone(),
        }
    }

    /// This type with the capability flag of every function type in it, at
    /// any depth, replaced by `map` of it. The one walk over capability
    /// flags.
    pub fn map_cap(&self, map: &mut impl FnMut(Cap) -> Cap) -> Ty {
        match self {
            Ty::Named(name, args, trust) => Ty::Named(
                name.clone(),
                args.iter().map(|a| a.map_cap(map)).collect(),
                *trust,
            ),
            Ty::Fn {
                cap,
                scoped,
                params,
                ret,
            } => Ty::Fn {
                cap: map(*cap),
                scoped: *scoped,
                params: params.iter().map(|p| p.map_cap(map)).collect(),
                ret: Box::new(ret.map_cap(map)),
            },
            other => other.clone(),
        }
    }

    /// This type with every trusted text flag in it, at any depth,
    /// replaced by `trust`.
    pub fn trusted_as(&self, trust: Trust) -> Ty {
        self.map_trust(&mut |t| match t {
            Trust::Trusted => trust,
            other => other,
        })
    }

    /// Holds a text type that is fixed trusted, at any depth.
    pub fn has_trusted_text(&self) -> bool {
        match self {
            Ty::Named(_, args, trust) => {
                (self.is_text() && *trust == Trust::Trusted)
                    || args.iter().any(Ty::has_trusted_text)
            }
            Ty::Fn { params, ret, .. } => {
                params.iter().any(Ty::has_trusted_text) || ret.has_trusted_text()
            }
            Ty::Unit | Ty::Param(_) | Ty::Var(_) | Ty::Never | Ty::Error => false,
        }
    }

    /// `List<element>`.
    pub fn list(element: Ty) -> Ty {
        Ty::generic(LIST, vec![element])
    }

    /// `Range<element>`.
    pub fn range(element: Ty) -> Ty {
        Ty::generic(RANGE, vec![element])
    }

    /// The arguments of this type when it is the named type `name` with
    /// `arity` arguments, such as `[T, E]` for `Result<T, E>`.
    pub fn args_of(&self, name: &str, arity: usize) -> Option<&[Ty]> {
        match self {
            Ty::Named(n, args, _) if n == name && args.len() == arity => Some(args),
            _ => None,
        }
    }

    /// The name of a named type, such as `List` for `List<Int>`.
    pub fn head(&self) -> Option<&str> {
        match self {
            Ty::Named(name, _, _) => Some(name),
            _ => None,
        }
    }

    /// The integer type this is, or `None` for any other type.
    pub fn integer_type(&self) -> Option<IntType> {
        IntType::named(self.head()?)
    }

    /// The inclusive range of an integer type, or `None` for any other.
    pub fn integer_range(&self) -> Option<(i128, i128)> {
        self.integer_type().map(|t| (t.min(), t.max()))
    }

    pub fn is_integer(&self) -> bool {
        self.integer_range().is_some()
    }

    /// An integer type that can hold negative values.
    pub fn is_signed_integer(&self) -> bool {
        self.integer_range().is_some_and(|(min, _)| min < 0)
    }

    /// Absorbs errors: no diagnostic should be reported about it.
    pub fn is_error(&self) -> bool {
        matches!(self, Ty::Error)
    }

    /// A value of this type may hold a `Scope` handle, so it cannot
    /// outlive its `scope` block: it is a `Scope`, a scoped closure, or
    /// names one in a type argument or a function type's result.
    pub fn holds_scope(&self) -> bool {
        match self {
            Ty::Named(n, args, _) => n == SCOPE || args.iter().any(Ty::holds_scope),
            Ty::Fn { scoped, ret, .. } => *scoped || ret.holds_scope(),
            Ty::Unit | Ty::Param(_) | Ty::Var(_) | Ty::Never | Ty::Error => false,
        }
    }

    /// Contains an unsolved inference variable.
    pub fn has_vars(&self) -> bool {
        match self {
            Ty::Var(_) => true,
            Ty::Named(_, args, _) => args.iter().any(Ty::has_vars),
            Ty::Fn { params, ret, .. } => params.iter().any(Ty::has_vars) || ret.has_vars(),
            Ty::Unit | Ty::Param(_) | Ty::Never | Ty::Error => false,
        }
    }

    /// Replace each generic parameter named in `subst` with its type.
    pub fn substitute(&self, subst: &[(String, Ty)]) -> Ty {
        match self {
            Ty::Param(name) => subst
                .iter()
                .find(|(n, _)| n == name)
                .map_or_else(|| self.clone(), |(_, ty)| ty.clone()),
            Ty::Named(name, args, trust) => Ty::Named(
                name.clone(),
                args.iter().map(|a| a.substitute(subst)).collect(),
                *trust,
            ),
            Ty::Fn {
                cap,
                scoped,
                params,
                ret,
            } => Ty::Fn {
                cap: *cap,
                scoped: *scoped,
                params: params.iter().map(|p| p.substitute(subst)).collect(),
                ret: Box::new(ret.substitute(subst)),
            },
            Ty::Unit | Ty::Var(_) | Ty::Never | Ty::Error => self.clone(),
        }
    }
}

/// Renders a type as it would be written in Oska source. An unsolved
/// inference variable is shown as `_`.
impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ty::Named(name, args, trust) => {
                if *trust == Trust::Untrusted {
                    write!(f, "{UNTRUSTED}<")?;
                }
                write!(f, "{name}")?;
                if !args.is_empty() {
                    write!(f, "<")?;
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{arg}")?;
                    }
                    write!(f, ">")?;
                }
                if *trust == Trust::Untrusted {
                    write!(f, ">")?;
                }
                Ok(())
            }
            Ty::Unit => write!(f, "()"),
            Ty::Fn {
                cap, params, ret, ..
            } => {
                if *cap == Cap::Bearing {
                    write!(f, "cap ")?;
                }
                write!(f, "fn(")?;
                for (i, param) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{param}")?;
                }
                write!(f, ")")?;
                if **ret != Ty::Unit {
                    write!(f, " -> {ret}")?;
                }
                Ok(())
            }
            Ty::Param(name) => write!(f, "{name}"),
            Ty::Var(_) => write!(f, "_"),
            Ty::Never => write!(f, "!"),
            Ty::Error => write!(f, "{{error}}"),
        }
    }
}
