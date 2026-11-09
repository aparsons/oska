//! Unification: the inference context for one function body.
//!
//! Inference variables are created with [`Infer::fresh`] and solved by
//! [`Infer::unify`]. Nothing is shared between bodies, so inference never
//! crosses a function boundary.
//!
//! Unification is directional: a value of the `actual` type flows where
//! the `expected` type is required. The types must be equal except for
//! the **`cap` rule**, which unification enforces itself, so that no route
//! a value can take turns a `cap fn` into a plain `fn`.

use crate::ty::Ty;

/// Why a value cannot flow where a type is expected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mismatch {
    /// The types differ.
    Types,
    /// The types match, except that a `cap fn` would become a plain `fn`.
    CapLost,
}

/// Which way values flow at a position inside a type: the same way as the
/// whole (`Positive`), against it (`Negative`, inside a function type's
/// parameters), or `Ignore` to skip the `cap` rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Polarity {
    Positive,
    Negative,
    Ignore,
}

impl Polarity {
    fn flip(self) -> Polarity {
        match self {
            Polarity::Positive => Polarity::Negative,
            Polarity::Negative => Polarity::Positive,
            Polarity::Ignore => Polarity::Ignore,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct Infer {
    /// The solution of each variable, by index.
    solutions: Vec<Option<Ty>>,
}

impl Infer {
    /// A new, unsolved inference variable.
    pub fn fresh(&mut self) -> Ty {
        self.solutions.push(None);
        Ty::Var(self.solutions.len() as u32 - 1)
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
            Ty::Named(name, args) => {
                Ty::Named(name, args.iter().map(|a| self.resolve(a)).collect())
            }
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
    /// The types must be equal, except for the `cap` rule: a `cap fn` can
    /// carry authority, so it never flows into a plain `fn`, while a plain
    /// `fn` may flow into a `cap fn`. The rule applies at every depth:
    /// covariantly through type arguments and results, contravariantly
    /// through a function type's parameters. An inference variable is
    /// solved to exactly the type it meets, so a variable solved to a
    /// plain `fn` rejects a `cap fn` later.
    pub fn unify(&mut self, actual: &Ty, expected: &Ty) -> bool {
        self.flow(actual, expected).is_ok()
    }

    /// [`Infer::unify`], saying why it failed.
    pub fn flow(&mut self, actual: &Ty, expected: &Ty) -> Result<(), Mismatch> {
        let saved = self.solutions.clone();
        if self.unify_inner(actual, expected, Polarity::Positive) {
            return Ok(());
        }
        self.solutions = saved.clone();
        let only_cap = self.unify_inner(actual, expected, Polarity::Ignore);
        self.solutions = saved;
        Err(if only_cap {
            Mismatch::CapLost
        } else {
            Mismatch::Types
        })
    }

    fn unify_inner(&mut self, a: &Ty, b: &Ty, polarity: Polarity) -> bool {
        let a = self.shallow(a);
        let b = self.shallow(b);
        match (&a, &b) {
            (Ty::Error, _) | (_, Ty::Error) | (Ty::Never, _) | (_, Ty::Never) => true,
            (Ty::Var(x), Ty::Var(y)) if x == y => true,
            (Ty::Var(v), other) | (other, Ty::Var(v)) => {
                if self.occurs(*v, other) {
                    return false;
                }
                self.solutions[*v as usize] = Some(other.clone());
                true
            }
            (Ty::Named(n1, a1), Ty::Named(n2, a2)) => {
                n1 == n2
                    && a1.len() == a2.len()
                    && a1
                        .iter()
                        .zip(a2)
                        .all(|(x, y)| self.unify_inner(x, y, polarity))
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
                // `a` flows into `b` when positive, `b` into `a` when
                // negative.
                let cap_kept = match polarity {
                    Polarity::Positive => !*c1 || *c2,
                    Polarity::Negative => !*c2 || *c1,
                    Polarity::Ignore => true,
                };
                let flipped = polarity.flip();
                cap_kept
                    && p1.len() == p2.len()
                    && p1
                        .iter()
                        .zip(p2)
                        .all(|(x, y)| self.unify_inner(x, y, flipped))
                    && self.unify_inner(r1, r2, polarity)
            }
            _ => false,
        }
    }

    fn occurs(&self, v: u32, ty: &Ty) -> bool {
        match self.shallow(ty) {
            Ty::Var(w) => v == w,
            Ty::Named(_, args) => args.iter().any(|a| self.occurs(v, a)),
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
