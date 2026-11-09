//! Unification: the inference context for one function body.
//!
//! Inference variables are created with [`Infer::fresh`] and solved by
//! [`Infer::unify`]. Nothing is shared between bodies, so inference never
//! crosses a function boundary.
//!
//! Unification is directional: a value of the `actual` type flows where
//! the `expected` type is required. The types must be equal except for
//! two one-way **flags**, which unification enforces itself so that no
//! route a value can take loses them:
//!
//! - the **`cap` rule**: a `cap fn` never becomes a plain `fn`;
//! - the **trust rule** (ADR 0006): untrusted text never becomes trusted.
//!
//! Trust is solved like a one-bit subtyping problem. Besides `Trusted`
//! and `Untrusted`, a text type's flag may be a **trust variable**
//! ([`Trust::Var`]). Each flow between flags is an edge `a ≤ b`, where
//! trusted ≤ untrusted. A variable is untrusted once untrusted text
//! reaches it through the edges, and pinned trusted once it reaches a
//! trusted slot; one that would be both is a [`Mismatch::TrustLost`].
//! Edges are kept, so text that turns out untrusted later in the body
//! still reaches every slot it flowed into.
//!
//! Trust variables give two things:
//!
//! - **Joins.** When an inference variable is solved, every text flag in
//!   its solution becomes a fresh trust variable that the solving type
//!   flows into (or out of). So a variable first met with trusted text
//!   still accepts untrusted text later, and becomes untrusted: the join
//!   of trusted and untrusted is untrusted. A variable that has already
//!   flowed into a trusted slot rejects it.
//! - **Trust polymorphism.** A `#[follows_trust]` prelude operation gets
//!   one fresh trust variable for each call, in place of every trusted
//!   text flag in its signature (see `body/calls.rs`). The receiver and
//!   the text arguments flow into it, and every text type in the result
//!   is it.

use crate::ty::{Trust, Ty};

/// What is known about one trust variable.
#[derive(Debug, Default, Clone)]
struct TrustVar {
    /// Untrusted text flows into it.
    untrusted: bool,
    /// It flows into a trusted slot.
    pinned: bool,
    /// The variables it flows into.
    into: Vec<u32>,
    /// The variables that flow into it.
    from: Vec<u32>,
}

/// Why a value cannot flow where a type is expected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mismatch {
    /// The types differ.
    Types,
    /// The types match, except that a `cap fn` would become a plain `fn`.
    CapLost,
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

    /// Whether a one-way flag survives: `a` and `b` say whether each side
    /// has the flag (is a `cap fn`, is untrusted). A flagged value never
    /// flows into an unflagged slot.
    fn keeps(self, a: bool, b: bool) -> bool {
        match self {
            Polarity::Positive => !a || b,
            Polarity::Negative => !b || a,
        }
    }
}

/// Which one-way flags a unification enforces. [`Infer::flow`] retries
/// with one turned off to find out which flag was the only obstacle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rules {
    cap: bool,
    trust: bool,
}

#[derive(Debug, Default, Clone)]
pub struct Infer {
    /// The solution of each variable, by index.
    solutions: Vec<Option<Ty>>,
    /// Each trust variable, by index.
    trust: Vec<TrustVar>,
}

impl Infer {
    /// A new, unsolved inference variable.
    pub fn fresh(&mut self) -> Ty {
        self.solutions.push(None);
        Ty::Var(self.solutions.len() as u32 - 1)
    }

    /// A new trust variable, with nothing flowing into or out of it.
    pub fn fresh_trust(&mut self) -> Trust {
        self.trust.push(TrustVar::default());
        Trust::Var(self.trust.len() as u32 - 1)
    }

    /// `ty` with every text flag replaced by a fresh trust variable, so
    /// that it accepts text of either trust. Unsolved variables inside
    /// are kept.
    pub fn loosen(&mut self, ty: &Ty) -> Ty {
        self.refresh(ty, true)
    }

    /// `ty` with every fixed text flag replaced by a fresh trust variable,
    /// and with its trust variables kept. An inference variable is solved
    /// to this: it can join with text of other trust, while text it shares
    /// a trust variable with (such as the elements of a list it was taken
    /// from) still sees everything that flows into it later.
    fn generalize(&mut self, ty: &Ty) -> Ty {
        self.refresh(ty, false)
    }

    fn refresh(&mut self, ty: &Ty, variables_too: bool) -> Ty {
        match ty {
            Ty::Named(name, args, trust) => {
                let args = args
                    .iter()
                    .map(|a| self.refresh(a, variables_too))
                    .collect();
                let trust = match trust {
                    Trust::Var(_) if !variables_too => *trust,
                    _ if ty.is_text() => self.fresh_trust(),
                    _ => *trust,
                };
                Ty::Named(name.clone(), args, trust)
            }
            Ty::Fn { cap, params, ret } => Ty::Fn {
                cap: *cap,
                params: params
                    .iter()
                    .map(|p| self.refresh(p, variables_too))
                    .collect(),
                ret: Box::new(self.refresh(ret, variables_too)),
            },
            other => other.clone(),
        }
    }

