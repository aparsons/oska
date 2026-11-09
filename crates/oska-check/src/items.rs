//! The value-level view of the prelude and the module: function
//! signatures, struct and enum shapes, and methods, with every written
//! type lowered to a [`Ty`].

use std::collections::HashMap;

use oska_syntax::ast::{Fields, FnDecl, GenericParam, ItemKind, Module, Type, TypeKind};

use crate::decls::{DeclKind, Decls};
use crate::prelude;
use crate::ty::Ty;

/// A function or method signature. Generic parameters appear in the
/// types as [`Ty::Param`]; instantiate them before use.
#[derive(Debug, Clone)]
pub struct FnSig {
    pub name: String,
    /// The impl's generic parameters followed by the function's own.
    pub generics: Vec<String>,
    /// For a method: whether its receiver is `var self`. `None` for a
    /// free or associated function.
    pub receiver: Option<bool>,
    /// For a method or associated function: the type of `Self`.
    pub self_ty: Option<Ty>,
    pub params: Vec<(String, Ty)>,
    pub ret: Ty,
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

#[derive(Debug, Clone)]
pub struct StructInfo {
    pub generics: Vec<String>,
    pub fields: FieldsInfo,
}

#[derive(Debug, Clone)]
pub struct EnumInfo {
    pub generics: Vec<String>,
    pub variants: Vec<(String, FieldsInfo)>,
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
    /// Methods from `impl Trait for Type` blocks, by type name.
    pub trait_impls: HashMap<String, Vec<FnSig>>,
    /// Variants usable without their enum's name (`Some`, `None`, `Ok`,
    /// `Err`): every variant of a prelude enum, mapped to its enum.
    pub bare_variants: HashMap<String, String>,
}

impl Items {
    /// Collect the prelude's items and then `module`'s. The first of two
    /// declarations with one name wins; `Decls` reports the duplicate.
    pub fn collect(module: &Module, decls: &Decls) -> Items {
        let mut items = Items::default();
        items.add(prelude::module(), decls, true);
        items.add(module, decls, false);
        items
    }

    fn add(&mut self, module: &Module, decls: &Decls, in_prelude: bool) {
        for item in &module.items {
            match &item.kind {
                ItemKind::Fn(f) => {
                    let sig = Lower::new(decls).signature(f, &[], None);
                    self.fns.entry(sig.name.clone()).or_insert(sig);
                }
                ItemKind::Struct(s) => {
                    let lower = Lower::new(decls).with_generics(&s.generics);
                    let info = StructInfo {
                        generics: lower.generics.clone(),
                        fields: lower.fields(&s.fields),
                    };
                    self.structs.entry(s.name.name.clone()).or_insert(info);
                }
                ItemKind::Enum(e) => {
                    let lower = Lower::new(decls).with_generics(&e.generics);
                    let variants = e
                        .variants
                        .iter()
                        .map(|v| (v.name.name.clone(), lower.fields(&v.fields)))
                        .collect::<Vec<_>>();
                    if in_prelude {
                        for (variant, _) in &variants {
                            self.bare_variants
                                .insert(variant.clone(), e.name.name.clone());
                        }
                    }
                    let info = EnumInfo {
                        generics: lower.generics.clone(),
                        variants,
                    };
                    self.enums.entry(e.name.name.clone()).or_insert(info);
                }
                ItemKind::Impl(imp) => {
                    let lower = Lower::new(decls).with_generics(&imp.generics);
                    let target = lower.ty(&imp.target);
                    let Some(type_name) = target.head().map(str::to_string) else {
                        continue;
                    };
                    let table = if imp.trait_ref.is_some() {
                        &mut self.trait_impls
                    } else {
                        &mut self.inherent
                    };
                    for method in &imp.methods {
                        if let ItemKind::Fn(f) = &method.kind {
                            let sig = lower.signature(f, &imp.generics, Some(target.clone()));
                            table.entry(type_name.clone()).or_default().push(sig);
                        }
                    }
                }
                ItemKind::Use(_) | ItemKind::Const(_) | ItemKind::Trait(_) | ItemKind::Test(_) => {}
            }
        }
    }

    /// The method or associated function `name` of the type `type_name`:
    /// inherent methods first, then methods of implemented traits.
    pub fn method(&self, type_name: &str, name: &str) -> Option<&FnSig> {
        [&self.inherent, &self.trait_impls]
            .into_iter()
            .filter_map(|table| table.get(type_name))
            .flatten()
            .find(|s| s.name == name)
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
    pub self_ty: Option<Ty>,
}

impl<'d, 'a> Lower<'d, 'a> {
    pub fn new(decls: &'d Decls<'a>) -> Self {
        Lower {
            decls,
            generics: Vec::new(),
            self_ty: None,
        }
    }

    /// Bring `generics` into scope as well.
    pub fn with_generics(mut self, generics: &[GenericParam]) -> Self {
        self.generics
            .extend(generics.iter().map(|g| g.name.name.clone()));
        self
    }

    pub fn with_self(mut self, self_ty: Ty) -> Self {
        self.self_ty = Some(self_ty);
        self
    }

    pub fn decls(&self) -> &'d Decls<'a> {
        self.decls
    }

    /// The signature of `f`, declared inside an impl with `outer`
    /// generics and `self_ty` (or neither, for a free function).
    pub fn signature(&self, f: &FnDecl, outer: &[GenericParam], self_ty: Option<Ty>) -> FnSig {
        let mut lower = self.clone().with_generics(&f.generics);
        if let Some(self_ty) = &self_ty {
            lower = lower.with_self(self_ty.clone());
        }
        let generics = outer
            .iter()
            .chain(&f.generics)
            .map(|g| g.name.name.clone())
            .collect();
        FnSig {
            name: f.name.name.clone(),
            generics,
            receiver: f.receiver.as_ref().map(|r| r.mutable),
            self_ty,
            params: f
                .params
                .iter()
                .map(|p| (p.name.name.clone(), lower.ty(&p.ty)))
                .collect(),
            ret: f.return_type.as_ref().map_or(Ty::Unit, |t| lower.ty(t)),
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
                cap: *cap,
                params: params.iter().map(|p| self.ty(p)).collect(),
                ret: Box::new(return_type.as_ref().map_or(Ty::Unit, |r| self.ty(r))),
            },
            TypeKind::Path { segments, args } => {
                let [ident] = segments.as_slice() else {
                    return Ty::Error;
                };
                let name = ident.name.as_str();
                if self.generics.iter().any(|g| g == name) {
                    return Ty::Param(name.to_string());
                }
                if name == "Self" {
                    return self.self_ty.clone().unwrap_or(Ty::Error);
                }
                match self.decls.get(name) {
                    Some(decl) if decl.kind != DeclKind::Trait => {
                        // A wrong number of arguments is a declaration error;
                        // here missing ones become errors and extras are
                        // dropped, so checking can go on.
                        let mut lowered: Vec<Ty> = args.iter().map(|a| self.ty(a)).collect();
                        lowered.resize(decl.generics.len(), Ty::Error);
                        Ty::Named(name.to_string(), lowered)
                    }
                    _ => Ty::Error,
                }
            }
        }
    }
}
