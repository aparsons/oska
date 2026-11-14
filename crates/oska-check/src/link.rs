//! Linking a package's modules into the one module the checker checks.
//!
//! The modules' texts are joined end to end into one text, and each is
//! parsed at its offset in it (`package.rs`), so every span is unique
//! across the package. Linking then resolves each module's `use pkg.…`
//! imports and rewrites the module so that every name it writes means the
//! same thing in the joined module:
//!
//! - Every item is declared under its **key** ([`ItemKey`]): `pkg.`, the
//!   module path and the name, such as `pkg.util.text.shout`, or
//!   `pkg.main.helper` in the entry module. The one exception is the entry
//!   module's `main`, which keeps its name: the runtime starts there. An
//!   identifier never contains `.`, so no source can write a key: it is
//!   reached only through linking, and no two modules' names, nor a name
//!   and another module's `std` import, can clash in the joined module.
//! - A reference to an item, written plainly (`shout`, after `use
//!   pkg.util.text.{shout}` or in its own module) or through a qualifier
//!   (`text.shout`), becomes its key. Locals, generic parameters and
//!   `Self` shadow items, as they do in the checker.
//! - Every `use pkg.…` is removed; `use std.…` stays.
//!
//! Linking reports what the joined module cannot see: unknown modules
//! and names, private items used from another module, import cycles,
//! names bound twice, and a name that one module would see only because
//! another imports (a `std` import is per module). Everything else, every
//! type, trust, capability and contract rule, is the checker's, over the
//! whole package at once.
//!
//! A key is internal: [`Keys::display`] turns every key in a message
//! into the name code in the message's module writes.

use std::collections::{HashMap, HashSet};

use oska_syntax::ast::{
    Block, Expr, ExprKind, FnDecl, GenericParam, Ident, Item, ItemKind, Module, Pattern,
    PatternKind, StmtKind, StrPart, Type, TypeKind, UseDecl, Visibility,
};
use oska_syntax::{Diagnostic, Edit, Span};

use crate::codes;
use crate::package::ENTRY_MODULE;
use crate::prelude::{self, StdModule};
use crate::suggest;
use crate::ty;

/// The first segment of a `use` path that names a module of this package.
pub const PKG: &str = "pkg";

/// One parsed module of a package, its spans offsets into the joined text.
pub struct Parsed {
    /// The module path, such as `util.text`.
    pub module: String,
    pub ast: Module,
}

/// The joined module, and what linking reported.
pub struct Linked {
    pub module: Module,
    pub diagnostics: Vec<Diagnostic>,
    /// The span of every reference linking reported. Linking leaves such
    /// a reference as written, so the checker reports it again (an
    /// unknown name, say): a diagnostic of the joined module inside one of
    /// them is the same error, and is dropped.
    pub reported: Vec<Span>,
    /// Every item's key.
    pub keys: Keys,
}

/// Which module of the package each part of the joined text is in, for
/// the rules that depend on where code is: privacy of fields, methods and
/// traits (`body.rs`), and the names of the authority report
/// (`authority.rs`). A single file has none: all its code is in one
/// module.
#[derive(Debug, Default)]
pub struct Modules {
    /// Each module's path and the span of its text.
    ranges: Vec<(String, Span)>,
}

impl Modules {
    pub fn new(ranges: Vec<(String, Span)>) -> Modules {
        Modules { ranges }
    }

    /// The path of the module whose text contains `offset`, such as
    /// `util.text`.
    pub fn at(&self, offset: usize) -> &str {
        self.ranges
            .iter()
            .find(|(_, span)| span.start <= offset && offset <= span.end)
            .map_or(ENTRY_MODULE, |(module, _)| module.as_str())
    }

    /// The name the authority report gives a function or method declared
    /// at `offset`, whose type or trait, for a method, is `type_key`. In
    /// a single file, the name, or `Type.name` for a method. In a package,
    /// `pkg.`, the module path and the name, as for an [`ItemKey`]; a
    /// method is named by its type's module, wherever its `impl` is, so
    /// every name is the package's one function with that name: an
    /// `impl util.Box` in `main` gives `pkg.util.Box.leak`, never
    /// `pkg.main.…`. A package's names start with `pkg.` so that none reads
    /// as a `std` module's, even a module named `html`.
    pub fn function_name(&self, offset: usize, type_key: Option<&str>, name: &str) -> String {
        if self.ranges.is_empty() {
            return match type_key {
                Some(ty) => format!("{ty}.{name}"),
                None => name.to_string(),
            };
        }
        let here = self.at(offset);
        let name = ItemKey::parse(name).map_or(name, |key| key.name);
        match type_key {
            Some(ty) => {
                let ty = ItemKey::parse(ty).unwrap_or(ItemKey {
                    module: here,
                    name: ty,
                });
                format!("{}.{name}", ty.joined())
            }
            None => ItemKey { module: here, name }.joined(),
        }
    }

