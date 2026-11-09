//! Unification: the inference context for one function body.
//!
//! Inference variables are created with [`Infer::fresh`] and solved by
//! [`Infer::unify`]. Nothing is shared between bodies, so inference never
//! crosses a function boundary.

use crate::ty::Ty;

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

    /// Make `a` and `b` the same type, solving variables as needed. On
    /// failure nothing is solved, so the caller can report one mismatch
    /// and carry on. [`Ty::Error`] and [`Ty::Never`] unify with anything.
    ///
    /// The `cap` flag of function types is not compared here: passing a
    /// `cap fn` where a plain `fn` is expected is a capability rule, not
    /// a type mismatch.
    pub fn unify(&mut self, a: &Ty, b: &Ty) -> bool {
        let saved = self.solutions.clone();
        let ok = self.unify_inner(a, b);
        if !ok {
            self.solutions = saved;
        }
        ok
    }

    fn unify_inner(&mut self, a: &Ty, b: &Ty) -> bool {
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
                    && a1.iter().zip(a2).all(|(x, y)| self.unify_inner(x, y))
            }
            (Ty::Unit, Ty::Unit) => true,
            (Ty::Param(p), Ty::Param(q)) => p == q,
            (
                Ty::Fn {
                    params: p1,
                    ret: r1,
                    ..
                },
                Ty::Fn {
                    params: p2,
                    ret: r2,
                    ..
                },
            ) => {
                p1.len() == p2.len()
                    && p1.iter().zip(p2).all(|(x, y)| self.unify_inner(x, y))
                    && self.unify_inner(r1, r2)
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
