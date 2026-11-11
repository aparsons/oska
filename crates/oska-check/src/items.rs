//! The value-level view of the prelude and the module: function
//! signatures, struct and enum shapes, traits, impls, and methods, with
//! every written type lowered to a [`Ty`].

use std::collections::{HashMap, HashSet};

use oska_syntax::ast::{
    Field, Fields, FnDecl, GenericParam, Item, ItemKind, Module, Type, TypeKind, Visibility,
};
use oska_syntax::Span;

use crate::authority::capability_bearing;
use crate::collect::{DeclKind, Decls};
use crate::ty::{self, Cap, Trust, Ty};

/// A function or method signature. Generic parameters appear in the
/// types as [`Ty::Param`]; instantiate them before use.
#[derive(Debug, Clone)]
pub struct FnSig {
    pub name: String,
    /// The impl's generic parameters followed by the function's own.
    pub generics: Vec<String>,
    /// The key of each of `generics`, for one the module declares.
    pub generic_keys: Vec<Option<GenericKey>>,
    /// The bounds on `generics`.
    pub bounds: Vec<Bound>,
    /// How the function takes `self`, if at all.
    pub receiver: Receiver,
    /// For a method or associated function: the type of `Self`.
    pub self_ty: Option<Ty>,
    pub params: Vec<(String, Ty)>,
    pub ret: Ty,
    /// For a method that comes from a trait: the trait's name.
    pub trait_name: Option<String>,
    /// A **pure function**: no parameter, the receiver included, is
    /// capability-bearing.
    pub pure: bool,
    /// A trust-polymorphic prelude text operation, marked
    /// `#[follows_trust]`: each call joins the trust of its receiver and
    /// text arguments, and every text type in its result has that trust
    /// (ADR 0006; see `infer.rs`).
    pub follows_trust: bool,
    /// A body-less prelude or `std` function, which each backend
    /// implements as an intrinsic.
    pub intrinsic: bool,
    /// For a method of an inherent `impl` the module declares: where it
    /// is and who may call it.
    pub declared: Option<Declared>,
}

/// Where a declaration of the module is, and who may use it: what
/// privacy across a package's modules needs (`body/privacy.rs`). The
/// prelude's declarations have none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Declared {
    pub visibility: Visibility,
    /// The start of the item, which says which module declares it.
    pub start: usize,
    /// The end of its last attribute, if it has any: a visibility marker
    /// goes after the attributes.
    pub attributes_end: Option<usize>,
}

impl Declared {
    pub fn of(item: &Item) -> Declared {
        Declared {
            visibility: item.visibility,
            start: item.span.start,
            attributes_end: item.attributes.last().map(|a| a.span.end),
        }
    }
}

/// How a function takes `self`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Receiver {
    /// A free function, or an associated function such as `List.new`.
    None,
    /// A method that takes `self`.
    Self_,
    /// A **mutating method**: it takes `var self`, so it can only be
    /// called on a `var` binding.
    VarSelf,
}

impl Receiver {
    /// A method, as opposed to a free or associated function.
    pub fn is_method(self) -> bool {
        self != Receiver::None
    }

    /// The receiver of a written function.
    pub fn of(f: &FnDecl) -> Receiver {
        match &f.receiver {
            None => Receiver::None,
            Some(r) if r.mutable => Receiver::VarSelf,
            Some(_) => Receiver::Self_,
        }
    }
}

/// A bound on a generic parameter, `T: Trait<Args>`, or the implicit
/// `Self: Trait` inside a trait's own methods.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bound {
    pub param: String,
    pub trait_name: String,
    pub args: Vec<Ty>,
}

/// One generic parameter the module declares, by the span of its name,
/// or a trait's `Self`, by the span of the trait's name. A body that
/// assumes something of a type parameter records it under this key, and
/// every instantiation of the parameter is checked against it
/// (`instantiation.rs`). The prelude's parameters have no key: no prelude
/// body is checked, so none assumes anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GenericKey(pub Span);

