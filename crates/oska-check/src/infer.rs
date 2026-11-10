//! Unification: the inference context for one function body.
//!
//! Inference variables are created with [`Infer::fresh`] and solved by
//! [`Infer::unify`]. Nothing is shared between bodies, so inference never
//! crosses a function boundary.
//!
//! Unification is directional: a value of the `actual` type flows where
//! the `expected` type is required. The types must be equal except for
//! three one-way **flags**, which unification enforces itself so that no
//! route a value can take loses them:
//!
//! - the **`cap` rule**: a `cap fn` never becomes a plain `fn`;
//! - the **scope rule**: a scoped closure, which captures a `Scope`
//!   handle, never becomes an unscoped `fn`, so it cannot reach a slot
//!   that outlives its `scope` block;
//! - the **trust rule** (ADR 0006): untrusted text never becomes trusted.
//!
//! The `cap` rule and the trust rule are solved alike, as one-bit
//! subtyping problems (see `infer/flags.rs`): plain `fn` ≤ `cap fn`, and
//! trusted ≤ untrusted. Besides its two fixed values, each flag may be a
//! variable: a **capability variable** ([`Cap::Var`]) on a function type,
//! a **trust variable** ([`Trust::Var`]) on a text type. Each flow between
//! flags is an edge `a ≤ b`. A variable is high (`cap fn`, untrusted) once
//! a high flag reaches it through the edges, and pinned low once it
//! reaches a low slot (a plain `fn`, trusted text); one that would be both
//! is a [`Mismatch::CapLost`] or a [`Mismatch::TrustLost`]. Edges are
//! kept, so a value that turns out high later in the body still reaches
//! every slot it flowed into.
//!
//! Flag variables give two things:
//!
//! - **Joins.** When an inference variable is solved, every fixed flag in
//!   its solution becomes a fresh variable that the solving type flows
//!   into (or out of). So a variable first met with a plain `fn` still
//!   accepts a `cap fn` later, and becomes a `cap fn`; one first met with
//!   trusted text accepts untrusted text, and becomes untrusted. The join
//!   is the high flag. A variable that has already flowed into a low slot
//!   rejects it. Joins therefore happen wherever values meet in an
//!   inference variable: list elements, `if` and `match` branches, the
//!   arguments of a generic for one type parameter, and later
//!   assignments to a `var` whose type was inferred.
//! - **Trust polymorphism.** A `#[follows_trust]` prelude operation gets
//!   one fresh trust variable for each call, in place of every trusted
//!   text flag in its signature (see `body/calls.rs`). The receiver and
//!   the text arguments flow into it, and every text type in the result
//!   is it.
//!
//! A check that must decide now whether a type is capability-bearing
//! reads an open capability flag as a plain `fn` and settles it so
//! ([`Infer::settle_caps`]): the variable is pinned, and a `cap fn` that
//! reaches it later is rejected rather than slipping past a decision
//! already made. A contract's purity is decided this way. A closure is
//! not: one whose captures are still open ([`Infer::is_open`]) gets a
//! fresh capability variable, raised at the end of the body if a capture
//! turned out capability-bearing ([`Infer::raise_cap`]).

mod flags;

use std::collections::HashSet;

use crate::ty::{Cap, Trust, Ty};
use flags::{Flag, Flags};

/// Why a value cannot flow where a type is expected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mismatch {
    /// The types differ.
    Types,
    /// The types match, except that a `cap fn` would become a plain `fn`.
    CapLost,
    /// The types match, except that a scoped closure would become an
    /// unscoped `fn`.
    ScopeLost,
    /// The types match, except that untrusted text would become trusted.
    TrustLost,
}

/// Which way values flow at a position inside a type: the same way as the
/// whole (`Positive`), or against it (`Negative`, inside a function
/// type's parameters).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Polarity {
    Positive,
    Negative,
}

impl Polarity {
    fn flip(self) -> Polarity {
        match self {
            Polarity::Positive => Polarity::Negative,
            Polarity::Negative => Polarity::Positive,
        }
    }

    /// Whether the scope flag survives: `a` and `b` say whether each side
    /// is scoped. A scoped closure never flows into an unscoped slot.
    fn keeps(self, a: bool, b: bool) -> bool {
        match self {
            Polarity::Positive => !a || b,
            Polarity::Negative => !b || a,
        }
    }

    /// The flags `a` and `b`, in the order values flow between them.
    fn order<T>(self, a: T, b: T) -> (T, T) {
        match self {
            Polarity::Positive => (a, b),
            Polarity::Negative => (b, a),
        }
    }
}

