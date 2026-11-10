//! Purity that a body relies on: what its contracts reach (`E0280`), and
//! what it assumes of its type parameters (`instantiation.rs`, `E0274`).
//!
//! A contract may call only pure functions. It reaches code in two ways,
//! and each is judged the same way:
//!
//! - an explicit call, judged where it is checked, once its generics are
//!   instantiated ([`Body::contract_call`]);
//! - an operator or interpolation hole that dispatches to a trait impl:
//!   `==` and `!=` to `Eq`, a hole to `Display`, and a `?` inside a
//!   closure to `From` ([`Body::contract_dispatch`]).
//!
//! Every one is judged again once the body's types are solved, so a value
//! whose type turns out capability-bearing later in the body (on a loop's
//! next pass, say) is still caught.
//!
//! A type parameter is never capability-bearing inside the body, so a
//! contract that uses a value of one, or a plain `fn` closure that captures
//! one, is pure only if the parameter is not instantiated with a
//! capability-bearing type; an unscoped closure that captures one is safe
//! only if it is not instantiated with a type that holds a `Scope`. Once
//! the body's types are solved, [`Body::settle_uses`] records these
//! assumptions, and every instantiation the body makes, for
//! `instantiation.rs` to check across the module.

use std::collections::BTreeSet;

use oska_syntax::Span;

use super::{Body, Callable};
use crate::authority::capability_bearing;
use crate::codes;
use crate::decl_rules;
use crate::infer::Infer;
use crate::instantiation::{self, Assumed, Assumption, Instance};
use crate::items::{Bound, GenericKey};
use crate::ty::{Cap, Ty};

/// What a body's purity depends on, settled once its types are solved.
#[derive(Default)]
pub(super) struct Uses {
    /// What its contracts call or dispatch to.
    contract: Vec<ContractUse>,
    /// Every closure that captures something.
    closures: Vec<ClosureUse>,
    /// Each generic parameter of a callee, instantiated.
    instances: Vec<(GenericKey, Ty, Span)>,
    /// Each bound a type must satisfy, which a generic impl may satisfy.
    obligations: Vec<(Ty, Bound, Span)>,
}

/// Something a contract reaches: a call, or an operator's trait impl.
struct ContractUse {
    span: Span,
    reaches: Reaches,
    /// The types whose values the code reached receives.
    tys: Vec<Ty>,
    /// Already reported as impure.
    reported: bool,
}

/// What a [`ContractUse`] reaches.
pub(super) enum Reaches {
    /// A call, described as in its diagnostics, such as "`log`".
    Call(String),
    /// A trait method an operator dispatches to: `what` (such as "`==`")
    /// calls `method` (such as "`Eq.eq`").
    Dispatch {
        what: &'static str,
        method: &'static str,
    },
}

/// A closure, with what it captured.
pub(super) struct ClosureUse {
    pub(super) span: Span,
    pub(super) cap: Cap,
    pub(super) scoped: bool,
    pub(super) captures: Vec<(String, Ty)>,
    /// A capture's type was not solved yet when the closure was checked,
    /// so its `scoped` flag was decided early.
    pub(super) unsolved: bool,
}