/// Where a written generic parameter's bounds end, so that a fix can
/// add one.
#[derive(Debug, Clone)]
pub struct GenericSite {
    pub name: String,
    /// The offset just after the name or the last bound.
    pub end: usize,
    pub has_bounds: bool,
}

/// How a struct's or variant's fields are declared, which decides how it
/// is constructed (ADR 0004).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// `{ a: T }`: every argument must be labelled, in order.
    Named,
    /// `(T)`: arguments are positional.
    Positional,
    /// A fieldless variant such as `None`.
    Unit,
}

#[derive(Debug, Clone)]
pub struct FieldsInfo {
    pub shape: Shape,
    /// Each field's name (absent when positional) and type.
    pub fields: Vec<(Option<String>, Ty)>,
}

impl FieldsInfo {
    /// The type of the named field `name`.
    pub fn field(&self, name: &str) -> Option<&Ty> {
        self.fields
            .iter()
            .find(|(n, _)| n.as_deref() == Some(name))
            .map(|(_, ty)| ty)
    }

    /// The position of the named field `name`, in declaration order.
    pub fn index(&self, name: &str) -> Option<usize> {
        self.fields
            .iter()
            .position(|(n, _)| n.as_deref() == Some(name))
    }

    /// The names of the named fields, in order.
    pub fn names(&self) -> Vec<&str> {
        self.fields
            .iter()
            .filter_map(|(n, _)| n.as_deref())
            .collect()
    }

