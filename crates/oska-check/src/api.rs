// SPDX-License-Identifier: Apache-2.0
//! The API reference: the public items of the prelude and the user-facing
//! `std` modules, as `oskac doc` prints them and oskalang.dev publishes
//! them (spec #294).
//!
//! Every item comes from the checker's own parse of the module, and its
//! authority from the same code as the authority report, so the reference
//! cannot drift from what the checker accepts. Nothing is parsed apart.
//!
//! An item's description is the block of plain `//` comment lines directly
//! above it, with no blank line between. That is a reading convention, not
//! syntax: Oska has no doc comments (ADR 0014 freezes new syntax).

use oska_syntax::ast::{Fields, FnDecl, GenericParam, Item, ItemKind, Module};
use oska_syntax::Diagnostic;

use crate::authority;
use crate::collect::{DeclKind, Decls};
use crate::items::Items;
use crate::link::Modules;
use crate::prelude;
use crate::ty::{Trust, Ty};

/// The name of the module that holds the prelude's items.
pub const PRELUDE: &str = "prelude";

/// The modules the reference documents, in the order it lists them.
/// `std.rt` is left out: it is the runtime's system-call layer, not user
/// API.
pub const MODULES: &[&str] = &[
    PRELUDE,
    "std.html",
    "std.json",
    "std.mcp",
    "std.sql",
    "std.proc",
    "std.sha2",
    "std.dns",
    "std.hmac",
    "std.ed25519",
    "std.sshsig",
    "std.ecdsa",
    "std.rsa",
    "std.p256",
    "std.x509",
    "std.hkdf",
    "std.x25519",
    "std.chacha20poly1305",
    "std.aes128gcm",
    "std.tls",
    "std.http",
    "std.map",
    "std.store",
];

/// What kind of item an entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The module itself, described by the comment at the top of its file.
    Module,
    Fn,
    Struct,
    Enum,
    Trait,
    /// A built-in capability: a prelude struct marked `#[capability]`.
    Capability,
    /// A method of an inherent `impl`, or of a trait.
    Method,
    /// `impl Trait for Type`.
    Impl,
}

impl Kind {
    /// The kind as the reference names it.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Module => "module",
            Kind::Fn => "fn",
            Kind::Struct => "struct",
            Kind::Enum => "enum",
            Kind::Trait => "trait",
            Kind::Capability => "capability",
            Kind::Method => "method",
            Kind::Impl => "impl",
        }
    }
}

/// One entry of the reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiItem {
    /// `prelude`, or the path a `use` names, such as `std.sql`.
    pub module: String,
    /// The item's name: `len` for a function, `List.push` for a method,
    /// `Eq for Int` for a trait impl, and the module's path for a module.
    /// A method that impls of one type with different type arguments
    /// share is named with its impl's type as written, as `Secret<U8>.rotl`
    /// and `Secret<U32>.rotl`, so that each has an anchor of its own.
    pub name: String,
    pub kind: Kind,
    /// The declaration as written, on one line, without a body or private
    /// fields. Empty for a module.
    pub signature: String,
    /// The comment block directly above the item, without its `//`
    /// markers. Lines are joined with spaces; an empty comment line
    /// between two lines starts a new paragraph (`\n\n`). Empty when there
    /// is no comment.
    pub description: String,
    /// The built-in capabilities the item reaches, in the authority
    /// report's order: what a function can reach through its receiver and
    /// parameters, or what a value of a type carries.
    pub capabilities: Vec<String>,
    /// The item's authority in the authority report's words: `pure`, or
    /// what each capability-bearing parameter supplies, as `FsRead (self)`.
    /// A type that carries capabilities names them, as `NetConnect`.
    pub authority: String,
}