    /// Let the trust of every text type in a value of type `ty` flow into
    /// `into`, as the holes of an interpolated string do. Function types
    /// are skipped: they are not text.
    pub fn join_into(&mut self, ty: &Ty, into: Trust) {
        if let Ty::Named(_, args, trust) = self.shallow(ty) {
            // Only fails when `into` is pinned, which the caller rules out.
            self.trust_flow(trust, into);
            for arg in &args {
                self.join_into(arg, into);
            }
        }
    }

    /// A trust flag as solved so far: a variable is untrusted once
    /// untrusted text reaches it, which is final. Any other variable is
    /// kept, even a pinned one: text that shares it must keep rejecting
    /// untrusted text that arrives later.
    fn trust_of(&self, trust: Trust) -> Trust {
        match trust {
            Trust::Var(v) if self.trust[v as usize].untrusted => Trust::Untrusted,
            other => other,
        }
    }

    /// Add the edge `a ≤ b`: text of trust `a` flows where `b` is
    /// required. Fails when untrusted text would reach a trusted slot.
    fn trust_flow(&mut self, a: Trust, b: Trust) -> bool {
        match (a, b) {
            (Trust::Trusted, _) | (_, Trust::Untrusted) => true,
            (Trust::Untrusted, Trust::Trusted) => false,
            (Trust::Untrusted, Trust::Var(v)) => self.taint(v),
            (Trust::Var(v), Trust::Trusted) => self.pin(v),
            (Trust::Var(x), Trust::Var(y)) => {
                if x == y {
                    return true;
                }
                self.trust[x as usize].into.push(y);
                self.trust[y as usize].from.push(x);
                (!self.trust[x as usize].untrusted || self.taint(y))
                    && (!self.trust[y as usize].pinned || self.pin(x))
            }
        }
    }

    /// Untrusted text reaches `v`, and everything `v` flows into.
    fn taint(&mut self, v: u32) -> bool {
        let var = &mut self.trust[v as usize];
        if var.untrusted {
            return true;
        }
        if var.pinned {
            return false;
        }
        var.untrusted = true;
        let into = var.into.clone();
        into.into_iter().all(|w| self.taint(w))
    }

    /// `v`, and everything that flows into it, flows into a trusted slot.
    fn pin(&mut self, v: u32) -> bool {
        let var = &mut self.trust[v as usize];
        if var.pinned {
            return true;
        }
        if var.untrusted {
            return false;
        }
        var.pinned = true;
        let from = var.from.clone();
        from.into_iter().all(|w| self.pin(w))
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
            Ty::Fn { cap, params, ret } => Ty::Fn {
                cap,
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
    /// The types must be equal, except for two one-way flags:
    ///
    /// - a `cap fn` can carry authority, so it never flows into a plain
    ///   `fn`, while a plain `fn` may flow into a `cap fn`;
    /// - untrusted text never flows into trusted text, while trusted text
    ///   may flow into untrusted.
    ///
    /// Both rules apply at every depth: covariantly through type arguments
    /// and results, contravariantly through a function type's parameters.
    /// An inference variable is solved to the type it meets, so a variable
    /// solved to a plain `fn` rejects a `cap fn` later. Its text flags are
    /// trust variables, though: one first met with trusted text accepts
    /// untrusted text later, unless it has flowed into a trusted slot.
    pub fn unify(&mut self, actual: &Ty, expected: &Ty) -> bool {
        self.flow(actual, expected).is_ok()
    }

    /// [`Infer::unify`], saying why it failed.
    pub fn flow(&mut self, actual: &Ty, expected: &Ty) -> Result<(), Mismatch> {
        let attempts = [
            (
                Rules {
                    cap: true,
                    trust: true,
                },
                None,
            ),
            (
                Rules {
                    cap: false,
                    trust: true,
                },
                Some(Mismatch::CapLost),
            ),
            (
                Rules {
                    cap: true,
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
                let trust_kept = !rules.trust
                    || match polarity {
                        Polarity::Positive => self.trust_flow(*t1, *t2),
                        Polarity::Negative => self.trust_flow(*t2, *t1),
                    };
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
                    params: p1,
                    ret: r1,
                },
                Ty::Fn {
                    cap: c2,
                    params: p2,
                    ret: r2,
                },
            ) => {
                let cap_kept = !rules.cap || polarity.keeps(*c1, *c2);
                let flipped = polarity.flip();
                cap_kept
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

    fn func(cap: bool, params: Vec<Ty>) -> Ty {
        Ty::Fn {
            cap,
            params,
            ret: Box::new(Ty::Unit),
        }
    }

    #[test]
    fn the_cap_rule_holds_at_every_depth() {
        let plain = func(false, vec![]);
        let capped = func(true, vec![]);
        let mut infer = Infer::default();
        assert_eq!(infer.flow(&plain, &capped), Ok(()));
        assert_eq!(infer.flow(&capped, &plain), Err(Mismatch::CapLost));
        let list = |t: &Ty| Ty::list(t.clone());
        assert_eq!(
            infer.flow(&list(&capped), &list(&plain)),
            Err(Mismatch::CapLost)
        );
        // Parameters flip the direction.
        let takes = |t: &Ty| func(false, vec![t.clone()]);
        assert_eq!(infer.flow(&takes(&capped), &takes(&plain)), Ok(()));
        assert_eq!(
            infer.flow(&takes(&plain), &takes(&capped)),
            Err(Mismatch::CapLost)
        );
        assert_eq!(infer.flow(&plain, &Ty::Unit), Err(Mismatch::Types));
    }
}