    /// The name the authority report gives the method `name` of `impl
    /// trait_ref for type_key`: `<Type as Trait>.name`, as the checked
    /// program names it ([`crate::program::trait_method`]). `trait_ref`
    /// carries a generic trait's arguments, as in `From<IoError>`, so the
    /// name is shared neither with an inherent method of the type nor with
    /// the same method of another impl (#123). In a package the type and
    /// the trait are named by their keys, and a prelude one plainly, as in
    /// `<Int as pkg.geo.Area>.area`: the orphan rule makes one of the two
    /// the package's own, so the name is the package's alone.
    pub fn trait_method_name(&self, type_key: &str, trait_ref: &str, name: &str) -> String {
        let name = ItemKey::parse(name).map_or(name, |key| key.name);
        crate::program::trait_method(type_key, trait_ref, name)
    }
}

/// The name an item of a package has in the joined module: `pkg.`, its
/// module path and its name, such as `pkg.util.text.shout`. Keys are
/// internal to checking a package; code names an item as [`ItemKey::in_module`]
/// says, and messages do too ([`Keys::display`]). The authority report
/// names functions with the whole key, which is unique in the package and
/// never reads as a `std` name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemKey<'a> {
    /// The module path, such as `util.text`.
    module: &'a str,
    name: &'a str,
}

impl<'a> ItemKey<'a> {
    /// The key of `name`, declared by `module`.
    pub fn new(module: &'a str, name: &'a str) -> ItemKey<'a> {
        ItemKey { module, name }
    }

    /// The key a name of the joined module is, if it is one:
    /// `pkg.util.text.shout` is `shout` of `util.text`.
    pub fn parse(joined: &'a str) -> Option<ItemKey<'a>> {
        let rest = joined.strip_prefix(PKG)?.strip_prefix('.')?;
        let (module, name) = rest.rsplit_once('.')?;
        Some(ItemKey { module, name })
    }

    /// The name the item has in the joined module.
    pub fn joined(self) -> String {
        format!("{PKG}.{}.{}", self.module, self.name)
    }

    /// The item's own name, such as `shout`.
    pub fn name(self) -> &'a str {
        self.name
    }

    /// The item as code in `module` names it: plainly in its own module,
    /// and otherwise by its module path and name, as in `util.text.shout`.
    pub fn in_module(self, module: &str) -> String {
        if self.module == module {
            self.name.to_string()
        } else {
            format!("{}.{}", self.module, self.name)
        }
    }
}

/// Every key of a package's items, to take keys out of messages.
#[derive(Debug, Default)]
pub struct Keys {
    keys: HashSet<String>,
}

impl Keys {
    /// The keys of `declared`, each a module path and an item name it
    /// declares.
    pub fn new<'a>(declared: impl IntoIterator<Item = (&'a str, &'a str)>) -> Keys {
        Keys {
            keys: declared
                .into_iter()
                .map(|(module, name)| key(module, name))
                .collect(),
        }
    }

    /// `text`, a message about code in `module`, with every key in it
    /// written as that code names the item ([`ItemKey::in_module`]). This
    /// is the one place a key becomes text a person reads: the checker's
    /// messages and the runtime's panics, which name items of the joined
    /// module, pass through it. A key is found where `pkg.` starts a
    /// dotted name and a prefix of that name, ending at a `.`, is a key
    /// of the package, so `pkg.util.Box.leak` is `util.Box` and `.leak`.
    pub fn display(&self, text: &str, module: &str) -> String {
        if self.keys.is_empty() {
            return text.to_string();
        }
        let mut out = String::with_capacity(text.len());
        let mut copied = 0;
        let mut from = 0;
        while let Some(found) = text[from..].find(PKG) {
            let at = from + found;
            from = at + PKG.len();
            let boundary = text[..at]
                .chars()
                .next_back()
                .map_or(true, |c| !is_name_char(c) && c != '.');
            if !boundary {
                continue;
            }
            let len = text[at..]
                .find(|c: char| !is_name_char(c) && c != '.')
                .unwrap_or(text.len() - at);
            let dotted = &text[at..at + len];
            let longest = dotted
                .match_indices('.')
                .map(|(dot, _)| dot)
                .chain([dotted.len()])
                .rev()
                .find(|&end| self.keys.contains(&dotted[..end]));
            let Some(end) = longest else {
                continue;
            };
            let Some(item) = ItemKey::parse(&dotted[..end]) else {
                continue;
            };
            out.push_str(&text[copied..at]);
            out.push_str(&item.in_module(module));
            copied = at + end;
            from = copied;
        }
        out.push_str(&text[copied..]);
        out
    }
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The name a declaration of `module` has in the joined module: its key,
/// or `main` for the entry module's `main`, where the runtime starts.
pub fn key(module: &str, name: &str) -> String {
    if module == ENTRY_MODULE && name == ENTRY_MODULE {
        name.to_string()
    } else {
        ItemKey::new(module, name).joined()
    }
}

/// The name an item declares, without its module: `Item` for
/// `pkg.util.Item`.
pub fn unqualified(name: &str) -> &str {
    ItemKey::parse(name).map_or(name, ItemKey::name)
}

/// An item a module declares.
struct Declared {
    key: String,
    visibility: Visibility,
    /// Where `pub(pkg) ` goes to make it visible: after its attributes.
    pub_at: usize,
}