impl ApiItem {
    /// The item's anchor on its module's page, made from its name as the
    /// site makes a heading's: lower-cased, a space as `-`, and only
    /// letters, digits, `-` and `_` kept. `List.push` is `listpush`, and
    /// `Eq for Int` is `eq-for-int`.
    pub fn anchor(&self) -> String {
        let mut out = String::new();
        for c in self.name.to_lowercase().chars() {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                out.push(c);
            } else if c == ' ' {
                out.push('-');
            }
        }
        out
    }

    /// The name without a method's type arguments: `Secret.rotl` for
    /// `Secret<U8>.rotl`, and without a function's module: `sha256` for
    /// `sha2.sha256`. Any other name is unchanged.
    pub fn unqualified_name(&self) -> String {
        if self.kind == Kind::Fn {
            return self.name.rsplit('.').next().unwrap_or("").to_string();
        }
        if self.kind != Kind::Method {
            return self.name.clone();
        }
        match (self.name.find('<'), self.name.rfind(">.")) {
            (Some(open), Some(close)) if open < close => {
                format!("{}{}", &self.name[..open], &self.name[close + 1..])
            }
            _ => self.name.clone(),
        }
    }
}

/// Every item of the reference: each module of [`MODULES`], in order.
pub fn toolchain_items() -> Vec<ApiItem> {
    let empty = Module {
        items: Vec::new(),
        span: oska_syntax::Span::at(0),
    };
    let mut out = Vec::new();
    for path in MODULES {
        if *path == PRELUDE {
            let mut diagnostics = Vec::new();
            let decls = Decls::collect(&empty, &mut diagnostics);
            let items = Items::collect(&empty, &decls);
            out.extend(items_of(
                PRELUDE,
                prelude::SOURCE,
                prelude::module(),
                &decls,
                &items,
            ));
        } else {
            let std = prelude::std_module(path).expect("every listed module is a std module");
            out.extend(std_items(path, std.source, std.module()));
        }
    }
    out
}

/// The items of `src`, a module written in prelude mode like the `std`
/// modules, as the reference would list it under `path`. For tests of the
/// reading rules on fixture modules.
pub fn module_items(path: &str, src: &str) -> Result<Vec<ApiItem>, Diagnostic> {
    let module = oska_syntax::parse_prelude(src)?;
    Ok(std_items(path, src, &module))
}

fn std_items(path: &str, src: &str, module: &Module) -> Vec<ApiItem> {
    // The module's own declarations are collected as a user module's
    // are, after the prelude's.
    let mut diagnostics = Vec::new();
    let decls = Decls::collect(module, &mut diagnostics);
    let items = Items::collect(module, &decls);
    items_of(path, src, module, &decls, &items)
}