/// Which one-way flags a unification enforces. [`Infer::flow`] retries
/// with one turned off to find out which flag was the only obstacle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rules {
    cap: bool,
    scope: bool,
    trust: bool,
}

#[derive(Debug, Default, Clone)]
pub struct Infer {
    /// The solution of each variable, by index.
    solutions: Vec<Option<Ty>>,
    /// The trust variables.
    trust: Flags,
    /// The capability variables.
    caps: Flags,
}

fn trust_flag(trust: Trust) -> Flag {
    match trust {
        Trust::Trusted => Flag::Low,
        Trust::Untrusted => Flag::High,
        Trust::Var(v) => Flag::Var(v),
    }
}

fn cap_flag(cap: Cap) -> Flag {
    match cap {
        Cap::Plain => Flag::Low,
        Cap::Bearing => Flag::High,
        Cap::Var(v) => Flag::Var(v),
    }
}

impl Infer {
    /// A new, unsolved inference variable.
    pub fn fresh(&mut self) -> Ty {
        self.solutions.push(None);
        Ty::Var(self.solutions.len() as u32 - 1)
    }

    /// A new trust variable, with nothing flowing into or out of it.
    pub fn fresh_trust(&mut self) -> Trust {
        Trust::Var(self.trust.fresh())
    }

    /// `ty` with every text flag replaced by a fresh trust variable, so
    /// that it accepts text of either trust. Unsolved variables inside
    /// are kept.
    pub fn loosen(&mut self, ty: &Ty) -> Ty {
        self.refresh(ty, true)
    }

    /// `ty` with every fixed text and capability flag replaced by a fresh
    /// variable, and with its flag variables kept. An inference variable
    /// is solved to this: it can join with text of other trust and with
    /// functions of the other capability, while a value it shares a flag
    /// variable with (such as the elements of a list it was taken from)
    /// still sees everything that flows into it later.
    fn generalize(&mut self, ty: &Ty) -> Ty {
        self.refresh(ty, false).map_cap(&mut |cap| match cap {
            Cap::Var(_) => cap,
            Cap::Plain | Cap::Bearing => Cap::Var(self.caps.fresh()),
        })
    }

    /// Whether `ty`, as solved so far, may still change: it holds an
    /// unsolved inference variable or a capability variable that is not
    /// yet a `cap fn`.
    pub fn is_open(&self, ty: &Ty) -> bool {
        let ty = self.resolve(ty);
        let mut open = ty.has_vars();
        ty.map_cap(&mut |cap| {
            open |= matches!(cap, Cap::Var(_));
            cap
        });
        open
    }

    /// A new capability variable, with nothing flowing into or out of it.
    pub fn fresh_cap(&mut self) -> Cap {
        Cap::Var(self.caps.fresh())
    }

    /// Make `cap` a `cap fn`, with every flag it has flowed into. Fails
    /// when it has reached a plain `fn` slot.
    pub fn raise_cap(&mut self, cap: Cap) -> bool {
        self.caps.flow(Flag::High, cap_flag(cap))
    }

    /// Settle every capability variable in `ty` that is still open as a
    /// plain `fn`, for a check that has just read it as one: a `cap fn`
    /// that reaches it later is rejected. One already a `cap fn` is kept.
    pub fn settle_caps(&mut self, ty: &Ty) {
        self.resolve(ty).map_cap(&mut |cap| {
            if let Cap::Var(v) = cap {
                // Pinning a variable that is not a `cap fn` cannot fail.
                self.caps.flow(Flag::Var(v), Flag::Low);
            }
            cap
        });
    }

    fn refresh(&mut self, ty: &Ty, variables_too: bool) -> Ty {
        ty.map_trust(&mut |trust| match trust {
            Trust::Var(_) if !variables_too => trust,
            _ => self.fresh_trust(),
        })
    }

    /// Let the trust of every text type in a value of type `ty` flow into
    /// `into`, as the holes of an interpolated string do. Function types
    /// are skipped: they are not text. A generic parameter counts as
    /// untrusted text, since it may be instantiated with some: text a
    /// generic body makes from a `T` is untrusted. So does a type in
    /// `holders`, which holds untrusted text in a field (see
    /// [`crate::items::Items::untrusted_holders`]): the text of an
    /// `IoError` is untrusted.
    pub fn join_into(&mut self, ty: &Ty, into: Trust, holders: &HashSet<String>) {
        // Each flow only fails when `into` is pinned, which the caller
        // rules out.
        match self.shallow(ty) {
            Ty::Named(name, args, trust) => {
                self.trust_flow(trust, into);
                if holders.contains(&name) {
                    self.trust_flow(Trust::Untrusted, into);
                }
                for arg in &args {
                    self.join_into(arg, into, holders);
                }
            }
            Ty::Param(_) => {
                self.trust_flow(Trust::Untrusted, into);
            }
            _ => {}
        }
    }