    /// Each field's name and type, with generics replaced by `subst`.
    pub fn instantiate(&self, subst: &[(String, Ty)]) -> Vec<(Option<String>, Ty)> {
        self.fields
            .iter()
            .map(|(n, t)| (n.clone(), t.substitute(subst)))
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct StructInfo {
    pub generics: Vec<String>,
    pub fields: FieldsInfo,
    /// Declared in the prelude with at least one private field, so the
    /// module cannot construct it. Built-in capabilities are all sealed.
    pub sealed: bool,
    /// The names of the private fields of a prelude struct, which the
    /// module cannot read.
    pub hidden_fields: Vec<String>,
    /// For a struct the module declares: where it is, and each field
    /// without `pub`, by position, with the offset where `pub ` would go.
    pub declared: Option<(Declared, Vec<(usize, usize)>)>,
}

#[derive(Debug, Clone)]
pub struct EnumInfo {
    pub generics: Vec<String>,
    pub variants: Vec<(String, FieldsInfo)>,
}

/// A trait declaration.
#[derive(Debug, Clone)]
pub struct TraitInfo {
    pub generics: Vec<String>,
    /// The key of each of `generics`, for a trait the module declares.
    pub generic_keys: Vec<Option<GenericKey>>,
    /// The key of the trait's `Self`, for a trait the module declares.
    pub self_key: Option<GenericKey>,
    /// Each method, with `Self` as `Ty::Param("Self")` and the trait's
    /// generics as parameters, and whether it has a default body.
    pub methods: Vec<(FnSig, bool)>,
    /// For a trait the module declares: where it is and who may use it.
    pub declared: Option<Declared>,
}

/// An `impl Trait<Args> for Target`.
#[derive(Debug, Clone)]
pub struct ImplInfo {
    pub trait_name: String,
    pub trait_args: Vec<Ty>,
    pub target: Ty,
    pub generics: Vec<String>,
    /// The key of each of `generics`, for an impl the module declares.
    pub generic_keys: Vec<Option<GenericKey>>,
    pub bounds: Vec<Bound>,
    /// The target is capability-bearing, so its methods with a receiver
    /// are not pure.
    pub receiver_bearing: bool,
    /// The span of the `impl` item, for one the module declares; `None`
    /// for the prelude's.
    pub span: Option<Span>,
}

impl ImplInfo {
    /// The trait as the impl's methods are named in the checked program:
    /// `Display`, or `From<IoError>` with a generic trait's arguments
    /// (see [`crate::program::trait_method`]).
    pub fn trait_ref(&self) -> String {
        if self.trait_args.is_empty() {
            return self.trait_name.clone();
        }
        let args: Vec<String> = self.trait_args.iter().map(Ty::to_string).collect();
        format!("{}<{}>", self.trait_name, args.join(", "))
    }

    /// The checked program's name for the impl's `method`, such as
    /// `<AppError as From<IoError>>.from`.
    pub fn method_name(&self, method: &str) -> Option<String> {
        let type_name = self.target.head()?;
        Some(crate::program::trait_method(
            type_name,
            &self.trait_ref(),
            method,
        ))
    }
}

impl TraitInfo {
    /// The trait's method `sig` as seen through a type: `Self` is
    /// `self_ty` and the trait's generics are `args`. Seen through the
    /// impl `outer`, its generics and their bounds come before the
    /// method's own.
    pub fn method_for(
        &self,
        sig: &FnSig,
        self_ty: &Ty,
        args: &[Ty],
        outer: Option<&ImplInfo>,
    ) -> FnSig {
        let mut subst: Vec<(String, Ty)> = vec![(ty::SELF.to_string(), self_ty.clone())];
        subst.extend(self.generics.iter().cloned().zip(args.iter().cloned()));
        let bounds = outer
            .map(|imp| imp.bounds.as_slice())
            .unwrap_or_default()
            .iter()
            .cloned()
            .chain(sig.bounds.iter().map(|b| Bound {
                param: b.param.clone(),
                trait_name: b.trait_name.clone(),
                args: b.args.iter().map(|a| a.substitute(&subst)).collect(),
            }))
            .collect();
        let (outer_generics, outer_keys) = match outer {
            Some(imp) => (imp.generics.as_slice(), imp.generic_keys.as_slice()),
            None => (&[][..], &[][..]),
        };
        FnSig {
            name: sig.name.clone(),
            generics: outer_generics
                .iter()
                .chain(&sig.generics)
                .cloned()
                .collect(),
            generic_keys: outer_keys
                .iter()
                .chain(&sig.generic_keys)
                .copied()
                .collect(),
            bounds,
            receiver: sig.receiver,
            self_ty: Some(self_ty.clone()),
            params: sig
                .params
                .iter()
                .map(|(n, t)| (n.clone(), t.substitute(&subst)))
                .collect(),
            ret: sig.ret.substitute(&subst),
            trait_name: sig.trait_name.clone(),
            pure: sig.pure,
            follows_trust: sig.follows_trust,
            intrinsic: sig.intrinsic,
            declared: sig.declared,
        }
    }
}

/// Whether a value of type `ty` holds untrusted text, given the types
/// already known to hold it.
fn holds(ty: &Ty, holders: &HashSet<String>) -> bool {
    match ty {
        Ty::Named(name, args, trust) => {
            (ty.is_text() && *trust == Trust::Untrusted)
                || holders.contains(name)
                || args.iter().any(|a| holds(a, holders))
        }
        _ => false,
    }
}

/// The key `name` is declared under: renamed for a `std.*` module's
/// declaration that the user module shadows (see [`Decls::prelude_key`]).
fn key_of(decls: &Decls, name: &str, in_prelude: bool) -> String {
    match in_prelude {
        true => decls.prelude_key(name).to_string(),
        false => name.to_string(),
    }
}

/// Everything a body can refer to besides its own locals.
#[derive(Debug, Default)]
pub struct Items {
    pub fns: HashMap<String, FnSig>,
    pub structs: HashMap<String, StructInfo>,
    pub enums: HashMap<String, EnumInfo>,
    /// Methods and associated functions from inherent `impl Type` blocks,
    /// by type name.
    pub inherent: HashMap<String, Vec<FnSig>>,
    /// The methods that `impl Trait for Type` blocks give a type, by type
    /// name: every method of the trait, each tagged with the trait.
    pub trait_impls: HashMap<String, Vec<FnSig>>,
    pub traits: HashMap<String, TraitInfo>,
    /// Every `impl Trait for Type`, the prelude's first.
    pub impls: Vec<ImplInfo>,
    /// Variants usable without their enum's name (`Some`, `None`, `Ok`,
    /// `Err`): every variant of a prelude enum, mapped to its enum.
    pub bare_variants: HashMap<String, String>,
    /// The module's constants whose value is a plain string literal, by
    /// name, with that text. The authority report uses them to name an
    /// attenuation's root.
    pub consts: HashMap<String, String>,
    /// The structs and enums that **hold untrusted text**: one of their
    /// fields is untrusted text, or a type that holds it, at any depth.
    /// Their values turn into untrusted text (see
    /// [`crate::infer::Infer::join_into`]), so an `IoError` built from
    /// command-line text cannot become trusted text by `to_string` or
    /// interpolation (ADR 0006).
    pub untrusted_holders: HashSet<String>,
}

impl Items {
    /// Collect the prelude's items and then `module`'s. The first of two
    /// declarations with one name wins; `Decls` reports the duplicate.
    pub fn collect(module: &Module, decls: &Decls) -> Items {
        let mut items = Items::default();
        items.add(decls.prelude_items(), decls, true);
        items.add(&module.items, decls, false);
        items.trait_methods();
        items.untrusted_holders();
        items
    }

    fn add<'i>(
        &mut self,
        items: impl IntoIterator<Item = &'i Item>,
        decls: &Decls,
        in_prelude: bool,
    ) {
        for item in items {
            match &item.kind {
                ItemKind::Fn(f) => {
                    let mut sig = Lower::in_scope(decls, in_prelude).signature(f, &[], None);
                    sig.follows_trust = follows_trust(item);
                    let key = key_of(decls, &sig.name, in_prelude);
                    self.fns.entry(key).or_insert(sig);
                }
                ItemKind::Struct(s) => {
                    let lower = Lower::in_scope(decls, in_prelude).with_generics(&s.generics);
                    let private: Vec<&Field> = match &s.fields {
                        Fields::Named(list) | Fields::Positional(list) => {
                            list.iter().filter(|f| !f.public).collect()
                        }
                        Fields::Unit => Vec::new(),
                    };
                    let info = StructInfo {
                        generics: lower.generics.clone(),
                        fields: lower.fields(&s.fields),
                        sealed: in_prelude && !private.is_empty(),
                        hidden_fields: if in_prelude {
                            private
                                .iter()
                                .filter_map(|f| f.name.as_ref().map(|n| n.name.clone()))
                                .collect()
                        } else {
                            Vec::new()
                        },
                        declared: (!in_prelude).then(|| {
                            let all: &[Field] = match &s.fields {
                                Fields::Named(list) | Fields::Positional(list) => list,
                                Fields::Unit => &[],
                            };
                            let private = all
                                .iter()
                                .enumerate()
                                .filter(|(_, f)| !f.public)
                                .map(|(i, f)| (i, f.span.start))
                                .collect();
                            (Declared::of(item), private)
                        }),
                    };
                    let key = key_of(decls, &s.name.name, in_prelude);
                    self.structs.entry(key).or_insert(info);
                }
                ItemKind::Enum(e) => {
                    let lower = Lower::in_scope(decls, in_prelude).with_generics(&e.generics);
                    let variants = e
                        .variants
                        .iter()
                        .map(|v| (v.name.name.clone(), lower.fields(&v.fields)))
                        .collect::<Vec<_>>();
                    let key = key_of(decls, &e.name.name, in_prelude);
                    if in_prelude {
                        for (variant, _) in &variants {
                            self.bare_variants.insert(variant.clone(), key.clone());
                        }
                    }
                    let info = EnumInfo {
                        generics: lower.generics.clone(),
                        variants,
                    };
                    self.enums.entry(key).or_insert(info);
                }
                ItemKind::Impl(imp) => {
                    let lower = Lower::in_scope(decls, in_prelude).with_generics(&imp.generics);
                    let target = lower.ty(&imp.target);
                    let receiver_bearing = capability_bearing(&target, decls);
                    match &imp.trait_ref {
                        Some(trait_ref) => {
                            if let Some((trait_name, trait_args)) = lower.bound(trait_ref) {
                                self.impls.push(ImplInfo {
                                    trait_name,
                                    trait_args,
                                    target,
                                    generics: lower.generics.clone(),
                                    generic_keys: lower.generic_keys(),
                                    bounds: lower.bounds.clone(),
                                    receiver_bearing,
                                    span: (!in_prelude).then_some(item.span),
                                });
                            }
                        }
                        None => {
                            let Some(type_name) = target.head().map(str::to_string) else {
                                continue;
                            };
                            for method in &imp.methods {
                                if let ItemKind::Fn(f) = &method.kind {
                                    let mut sig =
                                        lower.signature(f, &imp.generics, Some(target.clone()));
                                    if f.receiver.is_some() && receiver_bearing {
                                        sig.pure = false;
                                    }
                                    sig.follows_trust = follows_trust(method);
                                    sig.declared = (!in_prelude).then(|| Declared::of(method));
                                    self.inherent
                                        .entry(type_name.clone())
                                        .or_default()
                                        .push(sig);
                                }
                            }
                        }
                    }
                }
                ItemKind::Trait(t) => {
                    let self_ty = Ty::Param(ty::SELF.to_string());
                    let lower = Lower::in_scope(decls, in_prelude)
                        .with_generics(&t.generics)
                        .with_self(self_ty.clone());
                    let methods = t
                        .methods
                        .iter()
                        .filter_map(|m| match &m.kind {
                            ItemKind::Fn(f) => {
                                // The trait's own generics are not in the
                                // signature's: they are replaced, not
                                // instantiated, when the method is used.
                                let mut sig = lower.signature(f, &[], Some(self_ty.clone()));
                                sig.trait_name = Some(key_of(decls, &t.name.name, in_prelude));
                                sig.follows_trust = follows_trust(m);
                                Some((sig, f.body.is_some()))
                            }
                            _ => None,
                        })
                        .collect();
                    let info = TraitInfo {
                        generics: lower.generics.clone(),
                        generic_keys: lower.generic_keys(),
                        self_key: (!in_prelude).then_some(GenericKey(t.name.span)),
                        methods,
                        declared: (!in_prelude).then(|| Declared::of(item)),
                    };
                    let key = key_of(decls, &t.name.name, in_prelude);
                    self.traits.entry(key).or_insert(info);
                }
                ItemKind::Const(c) => {
                    if let (false, Some(text)) = (in_prelude, literal_text(&c.value)) {
                        self.consts.entry(c.name.name.clone()).or_insert(text);
                    }
                }
                ItemKind::Use(_) | ItemKind::Test(_) => {}
            }
        }
    }

    /// Find [`Items::untrusted_holders`]: grow the set until no struct or
    /// enum outside it has a field that holds untrusted text.
    fn untrusted_holders(&mut self) {
        loop {
            let holders = &self.untrusted_holders;
            let fields = |f: &FieldsInfo| f.fields.iter().any(|(_, t)| holds(t, holders));
            let found: Vec<String> = self
                .structs
                .iter()
                .filter(|(_, info)| fields(&info.fields))
                .map(|(name, _)| name)
                .chain(
                    self.enums
                        .iter()
                        .filter(|(_, info)| info.variants.iter().any(|(_, f)| fields(f)))
                        .map(|(name, _)| name),
                )
                .filter(|name| !holders.contains(*name))
                .cloned()
                .collect();
            if found.is_empty() {
                return;
            }
            self.untrusted_holders.extend(found);
        }
    }

    /// Give each implementing type its traits' methods.
    fn trait_methods(&mut self) {
        for imp in &self.impls {
            let (Some(type_name), Some(info)) =
                (imp.target.head(), self.traits.get(&imp.trait_name))
            else {
                continue;
            };
            let table = self.trait_impls.entry(type_name.to_string()).or_default();
            for (sig, _) in &info.methods {
                let mut method = info.method_for(sig, &imp.target, &imp.trait_args, Some(imp));
                if method.receiver.is_method() && imp.receiver_bearing {
                    method.pure = false;
                }
                table.push(method);
            }
        }
    }

    /// The type's own method or associated function `name`.
    pub fn inherent_method(&self, type_name: &str, name: &str) -> Option<&FnSig> {
        self.inherent
            .get(type_name)?
            .iter()
            .find(|s| s.name == name)
    }

    /// The methods named `name` that the type's traits provide, one per
    /// trait.
    pub fn trait_methods_named(&self, type_name: &str, name: &str) -> Vec<&FnSig> {
        let mut found: Vec<&FnSig> = Vec::new();
        for sig in self.trait_impls.get(type_name).into_iter().flatten() {
            if sig.name == name && !found.iter().any(|f| f.trait_name == sig.trait_name) {
                found.push(sig);
            }
        }
        found
    }

    /// The method or associated function `name` of the type `type_name`:
    /// inherent methods first, then methods of implemented traits.
    pub fn method(&self, type_name: &str, name: &str) -> Option<&FnSig> {
        self.inherent_method(type_name, name)
            .or_else(|| self.trait_methods_named(type_name, name).into_iter().next())
    }

    /// The names of every method of `type_name`, for suggestions.
    pub fn method_names(&self, type_name: &str) -> Vec<&str> {
        [&self.inherent, &self.trait_impls]
            .iter()
            .filter_map(|table| table.get(type_name))
            .flatten()
            .map(|s| s.name.as_str())
            .collect()
    }
}

/// Lowers written types to [`Ty`], with generic parameters and `Self` in
/// scope. Names that do not resolve become [`Ty::Error`]: name
/// resolution has already reported them.
#[derive(Clone)]
pub struct Lower<'d, 'a> {
    decls: &'d Decls<'a>,
    pub generics: Vec<String>,
    /// The bounds on `generics`.
    pub bounds: Vec<Bound>,
    /// Where each written generic parameter is, for fixes that add a
    /// bound.
    pub sites: Vec<GenericSite>,
    /// The key of each generic parameter in scope that the module
    /// declares, `Self` inside a trait included, innermost last.
    pub keys: Vec<(String, GenericKey)>,
    pub self_ty: Option<Ty>,
    /// Lowering the prelude or a `std` module: every declared name is in
    /// scope unqualified.
    see_all: bool,
}

impl<'d, 'a> Lower<'d, 'a> {
    /// Lowers types written in the user module, which sees an imported
    /// module's names only as that module's exports (see
    /// [`Decls::path_name`]).
    pub fn new(decls: &'d Decls<'a>) -> Self {
        Self::in_scope(decls, false)
    }