fn items_of(path: &str, src: &str, module: &Module, decls: &Decls, items: &Items) -> Vec<ApiItem> {
    let functions = authority::signatures(module, decls, items, &Modules::default());
    let authority_of = |name: &str| {
        functions
            .iter()
            .find(|f| f.name == name)
            .map(|f| Reach {
                capabilities: f.authority.clone(),
                authority: f.authority_words(),
            })
            .unwrap_or_else(Reach::pure)
    };
    let mut out = vec![ApiItem {
        module: path.to_string(),
        name: path.to_string(),
        kind: Kind::Module,
        signature: String::new(),
        description: module_description(src),
        capabilities: Vec::new(),
        authority: String::new(),
    }];
    let item = |name: String, kind: Kind, signature: String, at: &Item, reach: Reach| ApiItem {
        module: path.to_string(),
        name,
        kind,
        signature,
        description: description(src, at.span.start),
        capabilities: reach.capabilities,
        authority: reach.authority,
    };
    // Each inherent method's index in `out` and its impl's type as
    // written, to tell apart a method that impls of one type share.
    let mut methods: Vec<(usize, String)> = Vec::new();
    for it in &module.items {
        match &it.kind {
            ItemKind::Fn(f) if it.is_public() => {
                let name = f.name.name.clone();
                let reach = authority_of(&name);
                out.push(item(name, Kind::Fn, fn_signature(src, it, f), it, reach));
            }
            ItemKind::Struct(s) if it.is_public() => {
                let capability = it.attributes.iter().any(|a| a.name.name == "capability");
                let kind = if capability {
                    Kind::Capability
                } else {
                    Kind::Struct
                };
                let mut sig = format!(
                    "pub {}struct {}{}",
                    if s.cap { "cap " } else { "" },
                    s.name.name,
                    generics(src, &s.generics)
                );
                if let Fields::Named(fields) = &s.fields {
                    let public: Vec<&str> = fields
                        .iter()
                        .filter(|f| f.public)
                        .map(|f| text(src, f.span.start, f.span.end))
                        .collect();
                    if !public.is_empty() {
                        sig.push_str(&format!(" {{ {} }}", public.join(", ")));
                    }
                }
                let reach = type_reach(decls, &s.name.name, &s.generics);
                out.push(item(s.name.name.clone(), kind, one_line(&sig), it, reach));
            }
            ItemKind::Enum(e) if it.is_public() => {
                let variants: Vec<&str> = e
                    .variants
                    .iter()
                    .map(|v| text(src, v.span.start, v.span.end))
                    .collect();
                let sig = format!(
                    "pub {}enum {}{} {{ {} }}",
                    if e.cap { "cap " } else { "" },
                    e.name.name,
                    generics(src, &e.generics),
                    variants.join(", ")
                );
                let reach = type_reach(decls, &e.name.name, &e.generics);
                out.push(item(
                    e.name.name.clone(),
                    Kind::Enum,
                    one_line(&sig),
                    it,
                    reach,
                ));
            }
            ItemKind::Trait(t) if it.is_public() => {
                let sig = format!("pub trait {}{}", t.name.name, generics(src, &t.generics));
                let none = Reach::pure();
                out.push(item(t.name.name.clone(), Kind::Trait, sig, it, none));
                for method in &t.methods {
                    if let ItemKind::Fn(f) = &method.kind {
                        let name = format!("{}.{}", t.name.name, f.name.name);
                        let reach = authority_of(&name);
                        let sig = fn_signature(src, method, f);
                        out.push(item(name, Kind::Method, sig, method, reach));
                    }
                }
            }
            ItemKind::Impl(imp) => {
                let target = authority::type_name(&imp.target);
                if let Some(trait_ref) = &imp.trait_ref {
                    let trait_text = text(src, trait_ref.span.start, trait_ref.span.end);
                    let target_text = text(src, imp.target.span.start, imp.target.span.end);
                    let name = one_line(&format!("{trait_text} for {target_text}"));
                    let sig = one_line(&format!("impl{} {name}", generics(src, &imp.generics)));
                    let none = Reach::pure();
                    out.push(item(name, Kind::Impl, sig, it, none));
                    continue;
                }
                for method in &imp.methods {
                    if let ItemKind::Fn(f) = &method.kind {
                        if method.is_public() {
                            let name = format!("{target}.{}", f.name.name);
                            let reach = authority_of(&name);
                            let sig = fn_signature(src, method, f);
                            let written = text(src, imp.target.span.start, imp.target.span.end);
                            methods.push((out.len(), one_line(written)));
                            out.push(item(name, Kind::Method, sig, method, reach));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    qualify_shared_methods(&mut out, &methods);
    qualify_clashing_functions(path, &mut out);
    out
}

/// Names a function whose anchor a type of the same module has too, as
/// `sha256` and `Sha256` do in `std.sha2` (the site lower-cases names), with
/// the module's name: `sha2.sha256`. That is how a program calls it, and the
/// anchor is then its own. Every other name, and so its anchor, is left as
/// it is.
fn qualify_clashing_functions(path: &str, out: &mut [ApiItem]) {
    let types: Vec<String> = out
        .iter()
        .filter(|i| {
            matches!(
                i.kind,
                Kind::Struct | Kind::Enum | Kind::Trait | Kind::Capability
            )
        })
        .map(ApiItem::anchor)
        .collect();
    let module = path.rsplit('.').next().unwrap_or(path);
    for item in out.iter_mut() {
        if item.kind == Kind::Fn && types.contains(&item.anchor()) {
            item.name = format!("{module}.{}", item.name);
        }
    }
}

/// Names each method that impls of one type with different type arguments
/// share with its impl's type as written: `Secret<U8>.rotl` and
/// `Secret<U32>.rotl` rather than `Secret.rotl` twice. Every other name,
/// and so its anchor, is left as it is.
fn qualify_shared_methods(out: &mut [ApiItem], methods: &[(usize, String)]) {
    let shared: Vec<bool> = methods
        .iter()
        .map(|(i, written)| {
            methods
                .iter()
                .any(|(j, other)| out[*j].name == out[*i].name && other != written)
        })
        .collect();
    for ((i, written), shared) in methods.iter().zip(shared) {
        if shared {
            let method = out[*i].name.rsplit('.').next().unwrap_or("").to_string();
            out[*i].name = format!("{written}.{method}");
        }
    }
}

/// What an item reaches: the capabilities, and the words for them.
struct Reach {
    capabilities: Vec<String>,
    authority: String,
}

impl Reach {
    /// Reaching nothing.
    fn pure() -> Reach {
        Reach {
            capabilities: Vec::new(),
            authority: "pure".to_string(),
        }
    }
}

/// What a value of the type `name` carries: the capabilities it reaches
/// with its type parameters left open, and the words for them.
fn type_reach(decls: &Decls, name: &str, params: &[GenericParam]) -> Reach {
    let args = params
        .iter()
        .map(|g| Ty::Param(g.name.name.as_str().into()))
        .collect();
    let ty = Ty::Named(name.into(), args, Trust::Trusted);
    let capabilities = match decls.get(name).map(|d| &d.kind) {
        Some(DeclKind::Capability) => vec![name.to_string()],
        _ => authority::capabilities_of(&ty, decls),
    };
    let words = if capabilities.is_empty() {
        "pure".to_string()
    } else {
        capabilities.join(", ")
    };
    Reach {
        capabilities,
        authority: words,
    }
}

/// A function's signature: its source, attributes included, up to its
/// body, on one line.
fn fn_signature(src: &str, item: &Item, f: &FnDecl) -> String {
    let end = f.body.as_ref().map_or(item.span.end, |b| b.span.start);
    one_line(text(src, item.span.start, end))
}

/// `<T, E>`, as written, or nothing.
fn generics(src: &str, params: &[GenericParam]) -> String {
    match (params.first(), params.last()) {
        (Some(first), Some(last)) => format!("<{}>", text(src, first.span.start, last.span.end)),
        _ => String::new(),
    }
}

fn text(src: &str, start: usize, end: usize) -> &str {
    src.get(start..end).unwrap_or("")
}

/// `text` with every run of whitespace a single space, and no space just
/// inside a bracket or before a comma.
fn one_line(text: &str) -> String {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    joined
        .replace("( ", "(")
        .replace(" )", ")")
        .replace(", }", " }")
        .replace(",)", ")")
        .replace(" ,", ",")
}

/// The comment block on the lines directly above byte `start` of `src`:
/// the lines from the one just before `start`'s line, upward, while each
/// is a `//` comment. A blank line, code, or the top of the file ends it.
fn description(src: &str, start: usize) -> String {
    let line_start = src[..start].rfind('\n').map_or(0, |i| i + 1);
    let before = &src[..line_start];
    let mut lines: Vec<&str> = Vec::new();
    for line in before.lines().rev() {
        let line = line.trim();
        match line.strip_prefix("//") {
            Some(rest) => lines.push(rest.strip_prefix(' ').unwrap_or(rest).trim_end()),
            None => break,
        }
    }
    lines.reverse();
    paragraphs(&lines)
}

/// The comment block at the top of `src`, when a blank line separates it
/// from what follows: the module's own description.
fn module_description(src: &str) -> String {
    let mut lines: Vec<&str> = Vec::new();
    let mut rest = src.lines();
    for line in rest.by_ref() {
        let line = line.trim();
        match line.strip_prefix("//") {
            Some(text) => lines.push(text.strip_prefix(' ').unwrap_or(text).trim_end()),
            None if line.is_empty() => return paragraphs(&lines),
            // A comment that runs into a declaration describes it, not the
            // module.
            None => return String::new(),
        }
    }
    paragraphs(&lines)
}

/// Comment lines joined with spaces, an empty line starting a new
/// paragraph.
fn paragraphs(lines: &[&str]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in lines {
        if line.is_empty() {
            if !current.is_empty() {
                out.push(current.join(" "));
                current.clear();
            }
        } else {
            current.push(line);
        }
    }
    if !current.is_empty() {
        out.push(current.join(" "));
    }
    out.join("\n\n")
}
