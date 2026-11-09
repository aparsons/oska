//! The checker's type representation, used while checking bodies.
//!
//! Signatures are lowered from syntax into [`Ty`] once (see `items.rs`);
//! bodies are then checked against them with unification (`infer.rs`).

use std::fmt;

/// A type, as the body checker sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    /// A named type with its arguments: a primitive (`Int`, `String`), a
    /// prelude or user struct or enum, or a built-in capability.
    Named(String, Vec<Ty>),
    /// `()`.
    Unit,
    /// `fn(T) -> U`, or `cap fn(T) -> U` when `cap` is set.
    Fn {
        cap: bool,
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

/// The integer types and their ranges.
const INTEGERS: &[(&str, i128, i128)] = &[
    ("Int", i64::MIN as i128, i64::MAX as i128),
    ("I8", i8::MIN as i128, i8::MAX as i128),
    ("I16", i16::MIN as i128, i16::MAX as i128),
    ("I32", i32::MIN as i128, i32::MAX as i128),
    ("I64", i64::MIN as i128, i64::MAX as i128),
    ("U8", 0, u8::MAX as i128),
    ("U16", 0, u16::MAX as i128),
    ("U32", 0, u32::MAX as i128),
    ("U64", 0, u64::MAX as i128),
];

impl Ty {
    /// A named type without arguments, such as `Int`.
    pub fn named(name: &str) -> Ty {
        Ty::Named(name.to_string(), Vec::new())
    }

    pub fn int() -> Ty {
        Ty::named("Int")
    }

    pub fn bool() -> Ty {
        Ty::named("Bool")
    }

    pub fn string() -> Ty {
        Ty::named("String")
    }

    /// The name of a named type, such as `List` for `List<Int>`.
    pub fn head(&self) -> Option<&str> {
        match self {
            Ty::Named(name, _) => Some(name),
            _ => None,
        }
    }

    /// The inclusive range of an integer type, or `None` for any other.
    pub fn integer_range(&self) -> Option<(i128, i128)> {
        let name = self.head()?;
        INTEGERS
            .iter()
            .find(|(n, _, _)| *n == name)
            .map(|&(_, min, max)| (min, max))
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

    /// Contains an unsolved inference variable.
    pub fn has_vars(&self) -> bool {
        match self {
            Ty::Var(_) => true,
            Ty::Named(_, args) => args.iter().any(Ty::has_vars),
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
            Ty::Named(name, args) => Ty::Named(
                name.clone(),
                args.iter().map(|a| a.substitute(subst)).collect(),
            ),
            Ty::Fn { cap, params, ret } => Ty::Fn {
                cap: *cap,
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
            Ty::Named(name, args) => {
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
                Ok(())
            }
            Ty::Unit => write!(f, "()"),
            Ty::Fn { cap, params, ret } => {
                if *cap {
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