    /// A trust flag as solved so far: a variable is untrusted once
    /// untrusted text reaches it, which is final. Any other variable is
    /// kept, even a pinned one: text that shares it must keep rejecting
    /// untrusted text that arrives later.
    fn trust_of(&self, trust: Trust) -> Trust {
        match trust {
            Trust::Var(v) if self.trust.is_high(v) => Trust::Untrusted,
            other => other,
        }
    }

    /// A capability flag as solved so far: a variable is a `cap fn` once
    /// a `cap fn` reaches it, which is final. Any other variable is kept,
    /// and reads as a plain `fn` (see [`Infer::settle_caps`]).
    fn cap_of(&self, cap: Cap) -> Cap {
        match cap {
            Cap::Var(v) if self.caps.is_high(v) => Cap::Bearing,
            other => other,
        }
    }

    /// Add the edge `a ≤ b`: text of trust `a` flows where `b` is
    /// required. Fails when untrusted text would reach a trusted slot.
    fn trust_flow(&mut self, a: Trust, b: Trust) -> bool {
        self.trust.flow(trust_flag(a), trust_flag(b))
    }

    /// Follow solved variables at the top of `ty` only.
    pub fn shallow(&self, ty: &Ty) -> Ty {
        let mut ty = ty.clone();
        while let Ty::Var(v) = ty {
            match &self.solutions[v as usize] {
                Some(solution) => ty = solution.clone(),
                None => break,
            }
        }
        ty
    }

    /// Replace every solved variable in `ty`, all the way down.
    pub fn resolve(&self, ty: &Ty) -> Ty {
        match self.shallow(ty) {
            Ty::Named(name, args, trust) => Ty::Named(
                name,
                args.iter().map(|a| self.resolve(a)).collect(),
                self.trust_of(trust),
            ),
            Ty::Fn {
                cap,
                scoped,
                params,
                ret,
            } => Ty::Fn {
                cap: self.cap_of(cap),
                scoped,
                params: params.iter().map(|p| self.resolve(p)).collect(),
                ret: Box::new(self.resolve(&ret)),
            },
            other => other,
        }
    }

    /// Let a value of type `actual` flow where `expected` is required,
    /// solving variables as needed. On failure nothing is solved, so the
    /// caller can report one mismatch and carry on. [`Ty::Error`] and
    /// [`Ty::Never`] unify with anything.
    ///
    /// The types must be equal, except for three one-way flags:
    ///
    /// - a `cap fn` can carry authority, so it never flows into a plain
    ///   `fn`, while a plain `fn` may flow into a `cap fn`;
    /// - a scoped closure never flows into an unscoped `fn`;
    /// - untrusted text never flows into trusted text, while trusted text
    ///   may flow into untrusted.
    ///
    /// The rules apply at every depth: covariantly through type arguments
    /// and results, contravariantly through a function type's parameters.
    /// An inference variable is solved to the type it meets, with flag
    /// variables for its flags: one first met with a plain `fn` accepts a
    /// `cap fn` later and becomes one, and one first met with trusted text
    /// accepts untrusted text, unless it has flowed into a plain `fn` or
    /// trusted slot.
    pub fn unify(&mut self, actual: &Ty, expected: &Ty) -> bool {
        self.flow(actual, expected).is_ok()
    }

    /// [`Infer::unify`], saying why it failed.
    pub fn flow(&mut self, actual: &Ty, expected: &Ty) -> Result<(), Mismatch> {
        let attempts = [
            (
                Rules {
                    cap: true,
                    scope: true,
                    trust: true,
                },
                None,
            ),
            (
                Rules {
                    cap: false,
                    scope: true,
                    trust: true,
                },
                Some(Mismatch::CapLost),
            ),
            (
                Rules {
                    cap: true,
                    scope: false,
                    trust: true,
                },
                Some(Mismatch::ScopeLost),
            ),
            (
                Rules {
                    cap: true,
                    scope: true,
                    trust: false,
                },
                Some(Mismatch::TrustLost),
            ),
        ];
        let saved = self.clone();
        for (rules, mismatch) in attempts {
            let ok = self.unify_inner(actual, expected, Polarity::Positive, rules);
            if ok && mismatch.is_none() {
                return Ok(());
            }
            *self = saved.clone();
            if let (true, Some(mismatch)) = (ok, mismatch) {
                return Err(mismatch);
            }
        }
        Err(Mismatch::Types)
    }