/// What one module can name.
#[derive(Default)]
struct Scope {
    /// Its own items and the items it imports by name, by the name it
    /// writes, to their keys.
    names: HashMap<String, String>,
    /// The modules it imports as modules, by qualifier.
    qualifiers: HashMap<String, usize>,
    /// The `std` modules it imports, in any form.
    std_modules: Vec<&'static StdModule>,
    /// The `std` modules a plain `use` binds, by their name.
    std_bound: Vec<&'static StdModule>,
    /// The names a `std` brace group imports.
    std_names: HashSet<String>,
    /// The modules it imports, with the span of each `use` path.
    imports: Vec<(usize, Span)>,
}

/// Link `modules`, the parsed modules of a package, whose joined text is
/// `text`.
pub fn link(text: &str, modules: Vec<Parsed>) -> Linked {
    let mut linker = Linker {
        paths: modules.iter().map(|m| m.module.clone()).collect(),
        declared: modules.iter().map(|m| declared(text, m)).collect(),
        entry: modules.iter().position(|m| m.module == ENTRY_MODULE),
        module_starts: modules.iter().map(|m| m.ast.span.start).collect(),
        scopes: Vec::new(),
        diagnostics: Vec::new(),
        reported: Vec::new(),
    };
    for (index, parsed) in modules.iter().enumerate() {
        let scope = linker.scope(index, &parsed.ast);
        linker.scopes.push(scope);
    }
    linker.cycles();
    let package_std = PackageStd::of(&linker.scopes);
    let mut items = Vec::new();
    for (index, parsed) in modules.into_iter().enumerate() {
        let mut rewriter = Rewriter {
            linker: &mut linker,
            package_std: &package_std,
            module: index,
            locals: Vec::new(),
            generics: Vec::new(),
        };
        for mut item in parsed.ast.items {
            if is_pkg_use(&item) {
                continue;
            }
            rewriter.item(&mut item);
            items.push(item);
        }
    }
    let keys = Keys::new(linker.paths.iter().zip(&linker.declared).flat_map(
        |(module, declared)| {
            declared
                .keys()
                .map(move |name| (module.as_str(), name.as_str()))
        },
    ));
    Linked {
        keys,
        module: Module {
            items,
            span: Span {
                start: 0,
                end: text.len(),
            },
        },
        diagnostics: linker.diagnostics,
        reported: linker.reported,
    }
}

fn is_pkg_use(item: &Item) -> bool {
    matches!(&item.kind, ItemKind::Use(u) if u.path.first().is_some_and(|s| s.name == PKG))
}

/// Every item `parsed` declares, by name.
fn declared(text: &str, parsed: &Parsed) -> HashMap<String, Declared> {
    let mut declared = HashMap::new();
    for item in &parsed.ast.items {
        let Some(name) = item_name(item) else {
            continue;
        };
        declared.entry(name.name.clone()).or_insert(Declared {
            key: key(&parsed.module, &name.name),
            visibility: item.visibility,
            pub_at: crate::items::Declared::of(item).marker_at(text),
        });
    }
    declared
}

/// The names of the items `module` declares, as its source writes them.
pub fn declared_names(module: &Module) -> impl Iterator<Item = &str> {
    module
        .items
        .iter()
        .filter_map(item_name)
        .map(|ident| ident.name.as_str())
}

/// The name of the function, type, trait or constant `item` declares.
fn item_name(item: &Item) -> Option<&Ident> {
    match &item.kind {
        ItemKind::Fn(f) => Some(&f.name),
        ItemKind::Struct(s) => Some(&s.name),
        ItemKind::Enum(e) => Some(&e.name),
        ItemKind::Trait(t) => Some(&t.name),
        ItemKind::Const(c) => Some(&c.name),
        ItemKind::Use(_) | ItemKind::Test(_) | ItemKind::Impl(_) => None,
    }
}

fn item_name_mut(item: &mut Item) -> Option<&mut Ident> {
    match &mut item.kind {
        ItemKind::Fn(f) => Some(&mut f.name),
        ItemKind::Struct(s) => Some(&mut s.name),
        ItemKind::Enum(e) => Some(&mut e.name),
        ItemKind::Trait(t) => Some(&mut t.name),
        ItemKind::Const(c) => Some(&mut c.name),
        ItemKind::Use(_) | ItemKind::Test(_) | ItemKind::Impl(_) => None,
    }
}

/// The span from the start of `first` to the end of `last`.
fn join(first: Span, last: Span) -> Span {
    Span {
        start: first.start,
        end: last.end,
    }
}

fn path_span(path: &[Ident]) -> Span {
    match (path.first(), path.last()) {
        (Some(first), Some(last)) => join(first.span, last.span),
        _ => Span { start: 0, end: 0 },
    }
}

struct Linker {
    /// Each module's path, in package order.
    paths: Vec<String>,
    declared: Vec<HashMap<String, Declared>>,
    /// The entry module, `main`, if the package has one.
    entry: Option<usize>,
    /// Where each module's text starts in the joined text.
    module_starts: Vec<usize>,
    scopes: Vec<Scope>,
    diagnostics: Vec<Diagnostic>,
    reported: Vec<Span>,
}

impl Linker {
    /// The scope of module `index`: its own items, then its imports in
    /// order, reporting each import that fails.
    fn scope(&mut self, index: usize, ast: &Module) -> Scope {
        let mut scope = Scope::default();
        let mut names: Vec<String> = self.declared[index].keys().cloned().collect();
        names.sort_unstable();
        for name in names {
            self.not_a_prelude_name(ast, &name);
        }
        for (name, declared) in &self.declared[index] {
            scope.names.insert(name.clone(), declared.key.clone());
        }
        for item in &ast.items {
            if let ItemKind::Use(u) = &item.kind {
                self.use_decl(index, u, &mut scope);
            }
        }
        self.not_imported_and_declared(index, ast, &scope);
        scope
    }