impl Body<'_, '_, '_> {
    /// A call in a contract: it must be pure once instantiated. The callee
    /// is declared pure, and no argument, parameter, or receiver type is
    /// capability-bearing after its generics are solved. So
    /// `apply(log, out)` with `fn apply<T>(f: fn(T), x: T)` is impure when
    /// `out` is a `Stdout`.
    ///
    /// A pure call settles the open capability flags it read as plain
    /// `fn`s, so a `cap fn` cannot reach them later.
    pub(super) fn contract_call(&mut self, callable: &Callable, arg_tys: &[Ty], span: Span) {
        let tys: Vec<Ty> = callable
            .params
            .iter()
            .map(|(_, ty)| ty)
            .chain(callable.receiver.iter())
            .chain(arg_tys)
            .cloned()
            .collect();
        let pure = callable.pure && !tys.iter().any(|ty| self.bearing(ty));
        let reaches = Reaches::Call(callable.description.clone());
        if pure {
            self.settle_all(&tys);
        } else {
            self.impure(span, &reaches, None);
        }
        self.uses.contract.push(ContractUse {
            span,
            reaches,
            tys,
            reported: !pure,
        });
    }

    /// An operator in a contract that dispatches to a trait impl on values
    /// of the types `tys`, judged once the body's types are solved.
    pub(super) fn contract_dispatch(&mut self, span: Span, reaches: Reaches, tys: Vec<Ty>) {
        if self.in_contract {
            self.uses.contract.push(ContractUse {
                span,
                reaches,
                tys,
                reported: false,
            });
        }
    }

    /// A closure that captured something, to settle once the body's types
    /// are solved.
    pub(super) fn closure_use(&mut self, closure: ClosureUse) {
        self.uses.closures.push(closure);
    }

    /// The generic parameter `key` of a callee, instantiated with `ty` at
    /// `at`.
    pub(super) fn instance(&mut self, key: GenericKey, ty: &Ty, at: Span) {
        self.uses.instances.push((key, ty.clone(), at));
    }

    /// The bound `ty: bound` that the code at `at` needs.
    pub(super) fn obligation(&mut self, ty: &Ty, bound: Bound, at: Span) {
        self.uses.obligations.push((ty.clone(), bound, at));
    }

    /// Whether a capture of type `ty` keeps a closure's capability flag
    /// open: its type may still change, or holds a type parameter that
    /// may be instantiated with something capability-bearing.
    pub(super) fn capture_is_open(&self, ty: &Ty) -> bool {
        let (mut params, mut caps) = (Vec::new(), Vec::new());
        instantiation::positions(
            &self.infer.resolve(ty),
            Assumption::NotBearing,
            &mut params,
            &mut caps,
        );
        self.infer.is_open(ty) || !params.is_empty()
    }

    /// Settle everything the body's purity depends on, now that its types
    /// are solved: judge what the contracts reached again (`E0280`),
    /// reject a closure whose `scoped` flag was decided too early
    /// (`E0292`), and record what the body assumes of its type parameters
    /// and how it instantiates others.
    pub(super) fn settle_uses(&mut self) {
        let uses = std::mem::take(&mut self.uses);
        let carriers = self.carriers(&uses.closures);
        for used in &uses.contract {
            let resolved: Vec<Ty> = used.tys.iter().map(|t| self.infer.resolve(t)).collect();
            if !used.reported {
                if let Some(ty) = resolved.iter().find(|ty| self.bearing(ty)) {
                    self.impure(used.span, &used.reaches, Some(ty));
                }
            }
            for ty in &resolved {
                let params = carried(&self.infer, ty, Assumption::NotBearing, &carriers);
                self.assume(params, Assumption::NotBearing, |p, function| {
                    format!("a contract in `{function}` uses a value of type `{p}`")
                });
            }
        }
        for (closure, (_, carried_params)) in uses.closures.iter().zip(&carriers) {
            if self.infer.used_as_plain(closure.cap) {
                self.assume(
                    carried_params.clone(),
                    Assumption::NotBearing,
                    |p, function| {
                        format!(
                            "a closure in `{function}` that is used as a plain `fn` captures a \
                             value of type `{p}`"
                        )
                    },
                );
            }
            if closure.scoped {
                continue;
            }
            let captures: Vec<Ty> = closure
                .captures
                .iter()
                .map(|(_, t)| self.infer.resolve(t))
                .collect();
            let params: BTreeSet<String> = captures
                .iter()
                .flat_map(|t| carried(&self.infer, t, Assumption::NoScope, &[]))
                .collect();
            self.assume(params, Assumption::NoScope, |p, function| {
                format!(
                    "a closure in `{function}` that is not scoped captures a value of type `{p}`"
                )
            });
            let late = captures
                .iter()
                .zip(&closure.captures)
                .find(|(t, _)| t.holds_scope());
            if let (true, Some((_, (name, _)))) = (closure.unsolved, late) {
                self.out.diagnostics.push(decl_rules::scope_escapes(
                    closure.span,
                    &format!(
                        "be kept by this closure, through `{name}`, whose type was not known \
                         when the closure was checked; give `{name}` its type where it is \
                         declared, and the closure cannot escape the block"
                    ),
                ));
            }
        }
        for (key, ty, at) in &uses.instances {
            let instance = self.instance_in(&self.infer, *key, ty, *at, &carriers);
            self.out.instances.push(instance);
        }
        for (ty, bound, at) in &uses.obligations {
            let mut trial = self.infer.clone();
            self.obligation_instances(&mut trial, ty, bound, *at, &carriers, 0);
        }
    }

    /// For each closure, its capability variable and the type parameters
    /// it carries: those its captures carry, directly or through another
    /// closure they hold.
    fn carriers(&self, closures: &[ClosureUse]) -> Vec<(Option<u32>, BTreeSet<String>)> {
        let resolved: Vec<Vec<Ty>> = closures
            .iter()
            .map(|c| {
                c.captures
                    .iter()
                    .map(|(_, t)| self.infer.resolve(t))
                    .collect()
            })
            .collect();
        let mut carriers: Vec<(Option<u32>, BTreeSet<String>)> = closures
            .iter()
            .zip(&resolved)
            .map(|(c, tys)| {
                let var = match c.cap {
                    Cap::Var(v) => Some(v),
                    Cap::Plain | Cap::Bearing => None,
                };
                let params = tys
                    .iter()
                    .flat_map(|t| carried(&self.infer, t, Assumption::NotBearing, &[]))
                    .collect();
                (var, params)
            })
            .collect();
        // A closure may hold one checked after it, so repeat until no set
        // grows.
        loop {
            let mut grew = false;
            for (i, tys) in resolved.iter().enumerate() {
                let more: BTreeSet<String> = tys
                    .iter()
                    .flat_map(|t| carried(&self.infer, t, Assumption::NotBearing, &carriers))
                    .collect();
                let before = carriers[i].1.len();
                carriers[i].1.extend(more);
                grew |= carriers[i].1.len() > before;
            }
            if !grew {
                return carriers;
            }
        }
    }

    /// Record that the body assumes `assumption` of each type parameter in
    /// `params`, for the reason `why` gives for a parameter and the
    /// function.
    fn assume(
        &mut self,
        params: impl IntoIterator<Item = String>,
        assumption: Assumption,
        why: impl Fn(&str, &str) -> String,
    ) {
        for param in params {
            if let Some(key) = self.lower.key(&param) {
                let reason = why(&param, &self.out.function);
                self.out.assumed.push(Assumed {
                    key,
                    assumption,
                    reason,
                });
            }
        }
    }

    /// The instance of `key` by `ty`, as `infer` has solved it.
    fn instance_in(
        &self,
        infer: &Infer,
        key: GenericKey,
        ty: &Ty,
        at: Span,
        carriers: &[(Option<u32>, BTreeSet<String>)],
    ) -> Instance {
        let ty = infer.resolve(ty);
        let keys = |assumption| {
            carried(infer, &ty, assumption, carriers)
                .iter()
                .filter_map(|p| self.lower.key(p))
                .collect()
        };
        Instance {
            key,
            at,
            ty: ty.to_string(),
            bearing: capability_bearing(&ty, self.decls()),
            holds_scope: ty.holds_scope(),
            bearing_through: keys(Assumption::NotBearing),
            scope_through: keys(Assumption::NoScope),
        }
    }

    /// The instances of a generic impl's parameters that satisfying
    /// `ty: bound` makes, and those of the impls its own bounds are
    /// satisfied through in turn.
    fn obligation_instances(
        &mut self,
        infer: &mut Infer,
        ty: &Ty,
        bound: &Bound,
        at: Span,
        carriers: &[(Option<u32>, BTreeSet<String>)],
        depth: usize,
    ) {
        let ty = infer.resolve(ty);
        if depth > 16 || !matches!(ty, Ty::Named(..)) {
            return;
        }
        let Some((imp, args)) = self.impl_in(infer, &ty, &bound.trait_name, &bound.args, 0) else {
            return;
        };
        for (key, arg) in imp.generic_keys.iter().zip(&args) {
            if let Some(key) = key {
                let instance = self.instance_in(infer, *key, arg, at, carriers);
                self.out.instances.push(instance);
            }
        }
        let subst: Vec<(String, Ty)> = imp.generics.iter().cloned().zip(args).collect();
        for b in &imp.bounds {
            let param = Ty::Param(b.param.clone()).substitute(&subst);
            let inner = Bound {
                args: b.args.iter().map(|a| a.substitute(&subst)).collect(),
                ..b.clone()
            };
            self.obligation_instances(infer, &param, &inner, at, carriers, depth + 1);
        }
    }

    /// Report `E0280` at `span`, for a use that reaches capability-bearing
    /// `ty` when it is known.
    fn impure(&mut self, span: Span, reaches: &Reaches, ty: Option<&Ty>) {
        let message = match (reaches, ty) {
            (Reaches::Call(description), _) => format!(
                "a contract may call only pure functions, but {description} takes a capability"
            ),
            (Reaches::Dispatch { what, method }, Some(ty)) => format!(
                "a contract may call only pure functions, but {what} calls {method} on \
                 `{ty}`, which is capability-bearing"
            ),
            (Reaches::Dispatch { what, method }, None) => format!(
                "a contract may call only pure functions, but {what} calls {method} on a \
                 capability-bearing value"
            ),
        };
        self.error(codes::IMPURE_IN_CONTRACT, message, span);
    }
}

/// The type parameters that `ty`, resolved, would break `assumption`
/// through once instantiated: those in its positions (see
/// [`instantiation::positions`]), and those carried by a closure whose
/// capability variable flows into a function type in them, by `carriers`.
fn carried(
    infer: &Infer,
    ty: &Ty,
    assumption: Assumption,
    carriers: &[(Option<u32>, BTreeSet<String>)],
) -> BTreeSet<String> {
    let (mut params, mut caps) = (Vec::new(), Vec::new());
    instantiation::positions(ty, assumption, &mut params, &mut caps);
    let mut carried: BTreeSet<String> = params.into_iter().collect();
    for cap in caps {
        for (var, params) in carriers {
            if var.is_some_and(|v| infer.cap_reaches(v, cap)) {
                carried.extend(params.iter().cloned());
            }
        }
    }
    carried
}