    /// Lowers types written in the user module, or, when `in_prelude`, in
    /// the prelude or a `std` module, which see every declared name.
    pub fn in_scope(decls: &'d Decls<'a>, in_prelude: bool) -> Self {
        Lower {
            decls,
            generics: Vec::new(),
            bounds: Vec::new(),
            sites: Vec::new(),
            keys: Vec::new(),
            self_ty: None,
            see_all: in_prelude,
        }
    }

    /// Bring `generics` and their bounds into scope as well.
    pub fn with_generics(mut self, generics: &[GenericParam]) -> Self {
        self.generics
            .extend(generics.iter().map(|g| g.name.name.clone()));
        for g in generics {
            for bound in &g.bounds {
                if let Some((trait_name, args)) = self.bound(bound) {
                    self.bounds.push(Bound {
                        param: g.name.name.clone(),
                        trait_name,
                        args,
                    });
                }
            }
            self.sites.push(GenericSite {
                name: g.name.name.clone(),
                end: g.bounds.last().map_or(g.name.span.end, |b| b.span.end),
                has_bounds: !g.bounds.is_empty(),
            });
            if !self.see_all {
                self.keys
                    .push((g.name.name.clone(), GenericKey(g.name.span)));
            }
        }
        self
    }

    /// Give the generic parameter `name` the key `key`, as for a trait's
    /// `Self`.
    pub fn with_key(mut self, name: &str, key: GenericKey) -> Self {
        self.keys.push((name.to_string(), key));
        self
    }