    fn unify_inner(&mut self, a: &Ty, b: &Ty, polarity: Polarity, rules: Rules) -> bool {
        let a = self.shallow(a);
        let b = self.shallow(b);
        match (&a, &b) {
            (Ty::Error, _) | (_, Ty::Error) | (Ty::Never, _) | (_, Ty::Never) => true,
            (Ty::Var(x), Ty::Var(y)) if x == y => true,
            (Ty::Var(v), other) => {
                if self.occurs(*v, other) {
                    return false;
                }
                let solution = self.generalize(other);
                self.solutions[*v as usize] = Some(solution.clone());
                self.unify_inner(&solution, other, polarity, rules)
            }
            (other, Ty::Var(v)) => {
                if self.occurs(*v, other) {
                    return false;
                }
                let solution = self.generalize(other);
                self.solutions[*v as usize] = Some(solution.clone());
                self.unify_inner(other, &solution, polarity, rules)
            }
            (Ty::Named(n1, a1, t1), Ty::Named(n2, a2, t2)) => {
                if n1 != n2 || a1.len() != a2.len() {
                    return false;
                }
                let (from, into) = polarity.order(*t1, *t2);
                let trust_kept = !rules.trust || self.trust_flow(from, into);
                trust_kept
                    && a1
                        .iter()
                        .zip(a2)
                        .all(|(x, y)| self.unify_inner(x, y, polarity, rules))
            }
            (Ty::Unit, Ty::Unit) => true,
            (Ty::Param(p), Ty::Param(q)) => p == q,
            (
                Ty::Fn {
                    cap: c1,
                    scoped: s1,
                    params: p1,
                    ret: r1,
                },
                Ty::Fn {
                    cap: c2,
                    scoped: s2,
                    params: p2,
                    ret: r2,
                },
            ) => {
                let (from, into) = polarity.order(cap_flag(*c1), cap_flag(*c2));
                let cap_kept = !rules.cap || self.caps.flow(from, into);
                let scope_kept = !rules.scope || polarity.keeps(*s1, *s2);
                let flipped = polarity.flip();
                cap_kept
                    && scope_kept
                    && p1.len() == p2.len()
                    && p1
                        .iter()
                        .zip(p2)
                        .all(|(x, y)| self.unify_inner(x, y, flipped, rules))
                    && self.unify_inner(r1, r2, polarity, rules)
            }
            _ => false,
        }
    }

    fn occurs(&self, v: u32, ty: &Ty) -> bool {
        match self.shallow(ty) {
            Ty::Var(w) => v == w,
            Ty::Named(_, args, _) => args.iter().any(|a| self.occurs(v, a)),
            Ty::Fn { params, ret, .. } => {
                params.iter().any(|p| self.occurs(v, p)) || self.occurs(v, &ret)
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn func(cap: Cap, params: Vec<Ty>) -> Ty {
        Ty::Fn {
            cap,
            scoped: false,
            params,
            ret: Box::new(Ty::Unit),
        }
    }

    #[test]
    fn the_cap_rule_holds_at_every_depth() {
        let plain = func(Cap::Plain, vec![]);
        let capped = func(Cap::Bearing, vec![]);
        let mut infer = Infer::default();
        assert_eq!(infer.flow(&plain, &capped), Ok(()));
        assert_eq!(infer.flow(&capped, &plain), Err(Mismatch::CapLost));
        let list = |t: &Ty| Ty::list(t.clone());
        assert_eq!(
            infer.flow(&list(&capped), &list(&plain)),
            Err(Mismatch::CapLost)
        );
        // Parameters flip the direction.
        let takes = |t: &Ty| func(Cap::Plain, vec![t.clone()]);
        assert_eq!(infer.flow(&takes(&capped), &takes(&plain)), Ok(()));
        assert_eq!(
            infer.flow(&takes(&plain), &takes(&capped)),
            Err(Mismatch::CapLost)
        );
        assert_eq!(infer.flow(&plain, &Ty::Unit), Err(Mismatch::Types));
    }
}