    /// A module declares its items under keys, which never clash with
    /// the prelude's names; report the clash a single file would.
    fn not_a_prelude_name(&mut self, ast: &Module, name: &str) {
        let in_prelude = prelude::PRIMITIVES.contains(&name)
            || name == ty::UNTRUSTED
            || prelude::module()
                .items
                .iter()
                .any(|item| prelude::item_name(item) == Some(name));
        if !in_prelude {
            return;
        }
        let Some((item, ident)) = ast.items.iter().find_map(|item| {
            item_name(item)
                .filter(|i| i.name == name)
                .map(|i| (item, i))
        }) else {
            return;
        };
        let message = match item.kind {
            ItemKind::Fn(_) => format!("the function `{name}` is already declared in the prelude"),
            _ => format!("`{name}` is already declared in the prelude"),
        };
        self.diagnostics
            .push(Diagnostic::new(codes::DUPLICATE_NAME, message, ident.span));
    }

    /// A `std` brace group that imports a name the module also declares,
    /// which a single file reports as declared twice.
    fn not_imported_and_declared(&mut self, index: usize, ast: &Module, scope: &Scope) {
        for item in &ast.items {
            let Some(ident) = item_name(item) else {
                continue;
            };
            if scope.std_names.contains(&ident.name)
                && self.declared[index].contains_key(&ident.name)
            {
                self.diagnostics.push(Diagnostic::new(
                    codes::DUPLICATE_NAME,
                    format!("`{}` is already declared in the prelude", ident.name),
                    ident.span,
                ));
            }
        }
    }

    fn use_decl(&mut self, index: usize, u: &UseDecl, scope: &mut Scope) {
        let Some(first) = u.path.first() else {
            return;
        };
        if first.name == "std" {
            self.std_use(u, scope);
            return;
        }
        if first.name != PKG {
            // Name resolution reports it.
            return;
        }
        let span = path_span(&u.path);
        let written: Vec<&str> = u.path[1..].iter().map(|s| s.name.as_str()).collect();
        let written = written.join(".");
        let Some(target) = self.paths.iter().position(|p| *p == written) else {
            self.unknown_module(&written, span);
            return;
        };
        scope.imports.push((target, span));
        match &u.names {
            None => {
                let Some(last) = u.path.last() else {
                    return;
                };
                if let Some(earlier) = self.bound(scope, &last.name, Some(target)) {
                    self.bound_twice(&last.name, earlier, last.span);
                    return;
                }
                scope.qualifiers.insert(last.name.clone(), target);
            }
            Some(names) => {
                for name in names {
                    self.import_name(index, target, name, scope);
                }
            }
        }
    }

    fn std_use(&mut self, u: &UseDecl, scope: &mut Scope) {
        let written: Vec<&str> = u.path.iter().map(|s| s.name.as_str()).collect();
        let Some(module) = prelude::std_module(&written.join(".")) else {
            return;
        };
        if !scope.std_modules.iter().any(|m| m.path == module.path) {
            scope.std_modules.push(module);
        }
        match &u.names {
            None => {
                let name = module.name();
                if let Some(last) = u.path.last() {
                    if let Some(earlier) = self.bound(scope, name, None) {
                        self.bound_twice(name, earlier, last.span);
                        return;
                    }
                }
                scope.std_bound.push(module);
            }
            Some(names) => {
                for name in names.iter().filter(|n| module.exports_name(&n.name)) {
                    scope.std_names.insert(name.name.clone());
                }
            }
        }
    }