    /// The key of the generic parameter `name` in scope, if the module
    /// declares it.
    pub fn key(&self, name: &str) -> Option<GenericKey> {
        self.keys
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, key)| *key)
    }

    /// The key of each of `generics`, in order.
    fn generic_keys(&self) -> Vec<Option<GenericKey>> {
        self.generics.iter().map(|g| self.key(g)).collect()
    }

    /// Bring generic parameters into scope by name only, without bounds,
    /// as for a declaration's field types.
    pub fn with_generic_names(mut self, names: &[&str]) -> Self {
        self.generics.extend(names.iter().map(|n| n.to_string()));
        self
    }

    pub fn with_self(mut self, self_ty: Ty) -> Self {
        self.self_ty = Some(self_ty);
        self
    }

    /// Add the bound `param: trait_name<args>`, such as the implicit
    /// `Self: Trait` inside a trait.
    pub fn with_bound(mut self, bound: Bound) -> Self {
        self.bounds.push(bound);
        self
    }

    pub fn decls(&self) -> &'d Decls<'a> {
        self.decls
    }

    /// A written trait reference, such as `From<IoError>`: the trait's
    /// name and its lowered arguments. `None` when it is not a trait.
    pub fn bound(&self, ty: &Type) -> Option<(String, Vec<Ty>)> {
        let TypeKind::Path { segments, args } = &ty.kind else {
            return None;
        };
        let name = self.decls.path_name(segments, self.see_all)?;
        let decl = self.decls.get(name)?;
        if decl.kind != DeclKind::Trait {
            return None;
        }
        let mut lowered: Vec<Ty> = args.iter().map(|a| self.ty(a)).collect();
        lowered.resize(decl.generics.len(), Ty::Error);
        Some((name.to_string(), lowered))
    }

    /// The signature of `f`, declared inside an impl with `outer`
    /// generics and `self_ty` (or neither, for a free function).
    pub fn signature(&self, f: &FnDecl, outer: &[GenericParam], self_ty: Option<Ty>) -> FnSig {
        let mut lower = self.clone().with_generics(&f.generics);
        if let Some(self_ty) = &self_ty {
            lower = lower.with_self(self_ty.clone());
        }
        let generics: Vec<String> = outer
            .iter()
            .chain(&f.generics)
            .map(|g| g.name.name.clone())
            .collect();
        let bounds = lower
            .bounds
            .iter()
            .filter(|b| generics.contains(&b.param))
            .cloned()
            .collect();
        let generic_keys = outer
            .iter()
            .chain(&f.generics)
            .map(|g| (!self.see_all).then_some(GenericKey(g.name.span)))
            .collect();
        FnSig {
            name: f.name.name.clone(),
            generics,
            generic_keys,
            bounds,
            receiver: Receiver::of(f),
            self_ty,
            params: f
                .params
                .iter()
                .map(|p| (p.name.name.clone(), lower.param_ty(&p.ty)))
                .collect(),
            ret: f.return_type.as_ref().map_or(Ty::Unit, |t| lower.ty(t)),
            trait_name: None,
            follows_trust: false,
            intrinsic: self.see_all && f.body.is_none(),
            declared: None,
            pure: !f
                .params
                .iter()
                .any(|p| capability_bearing(&lower.ty(&p.ty), self.decls)),
        }
    }

    /// A parameter's type. A function-typed parameter of a prelude or
    /// `std` function accepts a scoped closure, one that captures a
    /// `Scope` handle: they only call what they are given, as `spawn` and
    /// `map` do, and their bodies, if any, are checked with the parameter
    /// scoped, so they cannot keep it. A user function's cannot: its body
    /// could return or store the closure past its `scope` block.
    fn param_ty(&self, ty: &Type) -> Ty {
        match self.ty(ty) {
            Ty::Fn {
                cap, params, ret, ..
            } if self.see_all => Ty::Fn {
                cap,
                scoped: true,
                params,
                ret,
            },
            other => other,
        }
    }

    pub fn fields(&self, fields: &Fields) -> FieldsInfo {
        let (shape, list) = match fields {
            Fields::Named(list) => (Shape::Named, list.as_slice()),
            Fields::Positional(list) => (Shape::Positional, list.as_slice()),
            Fields::Unit => (Shape::Unit, [].as_slice()),
        };
        FieldsInfo {
            shape,
            fields: list
                .iter()
                .map(|f| (f.name.as_ref().map(|n| n.name.clone()), self.ty(&f.ty)))
                .collect(),
        }
    }

    pub fn ty(&self, ty: &Type) -> Ty {
        match &ty.kind {
            TypeKind::Unit => Ty::Unit,
            TypeKind::Fn {
                cap,
                params,
                return_type,
            } => Ty::Fn {
                cap: Cap::of(*cap),
                scoped: false,
                params: params.iter().map(|p| self.ty(p)).collect(),
                ret: Box::new(return_type.as_ref().map_or(Ty::Unit, |r| self.ty(r))),
            },
            TypeKind::Path { segments, args } => {
                if let [ident] = segments.as_slice() {
                    let name = ident.name.as_str();
                    if self.generics.iter().any(|g| g == name) {
                        return Ty::Param(name.to_string());
                    }
                    if name == ty::SELF {
                        return self.self_ty.clone().unwrap_or(Ty::Error);
                    }
                }
                let Some(name) = self.decls.path_name(segments, self.see_all) else {
                    return Ty::Error;
                };
                if segments.len() == 1 && name == ty::UNTRUSTED {
                    // Trust is a flag on text, not a type (ADR 0006).
                    return match args.as_slice() {
                        [arg] => match self.ty(arg) {
                            text if text.is_text() => text.with_trust(Trust::Untrusted),
                            _ => Ty::Error,
                        },
                        _ => Ty::Error,
                    };
                }
                match self.decls.get(name) {
                    Some(decl) if decl.kind != DeclKind::Trait => {
                        // A wrong number of arguments is a declaration error;
                        // here missing ones become errors and extras are
                        // dropped, so checking can go on.
                        let mut lowered: Vec<Ty> = args.iter().map(|a| self.ty(a)).collect();
                        lowered.resize(decl.generics.len(), Ty::Error);
                        Ty::generic(name, lowered)
                    }
                    _ => Ty::Error,
                }
            }
        }
    }
}

/// Whether a function item is marked `#[follows_trust]`. Only the prelude
/// can write the attribute: the parser rejects it in user code.
fn follows_trust(item: &Item) -> bool {
    item.attributes.iter().any(|a| a.name.name == FOLLOWS_TRUST)
}

/// The attribute that makes a prelude function trust-polymorphic.
const FOLLOWS_TRUST: &str = "follows_trust";

/// The text of a string literal with no interpolation hole.
pub(crate) fn literal_text(expr: &oska_syntax::ast::Expr) -> Option<String> {
    use oska_syntax::ast::{ExprKind, StrPart};
    let ExprKind::Str(parts) = &expr.kind else {
        return None;
    };
    let mut text = String::new();
    for part in parts {
        match part {
            StrPart::Text { value, .. } => text.push_str(value),
            StrPart::Hole { .. } => return None,
        }
    }
    Some(text)
}
