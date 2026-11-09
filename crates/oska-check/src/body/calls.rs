//! Calls and construction: functions, methods, associated functions,
//! constructors, and variants, each instantiated as a [`Callable`] and
//! applied to its arguments, with labels, bounds, and contract purity
//! checked.

use oska_syntax::ast::{Arg, Expr, ExprKind, Ident};
use oska_syntax::{Diagnostic, Edit, Span};

use super::{Body, Callable, Labels, QualifiedPath};
use crate::codes;
use crate::collect::DeclKind;
use crate::items::{Bound, FieldsInfo, FnSig, Receiver, Shape};
use crate::program::lower::Resolution;
use crate::program::Callee;

use crate::ty::{self, Ty};

impl Body<'_, '_, '_> {
    pub(super) fn call(
        &mut self,
        expr: &Expr,
        callee: &Expr,
        args: &[Arg],
        expected: Option<&Ty>,
    ) -> Ty {
        let callable = match &callee.kind {
            ExprKind::Field { base, name } => match self.qualified_path(base, name) {
                Some(QualifiedPath::Export(key)) => self.item_callee(&key, name),
                Some(QualifiedPath::Member(type_name)) => self.type_member(&type_name, name),
                Some(QualifiedPath::Reported) => None,
                None => return self.method_call(base, name, args, expected, expr.span),
            },
            ExprKind::Name(ident) if self.local(&ident.name).is_none() => self.named_callee(ident),
            _ => {
                let ty = self.infer(callee);
                match self.infer.resolve(&ty) {
                    Ty::Fn { cap, params, ret } => Some(Callable {
                        description: "this function".to_string(),
                        params: params.into_iter().map(|p| (None, p)).collect(),
                        ret: *ret,
                        labels: Labels::Optional,
                        obligations: Vec::new(),
                        pure: !cap,
                        receiver: None,
                        joined: None,
                        type_args: Vec::new(),
                    }),
                    Ty::Error => None,
                    other => {
                        self.error(
                            codes::NOT_CALLABLE,
                            format!("`{other}` is not a function"),
                            callee.span,
                        );
                        None
                    }
                }
            }
        };
        match callable {
            Some(callable) => self.apply(callable, args, expected, expr.span),
            None => {
                self.infer_args(args);
                Ty::Error
            }
        }
    }

    /// A bare name called: a function, a struct, or a prelude variant.
    pub(super) fn named_callee(&mut self, ident: &Ident) -> Option<Callable> {
        if !self.require_unqualified(ident) {
            return None;
        }
        self.item_callee(&ident.name, ident)
    }

    /// A function, struct, or prelude variant called, named either
    /// unqualified or as a module's export: `ident` as written, declared
    /// under `key` (see [`crate::collect::Decls::prelude_key`]).
    fn item_callee(&mut self, key: &str, ident: &Ident) -> Option<Callable> {
        let name = key;
        if let Some(sig) = self.items.fns.get(name).cloned() {
            return Some(self.instantiate(&sig, None));
        }
        if let Some(info) = self.items.structs.get(name).cloned() {
            if info.sealed {
                let capability =
                    matches!(self.decls().get(name), Some(d) if d.kind == DeclKind::Capability);
                let message = if capability {
                    format!(
                        "`{name}` is a built-in capability and cannot be constructed; \
                         take it as a parameter, ultimately from `main`"
                    )
                } else {
                    format!(
                        "`{name}` has private fields and cannot be constructed outside the prelude"
                    )
                };
                self.error(codes::FORGED, message, ident.span);
                return None;
            }
            return Some(self.constructor(name, &info.generics, &info.fields, name.to_string()));
        }
        if let Some(enum_name) = self.items.bare_variants.get(name).cloned() {
            return self.variant(&enum_name, name);
        }
        if self.decls().get(name).is_some() {
            self.error(
                codes::NOT_A_VALUE,
                format!("`{name}` cannot be constructed"),
                ident.span,
            );
            return None;
        }
        self.unknown_value(ident);
        None
    }

    /// `Type.name`: a variant of an enum, or an associated function or
    /// method of the type. A method called this way takes its receiver as
    /// the first argument.
    pub(super) fn type_member(&mut self, type_name: &str, name: &Ident) -> Option<Callable> {
        if self.items.enums.contains_key(type_name) {
            if let Some(callable) = self.variant(type_name, &name.name) {
                return Some(callable);
            }
        }
        if matches!(self.decls().get(type_name), Some(d) if d.kind == DeclKind::Trait) {
            return self.qualified_call(type_name, name);
        }
        if let Some(sig) = self.items.method(type_name, &name.name).cloned() {
            let mut callable = self.instantiate(&sig, None);
            if sig.receiver.is_method() {
                let self_ty = sig.self_ty.clone().unwrap_or(Ty::Error);
                callable.params.insert(0, (None, self_ty));
            }
            return Some(callable);
        }
        let mut candidates = self.items.method_names(type_name);
        if let Some(info) = self.items.enums.get(type_name) {
            candidates.extend(info.variants.iter().map(|(v, _)| v.as_str()));
        }
        let message = format!("`{type_name}` has no variant or function `{}`", name.name);
        self.report_with_suggestion(codes::NO_MEMBER, message, name, &candidates);
        None
    }

    /// `Trait.method`: the trait's method called qualified, as in
    /// `Display.to_string(x)`. The receiver is the first argument, and
    /// its type must implement the trait.
    pub(super) fn qualified_call(&mut self, trait_name: &str, name: &Ident) -> Option<Callable> {
        let info = self.items.traits.get(trait_name)?.clone();
        let Some((sig, _)) = info.methods.iter().find(|(s, _)| s.name == name.name) else {
            let candidates: Vec<&str> = info.methods.iter().map(|(s, _)| s.name.as_str()).collect();
            let message = format!("the trait `{trait_name}` has no method `{}`", name.name);
            self.report_with_suggestion(codes::NO_MEMBER, message, name, &candidates);
            return None;
        };
        let self_ty = self.infer.fresh();
        let args: Vec<Ty> = info.generics.iter().map(|_| self.infer.fresh()).collect();
        let sig = info.method_for(sig, &self_ty, &args, &[], &[]);
        let mut callable = self.instantiate(&sig, None);
        callable.description = format!("`{trait_name}.{}`", name.name);
        // `Self` is a type argument here, so the result follows its trust.
        self.follow_type_arg(&mut callable, self_ty.clone());
        if sig.receiver.is_method() {
            callable.params.insert(0, (None, self_ty.clone()));
        }
        callable.obligations.push((
            self_ty,
            Bound {
                param: ty::SELF.to_string(),
                trait_name: trait_name.to_string(),
                args,
            },
        ));
        Some(callable)
    }

    /// Report `name` as provided by more than one of `traits` (`E0264`).
    pub(super) fn ambiguous_method(&mut self, name: &Ident, traits: &[&FnSig]) {
        let mut names: Vec<&str> = traits
            .iter()
            .filter_map(|s| s.trait_name.as_deref())
            .collect();
        names.sort();
        let listed = names
            .iter()
            .map(|t| format!("`{t}`"))
            .collect::<Vec<_>>()
            .join(" and ");
        let example = names.first().copied().unwrap_or("Trait");
        self.error(
            codes::AMBIGUOUS_METHOD,
            format!(
                "`{}` is provided by {listed}; call it qualified, as in `{example}.{}(x)`",
                name.name, name.name
            ),
            name.span,
        );
    }

    /// The constructor of `variant` of `enum_name`, instantiated.
    pub(super) fn variant(&mut self, enum_name: &str, variant: &str) -> Option<Callable> {
        let info = self.items.enums.get(enum_name)?.clone();
        let (_, fields) = info.variants.iter().find(|(v, _)| v == variant)?;
        Some(self.constructor(
            enum_name,
            &info.generics,
            fields,
            format!("{enum_name}.{variant}"),
        ))
    }

    /// A variant named without a call: a value when it is fieldless, and
    /// its constructor function otherwise.
    pub(super) fn variant_value(&mut self, enum_name: &str, variant: &str) -> Ty {
        match self.variant(enum_name, variant) {
            Some(callable) if callable.params.is_empty() => callable.ret,
            Some(callable) => callable.into_fn_type(),
            None => Ty::Error,
        }
    }

    pub(super) fn constructor(
        &mut self,
        type_name: &str,
        generics: &[String],
        fields: &FieldsInfo,
        description: String,
    ) -> Callable {
        let subst: Vec<(String, Ty)> = generics
            .iter()
            .map(|g| (g.clone(), self.infer.fresh()))
            .collect();
        let ret = Ty::generic(type_name, subst.iter().map(|(_, t)| t.clone()).collect());
        Callable {
            description,
            params: fields.instantiate(&subst),
            ret,
            labels: match fields.shape {
                Shape::Named => Labels::Required,
                Shape::Positional | Shape::Unit => Labels::None,
            },
            obligations: Vec::new(),
            pure: true,
            receiver: None,
            joined: None,
            type_args: Vec::new(),
        }
    }

    /// Instantiate `sig`'s generics with fresh variables. `self_ty`, when
    /// given, is unified with the signature's `Self` type, which solves an
    /// impl's generics from the receiver.
    ///
    /// A `#[follows_trust]` operation also gets one fresh trust variable
    /// in place of every trusted text flag in its signature: the receiver
    /// and text arguments flow into it, so every text type in the result
    /// is untrusted when any of them is (ADR 0006).
    ///
    /// A generic callee **follows the trust of its type arguments**: when
    /// what any generic is instantiated with holds untrusted text, every
    /// text type in the result is untrusted (see [`Body::apply`]). So
    /// `fn launder<T: Display>(x: T) -> String` returns untrusted text for
    /// an untrusted `x`, however its body turned `x` into text.
    pub(super) fn instantiate(&mut self, sig: &FnSig, self_ty: Option<&Ty>) -> Callable {
        let subst: Vec<(String, Ty)> = sig
            .generics
            .iter()
            .map(|g| (g.clone(), self.infer.fresh()))
            .collect();
        let follows_type_args = !subst.is_empty() && sig.ret.has_trusted_text();
        let joined = (sig.follows_trust || follows_type_args).then(|| self.infer.fresh_trust());
        let lower_with = |t: &Ty, whole: bool| {
            let t = t.substitute(&subst);
            match joined {
                Some(trust) if whole || sig.follows_trust => t.trusted_as(trust),
                _ => t,
            }
        };
        let lower = |t: &Ty| lower_with(t, false);
        if let (Some(actual), Some(declared)) = (self_ty, &sig.self_ty) {
            self.infer.unify(actual, &lower(declared));
        }
        Callable {
            description: format!("`{}`", sig.name),
            params: sig
                .params
                .iter()
                .map(|(n, t)| (Some(n.clone()), lower(t)))
                .collect(),
            ret: lower_with(&sig.ret, true),
            labels: Labels::Optional,
            obligations: sig
                .bounds
                .iter()
                .map(|b| {
                    let ty = Ty::Param(b.param.clone()).substitute(&subst);
                    let bound = Bound {
                        args: b.args.iter().map(|a| a.substitute(&subst)).collect(),
                        ..b.clone()
                    };
                    (ty, bound)
                })
                .collect(),
            pure: sig.pure,
            receiver: sig
                .self_ty
                .as_ref()
                .filter(|_| sig.receiver.is_method())
                .map(lower),
            joined,
            type_args: subst.into_iter().map(|(_, t)| t).collect(),
        }
    }

    /// `base.name(args)`: a method call.
    pub(super) fn method_call(
        &mut self,
        base: &Expr,
        name: &Ident,
        args: &[Arg],
        expected: Option<&Ty>,
        span: Span,
    ) -> Ty {
        let receiver = self.infer(base);
        let receiver = self.infer.resolve(&receiver);
        let found = match &receiver {
            Ty::Named(type_name, _, _) => self.type_method(&receiver, type_name, name),
            Ty::Unit | Ty::Fn { .. } => self.type_method(&receiver, &receiver.to_string(), name),
            Ty::Param(p) => self.bound_method(p, name),
            Ty::Var(_) => {
                self.error(
                    codes::CANNOT_INFER,
                    format!("the type must be known before `.{}`", name.name),
                    name.span,
                );
                None
            }
            Ty::Error | Ty::Never => None,
        };
        let Some(sig) = found else {
            self.infer_args(args);
            return Ty::Error;
        };
        let type_name = receiver
            .head()
            .map_or_else(|| receiver.to_string(), str::to_string);
        match sig.receiver {
            Receiver::None => {
                self.error(
                    codes::NO_MEMBER,
                    format!(
                        "`{}` is an associated function; call it as `{type_name}.{}(...)`",
                        name.name, name.name
                    ),
                    name.span,
                );
                self.infer_args(args);
                return Ty::Error;
            }
            Receiver::VarSelf => self.require_mutable(
                base,
                codes::MUTATING_CALL_ON_LET,
                &format!("call the mutating method `{}` on", name.name),
            ),
            Receiver::Self_ => {}
        }
        self.audit_call(&type_name, &name.name, span);
        if sig.trait_name.is_none() {
            // Inherent methods are only declared on the module's own types
            // (`coherence.rs`), so one on a prelude type is an intrinsic.
            let qualified = format!("{type_name}.{}", name.name);
            let intrinsic = self.decls().get(&type_name).map_or(true, |d| d.in_prelude);
            let callee = if intrinsic {
                Callee::Intrinsic(qualified)
            } else {
                Callee::Function(qualified)
            };
            self.record(span, Resolution::Method(callee));
        }
        let mut callable = self.instantiate(&sig, None);
        if let Some(declared) = &callable.receiver {
            // The receiver flows into `self` like an argument, so untrusted
            // text is no receiver of a method that wants trusted text.
            self.expect(&receiver, declared, base.span);
        }
        if let Ty::Param(_) = receiver {
            // A bound's method on a generic parameter: the parameter may
            // stand for untrusted text, so text in the result is untrusted.
            self.follow_type_arg(&mut callable, receiver);
        }
        self.apply(callable, args, expected, span)
    }

    /// Let `callable`'s result follow the trust of `type_arg` as well, as
    /// a generic's instantiation does (see [`Body::instantiate`]).
    fn follow_type_arg(&mut self, callable: &mut Callable, type_arg: Ty) {
        if callable.joined.is_none() && callable.ret.has_trusted_text() {
            let joined = self.infer.fresh_trust();
            callable.ret = callable.ret.trusted_as(joined);
            callable.joined = Some(joined);
        }
        callable.type_args.push(type_arg);
    }

    /// The method `name` of a receiver of type `receiver`, named
    /// `type_name`: its own `impl` first, then its traits. Reports an
    /// unknown method (`E0228`).
    pub(super) fn type_method(
        &mut self,
        receiver: &Ty,
        type_name: &str,
        name: &Ident,
    ) -> Option<FnSig> {
        if let Some(sig) = self.items.inherent_method(type_name, &name.name) {
            return Some(sig.clone());
        }
        let found = self.items.trait_methods_named(type_name, &name.name);
        match found.as_slice() {
            [] => {}
            [sig] => return Some((*sig).clone()),
            _ => {
                self.ambiguous_method(name, &found);
                return None;
            }
        }
        if self.secret_method(receiver, name) {
            return None;
        }
        let candidates = self.items.method_names(type_name);
        let message = format!("`{receiver}` has no method `{}`", name.name);
        self.report_with_suggestion(codes::NO_MEMBER, message, name, &candidates);
        None
    }

    /// The method `name` of a value of the generic parameter `param`,
    /// which only its bounds provide. When they don't (`E0260`) and one
    /// trait has the method, the fix adds that trait as a bound.
    pub(super) fn bound_method(&mut self, param: &str, name: &Ident) -> Option<FnSig> {
        let self_ty = Ty::Param(param.to_string());
        let mut found: Vec<FnSig> = Vec::new();
        for bound in self.lower.bounds.iter().filter(|b| b.param == param) {
            let Some(info) = self.items.traits.get(&bound.trait_name) else {
                continue;
            };
            for (sig, _) in info.methods.iter().filter(|(s, _)| s.name == name.name) {
                if !found.iter().any(|f| f.trait_name == sig.trait_name) {
                    found.push(info.method_for(sig, &self_ty, &bound.args, &[], &[]));
                }
            }
        }
        match found.as_slice() {
            [] => {}
            [sig] => return Some(sig.clone()),
            _ => {
                let found: Vec<&FnSig> = found.iter().collect();
                self.ambiguous_method(name, &found);
                return None;
            }
        }
        let mut diagnostic = Diagnostic::new(
            codes::NOT_IN_BOUNDS,
            format!("no bound of `{param}` provides a method `{}`", name.name),
            name.span,
        );
        let mut providers: Vec<&String> = self
            .items
            .traits
            .iter()
            .filter(|(_, info)| {
                info.generics.is_empty() && info.methods.iter().any(|(s, _)| s.name == name.name)
            })
            .map(|(trait_name, _)| trait_name)
            .collect();
        providers.sort();
        let site = self.lower.sites.iter().rev().find(|s| s.name == param);
        if let ([trait_name], Some(site)) = (providers.as_slice(), site) {
            let text = if site.has_bounds {
                format!(" + {trait_name}")
            } else {
                format!(": {trait_name}")
            };
            diagnostic = diagnostic.with_fix(
                format!("add the bound `{param}: {trait_name}`"),
                vec![Edit::insert(site.end, text)],
            );
        }
        self.out.diagnostics.push(diagnostic);
        None
    }

    /// Check `args` against `callable`'s parameters and return its result.
    pub(super) fn apply(
        &mut self,
        callable: Callable,
        args: &[Arg],
        expected: Option<&Ty>,
        span: Span,
    ) -> Ty {
        // Let the expected result type guide the arguments first, as in
        // `let r: Result<U8, E> = Ok(200)`. A mismatch is reported later,
        // against the whole call.
        // For a `#[follows_trust]` operation the hint guides types only:
        // its result's trust comes from its arguments, so untrusted text
        // is reported where the result meets a trusted slot.
        if let Some(expected) = expected {
            let hint = match callable.joined {
                Some(_) => {
                    let hint = self.infer.resolve(expected);
                    self.infer.loosen(&hint)
                }
                None => expected.clone(),
            };
            self.infer.unify(&callable.ret, &hint);
        }
        if args.len() != callable.params.len() {
            let n = callable.params.len();
            self.error(
                codes::ARGUMENT_COUNT,
                format!(
                    "{} takes {n} argument{}, but {} {} given",
                    callable.description,
                    if n == 1 { "" } else { "s" },
                    args.len(),
                    if args.len() == 1 { "was" } else { "were" },
                ),
                span,
            );
        }
        let mut arg_tys = Vec::new();
        for (i, arg) in args.iter().enumerate() {
            let ty = match callable.params.get(i) {
                Some((param, ty)) => {
                    self.label(arg, param.as_deref(), callable.labels);
                    self.check(&arg.value, ty)
                }
                None => self.infer(&arg.value),
            };
            arg_tys.push(ty);
        }
        if self.in_contract && !self.pure_call(&callable, &arg_tys) {
            self.error(
                codes::IMPURE_IN_CONTRACT,
                format!(
                    "a contract may call only pure functions, but {} takes a capability",
                    callable.description
                ),
                span,
            );
        }
        if let Some(joined) = callable.joined {
            for ty in &callable.type_args {
                self.infer.join_into(ty, joined);
            }
        }
        let what = format!("calling {}", callable.description);
        for (ty, bound) in &callable.obligations {
            self.require_trait(ty, &bound.trait_name, &bound.args, span, &what);
        }
        callable.ret
    }

    /// Whether a call is pure once instantiated: the callee is declared
    /// pure, and no argument, parameter, or receiver type is
    /// capability-bearing after its generics are solved. So
    /// `apply(log, out)` with `fn apply<T>(f: fn(T), x: T)` is impure when
    /// `out` is a `Stdout`.
    pub(super) fn pure_call(&self, callable: &Callable, arg_tys: &[Ty]) -> bool {
        callable.pure
            && !callable
                .params
                .iter()
                .map(|(_, ty)| ty)
                .chain(callable.receiver.iter())
                .chain(arg_tys)
                .any(|ty| self.bearing(ty))
    }

    /// Check an argument's label against its parameter.
    pub(super) fn label(&mut self, arg: &Arg, param: Option<&str>, labels: Labels) {
        match (&arg.label, param, labels) {
            (Some(label), Some(param), _) if label.name != param => {
                self.out.diagnostics.push(
                    Diagnostic::new(
                        codes::BAD_LABEL,
                        format!("expected the label `{param}`, found `{}`", label.name),
                        label.span,
                    )
                    .with_fix(
                        format!("replace with `{param}`"),
                        vec![Edit {
                            span: label.span,
                            replacement: param.to_string(),
                        }],
                    ),
                );
            }
            (Some(label), None, _) => {
                let span = Span {
                    start: label.span.start,
                    end: arg.value.span.start,
                };
                self.out.diagnostics.push(
                    Diagnostic::new(
                        codes::BAD_LABEL,
                        format!("positional arguments take no label, found `{}`", label.name),
                        label.span,
                    )
                    .with_fix("remove the label", vec![Edit::delete(span)]),
                );
            }
            (None, Some(param), Labels::Required) => {
                self.out.diagnostics.push(
                    Diagnostic::new(
                        codes::MISSING_LABEL,
                        format!("named fields are constructed by name: write `{param}: ...`"),
                        arg.value.span,
                    )
                    .with_fix(
                        format!("label the argument `{param}`"),
                        vec![Edit::insert(arg.value.span.start, format!("{param}: "))],
                    ),
                );
            }
            _ => {}
        }
    }

    pub(super) fn infer_args(&mut self, args: &[Arg]) {
        for arg in args {
            self.infer(&arg.value);
        }
    }
}

impl Callable {
    /// The function type of a callable used as a value.
    pub(super) fn into_fn_type(self) -> Ty {
        Ty::Fn {
            cap: false,
            params: self.params.into_iter().map(|(_, t)| t).collect(),
            ret: Box::new(self.ret),
        }
    }
}