    /// What already binds `name` in `scope`, if anything other than the
    /// module `target` does: an item, or another import.
    fn bound(&self, scope: &Scope, name: &str, target: Option<usize>) -> Option<&'static str> {
        if let Some(&module) = scope.qualifiers.get(name) {
            return (Some(module) != target).then_some("imported by an earlier `use`");
        }
        if scope.std_bound.iter().any(|m| m.name() == name) {
            return Some("imported by an earlier `use`");
        }
        if scope.names.contains_key(name) {
            return Some("a name in this module");
        }
        None
    }

    fn bound_twice(&mut self, name: &str, earlier: &str, span: Span) {
        self.diagnostics.push(Diagnostic::new(
            codes::NAME_BOUND_TWICE,
            format!(
                "`{name}` is already {earlier}: a module binds each name once, and an \
                 import cannot be renamed; import the module's items by name instead, \
                 as in `use pkg.a.{name}.{{Item}}`, or rename the item"
            ),
            span,
        ));
    }

    fn unknown_module(&mut self, written: &str, span: Span) {
        if written.is_empty() {
            self.diagnostics.push(Diagnostic::new(
                codes::UNKNOWN_PKG_MODULE,
                "`use pkg` must name a module of the package, as in `use pkg.util` \
                 for `src/util.osk`",
                span,
            ));
            return;
        }
        let candidates: Vec<String> = self.paths.iter().map(|p| format!("{PKG}.{p}")).collect();
        let candidates: Vec<&str> = candidates.iter().map(String::as_str).collect();
        let file = format!("src/{}.osk", written.replace('.', "/"));
        self.diagnostics.push(suggest::unknown(
            codes::UNKNOWN_PKG_MODULE,
            format!("unknown module `{PKG}.{written}`: the package has no `{file}`"),
            &format!("{PKG}.{written}"),
            span,
            &candidates,
        ));
    }

    /// `name` in the brace group of a `use` of module `target`, written
    /// in module `index`.
    fn import_name(&mut self, index: usize, target: usize, name: &Ident, scope: &mut Scope) {
        let Some(key) = self.item_of(index, target, name) else {
            return;
        };
        if scope.names.get(&name.name) == Some(&key) {
            return;
        }
        if let Some(earlier) = self.bound(scope, &name.name, None) {
            self.bound_twice(&name.name, earlier, name.span);
            return;
        }
        scope.names.insert(name.name.clone(), key);
    }

    /// The key of the item `name` of module `target`, written in module
    /// `index`: reported when the module has no such item, or when it is
    /// private to it. A private item still has its key, so that nothing
    /// else is reported about it.
    fn item_of(&mut self, index: usize, target: usize, name: &Ident) -> Option<String> {
        let Some(declared) = self.declared[target].get(&name.name) else {
            let mut candidates: Vec<&str> =
                self.declared[target].keys().map(String::as_str).collect();
            candidates.sort_unstable();
            let module = &self.paths[target];
            let diagnostic = suggest::unknown(
                codes::NOT_IN_MODULE,
                format!("`{PKG}.{module}` has no item `{}`", name.name),
                &name.name,
                name.span,
                &candidates,
            )
            .with_related(
                format!("the module `{PKG}.{module}` it looked in"),
                Span::at(self.module_starts[target]),
            );
            self.diagnostics.push(diagnostic);
            return None;
        };
        if declared.visibility == Visibility::Private && target != index {
            let module = &self.paths[target];
            let diagnostic = Diagnostic::new(
                codes::PRIVATE_ITEM,
                format!(
                    "`{}` is private to `{PKG}.{module}`; mark it `pub(pkg)` to use it from \
                     other modules of the package",
                    name.name
                ),
                name.span,
            )
            .with_fix(
                format!("mark `{}` `pub(pkg)`", name.name),
                vec![Edit::insert(declared.pub_at, "pub(pkg) ")],
            );
            self.diagnostics.push(diagnostic);
        }
        Some(declared.key.clone())
    }

    /// Report every import cycle once, naming each module in it.
    fn cycles(&mut self) {
        let edges: Vec<Vec<(usize, Span)>> =
            self.scopes.iter().map(|s| s.imports.clone()).collect();
        let mut reported: HashSet<usize> = HashSet::new();
        for start in 0..edges.len() {
            if reported.contains(&start) {
                continue;
            }
            let Some(cycle) = shortest_cycle(&edges, start) else {
                continue;
            };
            if cycle.iter().any(|(m, _)| reported.contains(m)) {
                continue;
            }
            reported.extend(cycle.iter().map(|(m, _)| *m));
            let mut names: Vec<String> = cycle
                .iter()
                .map(|(m, _)| format!("`{PKG}.{}`", self.paths[*m]))
                .collect();
            names.push(format!("`{PKG}.{}`", self.paths[start]));
            let span = cycle.first().map_or(Span { start: 0, end: 0 }, |(_, s)| *s);
            self.diagnostics.push(Diagnostic::new(
                codes::IMPORT_CYCLE,
                format!(
                    "modules import each other in a cycle: {}; move what they share \
                     into a module that imports none of them",
                    names.join(" imports ")
                ),
                span,
            ));
        }
    }
}

/// The shortest cycle of imports from module `start` back to it: each
/// module on it, from `start`, with the span of its `use` of the next.
fn shortest_cycle(edges: &[Vec<(usize, Span)>], start: usize) -> Option<Vec<(usize, Span)>> {
    // Breadth-first, remembering how each module was first reached.
    let mut came_from: HashMap<usize, (usize, Span)> = HashMap::new();
    let mut queue = std::collections::VecDeque::from([start]);
    while let Some(module) = queue.pop_front() {
        for &(next, span) in &edges[module] {
            if next == start {
                let mut cycle = vec![(module, span)];
                let mut at = module;
                while at != start {
                    let (previous, span) = *came_from.get(&at)?;
                    cycle.push((previous, span));
                    at = previous;
                }
                cycle.reverse();
                return Some(cycle);
            }
            if next != start && !came_from.contains_key(&next) {
                came_from.insert(next, (module, span));
                queue.push_back(next);
            }
        }
    }
    None
}

/// The `std` imports of the whole package: what the joined module sees,
/// which a module sees only when it imports them itself.
struct PackageStd {
    bound: HashSet<&'static str>,
    names: HashSet<String>,
}

impl PackageStd {
    fn of(scopes: &[Scope]) -> PackageStd {
        PackageStd {
            bound: scopes
                .iter()
                .flat_map(|s| s.std_bound.iter().map(|m| m.name()))
                .collect(),
            names: scopes
                .iter()
                .flat_map(|s| s.std_names.iter().cloned())
                .collect(),
        }
    }
}

/// What a reference names, for the code of an unknown one.
#[derive(Clone, Copy)]
enum Expect {
    Type,
    Value,
}

/// Rewrites one module's items for the joined module.
struct Rewriter<'l> {
    linker: &'l mut Linker,
    package_std: &'l PackageStd,
    module: usize,
    /// The locals in scope, innermost last.
    locals: Vec<String>,
    /// The generic parameters in scope, and `Self` inside a trait or impl.
    generics: Vec<String>,
}

impl Rewriter<'_> {
    fn scope(&self) -> &Scope {
        &self.linker.scopes[self.module]
    }

    fn is_entry(&self) -> bool {
        self.linker.entry == Some(self.module)
    }

    fn is_local(&self, name: &str) -> bool {
        self.locals.iter().any(|l| l == name)
    }

    fn report(&mut self, diagnostic: Diagnostic, span: Span) {
        self.linker.diagnostics.push(diagnostic);
        self.linker.reported.push(span);
    }

    fn item(&mut self, item: &mut Item) {
        let module = &self.linker.paths[self.module];
        if let Some(name) = item_name_mut(item) {
            name.name = key(module, &name.name);
        }
        match &mut item.kind {
            ItemKind::Fn(f) => self.function(f),
            ItemKind::Struct(s) => {
                let mark = self.enter(&mut s.generics);
                self.fields(&mut s.fields);
                self.generics.truncate(mark);
            }
            ItemKind::Enum(e) => {
                let mark = self.enter(&mut e.generics);
                for variant in &mut e.variants {
                    self.fields(&mut variant.fields);
                }
                self.generics.truncate(mark);
            }
            ItemKind::Trait(t) => {
                let mark = self.enter(&mut t.generics);
                self.generics.push(ty::SELF.to_string());
                self.methods(&mut t.methods);
                self.generics.truncate(mark);
            }
            ItemKind::Impl(imp) => {
                let mark = self.enter(&mut imp.generics);
                if let Some(trait_ref) = &mut imp.trait_ref {
                    self.ty(trait_ref);
                }
                self.ty(&mut imp.target);
                self.generics.push(ty::SELF.to_string());
                self.methods(&mut imp.methods);
                self.generics.truncate(mark);
            }
            ItemKind::Const(c) => {
                self.ty(&mut c.ty);
                self.expr(&mut c.value);
            }
            ItemKind::Test(t) => {
                // A test keeps its name; its module is where it is
                // (`oska_check::Locator::module_of`).
                self.locals.push("t".to_string());
                self.block(&mut t.body);
                self.locals.clear();
            }
            ItemKind::Use(_) => {}
        }
    }

    /// Bring `generics` into scope, rewriting their bounds; returns the
    /// mark to truncate back to.
    fn enter(&mut self, generics: &mut [GenericParam]) -> usize {
        let mark = self.generics.len();
        self.generics
            .extend(generics.iter().map(|g| g.name.name.clone()));
        for g in generics {
            for bound in &mut g.bounds {
                self.ty(bound);
            }
        }
        mark
    }

    fn fields(&mut self, fields: &mut oska_syntax::ast::Fields) {
        if let oska_syntax::ast::Fields::Named(fields)
        | oska_syntax::ast::Fields::Positional(fields) = fields
        {
            for field in fields {
                self.ty(&mut field.ty);
            }
        }
    }

    fn methods(&mut self, methods: &mut [Item]) {
        for method in methods {
            if let ItemKind::Fn(f) = &mut method.kind {
                self.function(f);
            }
        }
    }

    fn function(&mut self, f: &mut FnDecl) {
        let mark = self.enter(&mut f.generics);
        for param in &mut f.params {
            self.ty(&mut param.ty);
        }
        if let Some(ret) = &mut f.return_type {
            self.ty(ret);
        }
        self.locals = f.params.iter().map(|p| p.name.name.clone()).collect();
        for clause in &mut f.requires {
            self.expr(&mut clause.cond);
        }
        if let Some(body) = &mut f.body {
            self.block(body);
        }
        for clause in &mut f.ensures {
            self.locals.push(clause.binding.name.clone());
            self.expr(&mut clause.cond);
            self.locals.pop();
        }
        self.locals.clear();
        self.generics.truncate(mark);
    }

    fn ty(&mut self, ty: &mut Type) {
        match &mut ty.kind {
            TypeKind::Unit => {}
            TypeKind::Fn {
                params,
                return_type,
                ..
            } => {
                for param in params {
                    self.ty(param);
                }
                if let Some(ret) = return_type {
                    self.ty(ret);
                }
            }
            TypeKind::Path { segments, args } => {
                for arg in args {
                    self.ty(arg);
                }
                match segments.as_mut_slice() {
                    [ident] => {
                        if !self.generics.contains(&ident.name) {
                            if let Some(key) = self.plain(&ident.name, ident.span, Expect::Type) {
                                ident.name = key;
                            }
                        }
                    }
                    [qualifier, name] => {
                        let span = join(qualifier.span, name.span);
                        match self.pkg_qualifier(&qualifier.name) {
                            Some(target) => {
                                if let Some(key) = self.qualified(target, name, span) {
                                    *segments = vec![Ident { name: key, span }];
                                }
                            }
                            None => self.std_qualifier_leak(qualifier, span),
                        }
                    }
                    [first, .., _] if first.name == PKG => {
                        let segments = segments.clone();
                        self.pkg_path_type(&segments);
                    }
                    _ => {}
                }
            }
        }
    }

    /// A type written as a `pkg.…` path, which is written only in a
    /// `use`. Reported as written: the checker would name it by a key.
    fn pkg_path_type(&mut self, segments: &[Ident]) {
        let written: Vec<&str> = segments.iter().map(|s| s.name.as_str()).collect();
        let (Some(first), Some(last)) = (segments.first(), segments.last()) else {
            return;
        };
        let span = join(first.span, last.span);
        let (module, item) = match written.as_slice() {
            [_, module @ .., item] => (module.join("."), *item),
            _ => return,
        };
        let qualifier = module.rsplit('.').next().unwrap_or(&module);
        let diagnostic = Diagnostic::new(
            codes::UNKNOWN_NAME,
            format!(
                "unknown type `{}`: a `{PKG}.…` path is written only in a `use`; \
                 after `use {PKG}.{module}`, write `{qualifier}.{item}`",
                written.join(".")
            ),
            span,
        );
        self.report(diagnostic, span);
    }

    /// A name written plainly in a type or value position. Returns its
    /// key when it names an item of the package.
    fn plain(&mut self, name: &str, span: Span, expect: Expect) -> Option<String> {
        if let Some(key) = self.scope().names.get(name) {
            return Some(key.clone());
        }
        if self.scope().std_names.contains(name) {
            return None;
        }
        let leaked_std = self.package_std.names.contains(name);
        if leaked_std {
            let qualify = self
                .scope()
                .std_bound
                .iter()
                .find(|m| m.declared().contains(&name))
                .copied();
            if let Some(module) = qualify {
                self.report(suggest::qualify(module, span, name), span);
                return None;
            }
        }
        // The entry module's `main` keeps its name, so another module
        // writing `main` would find it.
        let leaked_entry = !self.is_entry()
            && self.linker.entry.is_some_and(|entry| {
                self.linker.declared[entry]
                    .get(name)
                    .is_some_and(|declared| declared.key == name)
            });
        if leaked_std || leaked_entry {
            let diagnostic = match expect {
                Expect::Type => {
                    Diagnostic::new(codes::UNKNOWN_NAME, format!("unknown type `{name}`"), span)
                }
                Expect::Value => {
                    Diagnostic::new(codes::UNKNOWN_VALUE, format!("unknown name `{name}`"), span)
                }
            };
            self.report(diagnostic, span);
        }
        None
    }

    /// The module `qualifier` names, when it is a module this module
    /// imports with `use pkg.…`.
    fn pkg_qualifier(&self, qualifier: &str) -> Option<usize> {
        if self.is_local(qualifier) || self.scope().names.contains_key(qualifier) {
            return None;
        }
        self.scope().qualifiers.get(qualifier).copied()
    }

    /// `name` in module `target`, written `qualifier.name` across `span`:
    /// its key, unless the module has no such item.
    fn qualified(&mut self, target: usize, name: &Ident, span: Span) -> Option<String> {
        let key = self.linker.item_of(self.module, target, name);
        if key.is_none() {
            self.linker.reported.push(span);
        }
        key
    }

    /// `qualifier.…` across `span`, where `qualifier` is a `std` module
    /// that another module binds with a plain `use` and this one does
    /// not: the joined module would see it, so report it here.
    fn std_qualifier_leak(&mut self, qualifier: &Ident, span: Span) {
        let name = qualifier.name.as_str();
        let bound_here = self.scope().std_bound.iter().any(|m| m.name() == name);
        if bound_here
            || !self.package_std.bound.contains(name)
            || self.is_local(name)
            || self.scope().names.contains_key(name)
        {
            return;
        }
        let diagnostic = Diagnostic::new(
            codes::UNKNOWN_VALUE,
            format!("unknown name `{name}`"),
            qualifier.span,
        );
        self.report(diagnostic, span);
    }

    fn block(&mut self, block: &mut Block) {
        let mark = self.locals.len();
        for stmt in &mut block.stmts {
            match &mut stmt.kind {
                StmtKind::Let {
                    name, ty, value, ..
                } => {
                    if let Some(ty) = ty {
                        self.ty(ty);
                    }
                    self.expr(value);
                    self.locals.push(name.name.clone());
                }
                StmtKind::Assign { target, value } => {
                    self.expr(target);
                    self.expr(value);
                }
                StmtKind::Return(value) => {
                    if let Some(value) = value {
                        self.expr(value);
                    }
                }
                StmtKind::Break | StmtKind::Continue => {}
                StmtKind::For {
                    binding,
                    iter,
                    body,
                } => {
                    self.expr(iter);
                    self.locals.push(binding.name.clone());
                    self.block(body);
                    self.locals.pop();
                }
                StmtKind::While { cond, body } => {
                    self.expr(cond);
                    self.block(body);
                }
                StmtKind::Expr(expr) => self.expr(expr),
            }
        }
        self.locals.truncate(mark);
    }

    fn expr(&mut self, expr: &mut Expr) {
        let span = expr.span;
        match &mut expr.kind {
            ExprKind::Int(_) | ExprKind::Bool(_) | ExprKind::Unit | ExprKind::SelfValue => {}
            ExprKind::Name(ident) => {
                if !self.is_local(&ident.name) {
                    if let Some(key) = self.plain(&ident.name, ident.span, Expect::Value) {
                        ident.name = key;
                    }
                }
            }
            ExprKind::Field { base, name } => {
                if let ExprKind::Name(qualifier) = &base.kind {
                    if let Some(target) = self.pkg_qualifier(&qualifier.name) {
                        if let Some(key) = self.qualified(target, name, span) {
                            expr.kind = ExprKind::Name(Ident { name: key, span });
                        }
                        return;
                    }
                    let qualifier = qualifier.clone();
                    self.std_qualifier_leak(&qualifier, span);
                }
                self.expr(base);
            }
            ExprKind::Str(parts) => {
                for part in parts {
                    if let StrPart::Hole { path, .. } = part {
                        self.hole(path);
                    }
                }
            }
            ExprKind::SinkStr { tag, .. } => {
                let tag = tag.clone();
                self.sink_literal(&tag, span);
            }
            ExprKind::Call { callee, args } => {
                self.expr(callee);
                for arg in args {
                    self.expr(&mut arg.value);
                }
            }
            ExprKind::Unary { operand, .. } => self.expr(operand),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Try(inner) => self.expr(inner),
            ExprKind::Range { start, end } => {
                self.expr(start);
                self.expr(end);
            }
            ExprKind::List(items) => {
                for item in items {
                    self.expr(item);
                }
            }
            ExprKind::Closure { params, body } => {
                let mark = self.locals.len();
                for param in params.iter_mut() {
                    if let Some(ty) = &mut param.ty {
                        self.ty(ty);
                    }
                }
                self.locals
                    .extend(params.iter().map(|p| p.name.name.clone()));
                self.expr(body);
                self.locals.truncate(mark);
            }
            ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => {
                self.expr(cond);
                self.block(then_block);
                if let Some(else_branch) = else_branch {
                    self.expr(else_branch);
                }
            }
            ExprKind::Block(block) => self.block(block),
            ExprKind::Scope { handle, body } => {
                self.locals.push(handle.name.clone());
                self.block(body);
                self.locals.pop();
            }
            ExprKind::Match { scrutinee, arms } => {
                self.expr(scrutinee);
                for arm in arms {
                    let mark = self.locals.len();
                    self.pattern(&mut arm.pattern);
                    if let Some(guard) = &mut arm.guard {
                        self.expr(guard);
                    }
                    self.expr(&mut arm.body);
                    self.locals.truncate(mark);
                }
            }
        }
    }

    /// An interpolation hole's path, such as `{name}` or `{text.GREETING}`.
    fn hole(&mut self, path: &mut Vec<Ident>) {
        let Some(first) = path.first() else {
            return;
        };
        if self.is_local(&first.name) {
            return;
        }
        self.path(path);
    }

    /// A pattern: its constructor paths are rewritten and its bindings
    /// become locals.
    fn pattern(&mut self, pattern: &mut Pattern) {
        match &mut pattern.kind {
            PatternKind::Wildcard | PatternKind::Literal(_) => {}
            PatternKind::Binding(ident) => {
                // An uppercase name is a fieldless variant (ADR 0005).
                if !ident.name.starts_with(|c: char| c.is_ascii_uppercase()) {
                    self.locals.push(ident.name.clone());
                }
            }
            PatternKind::Constructor { path, args } => {
                self.path(path);
                for arg in args {
                    self.pattern(&mut arg.pattern);
                }
            }
        }
    }

    /// A path of a hole or a constructor pattern, whose first segment is
    /// not a local: `qualifier.Item.…` or `Item.…`.
    fn path(&mut self, path: &mut Vec<Ident>) {
        if let [qualifier, name, ..] = path.as_slice() {
            let span = join(qualifier.span, name.span);
            if let Some(target) = self.pkg_qualifier(&qualifier.name) {
                let name = name.clone();
                if let Some(key) = self.qualified(target, &name, span) {
                    path.splice(0..2, [Ident { name: key, span }]);
                }
                return;
            }
            let qualifier = qualifier.clone();
            self.std_qualifier_leak(&qualifier, span);
        }
        if let Some(first) = path.first_mut() {
            if let Some(key) = self.plain(&first.name, first.span, Expect::Value) {
                first.name = key;
            }
        }
    }

    /// A sink literal needs its `std` module imported by its own module,
    /// whatever the rest of the package imports. The fix adds the `use`
    /// at the top of the module.
    fn sink_literal(&mut self, tag: &str, span: Span) {
        let Some(module) = prelude::sink_module(tag) else {
            return;
        };
        if self
            .scope()
            .std_modules
            .iter()
            .any(|m| m.path == module.path)
        {
            return;
        }
        let tag_span = Span {
            start: span.start,
            end: span.start + tag.len(),
        };
        let path = module.path;
        let module_start = self.linker.module_starts[self.module];
        let diagnostic = Diagnostic::new(
            codes::SINK_NOT_IMPORTED,
            format!("a `{tag}` literal needs `use {path}`"),
            tag_span,
        )
        .with_fix(
            format!("add `use {path}`"),
            vec![Edit::insert(module_start, format!("use {path}\n"))],
        );
        self.report(diagnostic, tag_span);
    }
}
