//! Per-tool authority: what each tool of an implementation of `std.mcp`'s
//! `Toolset` reaches.
//!
//! When `call`'s body is a `match` on its name parameter whose arms are
//! string literals, each arm is one tool. A tool reaches the
//! capability-bearing fields of `self` that its arm uses, and those used
//! by the methods of the same type that it calls on `self`, transitively.
//! A free function reaches only what it is given, so a field passed to
//! one is a use of that field. Any other use of `self`, such as passing it
//! whole, reaches all of it. Statements before the `match` run for every
//! tool. A `call` of any other shape is one entry for the whole toolset,
//! not split per tool, so the report is never more precise than the code.
//!
//! When `specs` is a list of `ToolSpec.new` calls with literal names, a
//! spec with no arm is listed as a tool that runs the default arm, and an
//! arm with no spec is noted. These are report notes, not diagnostics.

use oska_syntax::ast::{
    Block, Expr, ExprKind, FnDecl, ImplDecl, ItemKind, Module, PatternKind, Stmt, StmtKind, StrPart,
};

use super::{reach, Reach};
use crate::collect::Decls;
use crate::items::{Items, Lower};
use crate::ty::Ty;
use crate::{ToolAuthority, ToolsetAuthority, Via};

/// The `std` module that declares `Toolset`, and the names the checker
/// looks for in it.
const MODULE: &str = "std.mcp";
const TOOLSET: &str = "Toolset";
const TOOL_SPEC: &str = "ToolSpec";

/// Whether `trait_name`, an impl's trait as [`crate::items::ImplInfo`]
/// names it, is `std.mcp`'s `Toolset`.
pub(super) fn is_toolset(trait_name: &str) -> bool {
    trait_name == crate::link::key(MODULE, TOOLSET)
}

/// The per-tool authority of `call`, the method of `imp`, an
/// implementation of `Toolset` whose target is `target`. `lower` has the
/// impl's generics in scope.
pub(super) fn toolset(
    module: &Module,
    decls: &Decls,
    items: &Items,
    imp: &ImplDecl,
    call: &FnDecl,
    target: &Ty,
    lower: &Lower,
) -> ToolsetAuthority {
    let implementation = match target {
        Ty::Named(name, _, _) => name.clone(),
        _ => String::new(),
    };
    let mut uses = Uses {
        module,
        decls,
        items,
        target,
        bounded: lower.bounds.iter().map(|b| b.param.clone()).collect(),
        visiting: Vec::new(),
    };
    let Some(Split {
        before,
        arms,
        default,
    }) = call.body.as_ref().and_then(|b| split(b, call))
    else {
        let mut whole = Vec::new();
        uses.whole(&mut whole);
        return ToolsetAuthority {
            implementation,
            split: false,
            tools: vec![tool(None, true, whole)],
            arms_without_spec: Vec::new(),
            default: None,
            specs_checked: false,
        };
    };
    let mut shared = Vec::new();
    for stmt in before {
        uses.stmt(&stmt.kind, &mut shared);
    }
    let reached = |uses: &mut Uses, body: &Expr| {
        let mut found = shared.clone();
        uses.visiting.clear();
        uses.expr(body, &mut found);
        found
    };
    let mut tools: Vec<ToolAuthority> = Vec::new();
    for (name, body) in &arms {
        let found = reached(&mut uses, body);
        tools.push(tool(Some(name.clone()), true, found));
    }
    let specs = imp
        .methods
        .iter()
        .find_map(|m| match &m.kind {
            ItemKind::Fn(f) if f.name.name == "specs" => f.body.as_ref(),
            _ => None,
        })
        .and_then(spec_names);
    let mut arms_without_spec = Vec::new();
    let specs_checked = specs.is_some();
    if let Some(specs) = specs {
        for (name, _) in &arms {
            if !specs.contains(name) {
                arms_without_spec.push(name.clone());
            }
        }
        for name in specs {
            if !arms.iter().any(|(arm, _)| *arm == name) {
                let found = reached(&mut uses, default);
                tools.push(tool(Some(name), false, found));
            }
        }
    }
    ToolsetAuthority {
        implementation,
        split: true,
        tools,
        arms_without_spec,
        default: Some(tool(None, false, reached(&mut uses, default))),
        specs_checked,
    }
}

/// A tool's entry from the sources it reaches, each named as `self` or
/// `self.field`, in order of first use.
fn tool(name: Option<String>, arm: bool, sources: Vec<(String, Reach)>) -> ToolAuthority {
    let mut total = Reach::default();
    let mut via = Vec::new();
    for (param, r) in sources {
        if r.bearing {
            via.push(Via {
                param,
                capabilities: r.capabilities.clone(),
                generics: r.generics.clone(),
            });
        }
        total.add(r);
    }
    ToolAuthority {
        name,
        arm,
        authority: total.capabilities,
        via,
        pure: !total.bearing,
    }
}

/// The body of a `call` split per tool.
struct Split<'b> {
    /// The statements before the `match`, which run for every tool.
    before: &'b [Stmt],
    /// The literal arms, by name, first arm first.
    arms: Vec<(String, &'b Expr)>,
    /// The default arm's body.
    default: &'b Expr,
}

/// `body`, the body of `call`, split per tool: when its last statement
/// is a `match` on the name parameter whose arms are string literals,
/// unguarded, and one final `_` or binding.
fn split<'b>(body: &'b Block, call: &FnDecl) -> Option<Split<'b>> {
    let name = &call.params.first()?.name.name;
    let (last, before) = body.stmts.split_last()?;
    let StmtKind::Expr(Expr {
        kind: ExprKind::Match { scrutinee, arms },
        ..
    }) = &last.kind
    else {
        return None;
    };
    if !matches!(&scrutinee.kind, ExprKind::Name(n) if n.name == *name) {
        return None;
    }
    // A `let` before the `match` that rebinds the name makes the match
    // on another value. A parameter cannot be assigned, and a `let` in a
    // nested block ends with it.
    if before
        .iter()
        .any(|stmt| matches!(&stmt.kind, StmtKind::Let { name: bound, .. } if bound.name == *name))
    {
        return None;
    }
    let (default, literal) = arms.split_last()?;
    if default.guard.is_some()
        || !matches!(
            default.pattern.kind,
            PatternKind::Wildcard | PatternKind::Binding(_)
        )
    {
        return None;
    }
    let mut named: Vec<(String, &Expr)> = Vec::new();
    for arm in literal {
        if arm.guard.is_some() {
            return None;
        }
        let PatternKind::Literal(lit) = &arm.pattern.kind else {
            return None;
        };
        let text = literal_text(lit)?;
        // A repeated literal's later arm never runs.
        if !named.iter().any(|(n, _)| *n == text) {
            named.push((text, &arm.body));
        }
    }
    Some(Split {
        before,
        arms: named,
        default: &default.body,
    })
}

/// The text of `expr`, a string literal with no holes.
fn literal_text(expr: &Expr) -> Option<String> {
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

/// The names `specs` gives its tools, when its value is a list literal of
/// `ToolSpec.new` calls, each perhaps followed by `.with_title(...)`,
/// whose names are string literals.
fn spec_names(body: &Block) -> Option<Vec<String>> {
    let StmtKind::Expr(Expr {
        kind: ExprKind::List(specs),
        ..
    }) = &body.stmts.last()?.kind
    else {
        return None;
    };
    specs.iter().map(spec_name).collect()
}

fn spec_name(expr: &Expr) -> Option<String> {
    let ExprKind::Call { callee, args } = &expr.kind else {
        return None;
    };
    let ExprKind::Field { base, name } = &callee.kind else {
        return None;
    };
    match name.name.as_str() {
        "with_title" => spec_name(base),
        "new" if names_tool_spec(base) => literal_text(&args.first()?.value),
        _ => None,
    }
}

/// Whether `expr` names `std.mcp`'s `ToolSpec`: linking writes it as its
/// key, however the program imports it.
fn names_tool_spec(expr: &Expr) -> bool {
    matches!(&expr.kind, ExprKind::Name(n) if n.name == crate::link::key(MODULE, TOOL_SPEC))
}

/// What code on `self`, a value of `target`, reaches.
struct Uses<'a, 'd> {
    module: &'a Module,
    decls: &'d Decls<'d>,
    items: &'a Items,
    target: &'a Ty,
    bounded: Vec<String>,
    /// The methods of `target` being walked, so recursion ends.
    visiting: Vec<String>,
}

impl Uses<'_, '_> {
    /// Add `reach`, from `source`, to `out`.
    fn add(out: &mut Vec<(String, Reach)>, source: String, r: Reach) {
        match out.iter_mut().find(|(s, _)| *s == source) {
            Some((_, existing)) => existing.add(r),
            None => out.push((source, r)),
        }
    }

    /// All of `self`.
    fn whole(&self, out: &mut Vec<(String, Reach)>) {
        let r = reach(self.target, self.decls, &self.bounded, &mut Vec::new());
        Self::add(out, "self".to_string(), r);
    }

    /// The type of `self`'s field `name`, if `target` is a struct with
    /// one.
    fn field(&self, name: &str) -> Option<Ty> {
        let Ty::Named(ty, args, _) = self.target else {
            return None;
        };
        let info = self.items.structs.get(ty)?;
        let field = info.fields.field(name)?;
        let subst: Vec<(String, Ty)> = info
            .generics
            .iter()
            .cloned()
            .zip(args.iter().cloned())
            .collect();
        Some(field.substitute(&subst))
    }

    /// A use of `self.name`.
    fn use_field(&mut self, name: &str, out: &mut Vec<(String, Reach)>) {
        match self.field(name) {
            Some(ty) => {
                let r = reach(&ty, self.decls, &self.bounded, &mut Vec::new());
                Self::add(out, format!("self.{name}"), r);
            }
            None => self.whole(out),
        }
    }

    /// A call of `self.name(...)`: what each method of that name on
    /// `target`, in this module, reaches. A method with no body here,
    /// such as a trait's default, reaches all of `self`.
    fn method(&mut self, name: &str, out: &mut Vec<(String, Reach)>) {
        if self.visiting.iter().any(|m| m == name) {
            return;
        }
        self.visiting.push(name.to_string());
        let Ty::Named(target, _, _) = self.target else {
            self.whole(out);
            return;
        };
        let target = target.clone();
        let mut found = false;
        let module = self.module;
        for item in &module.items {
            let ItemKind::Impl(imp) = &item.kind else {
                continue;
            };
            let ty = Lower::new(self.decls)
                .with_generics(&imp.generics)
                .ty(&imp.target);
            if !matches!(&ty, Ty::Named(n, _, _) if *n == target) {
                continue;
            }
            for m in &imp.methods {
                if let ItemKind::Fn(f) = &m.kind {
                    if f.name.name == name && f.receiver.is_some() {
                        if let Some(body) = &f.body {
                            found = true;
                            self.block(body, out);
                        }
                    }
                }
            }
        }
        if !found {
            self.whole(out);
        }
    }

    fn block(&mut self, block: &Block, out: &mut Vec<(String, Reach)>) {
        for stmt in &block.stmts {
            self.stmt(&stmt.kind, out);
        }
    }

    fn stmt(&mut self, stmt: &StmtKind, out: &mut Vec<(String, Reach)>) {
        match stmt {
            StmtKind::Let { value, .. } => self.expr(value, out),
            StmtKind::Assign { target, value } => {
                self.expr(target, out);
                self.expr(value, out);
            }
            StmtKind::Return(value) => {
                if let Some(value) = value {
                    self.expr(value, out);
                }
            }
            StmtKind::Break | StmtKind::Continue => {}
            StmtKind::For { iter, body, .. } => {
                self.expr(iter, out);
                self.block(body, out);
            }
            StmtKind::While { cond, body } => {
                self.expr(cond, out);
                self.block(body, out);
            }
            StmtKind::Expr(e) => self.expr(e, out),
        }
    }

    fn expr(&mut self, expr: &Expr, out: &mut Vec<(String, Reach)>) {
        match &expr.kind {
            ExprKind::Int(_)
            | ExprKind::SinkStr { .. }
            | ExprKind::Bool(_)
            | ExprKind::Unit
            | ExprKind::Name(_) => {}
            ExprKind::Str(parts) => {
                for part in parts {
                    if let StrPart::Hole { path, .. } = part {
                        if path.first().is_some_and(|p| p.name == "self") {
                            match path.get(1) {
                                Some(field) => self.use_field(&field.name, out),
                                None => self.whole(out),
                            }
                        }
                    }
                }
            }
            ExprKind::SelfValue => self.whole(out),
            ExprKind::Field { base, name } => match &base.kind {
                ExprKind::SelfValue => self.use_field(&name.name, out),
                _ => self.expr(base, out),
            },
            ExprKind::Call { callee, args } => {
                match &callee.kind {
                    ExprKind::Field { base, name }
                        if matches!(base.kind, ExprKind::SelfValue)
                            && self.field(&name.name).is_none() =>
                    {
                        self.method(&name.name, out);
                    }
                    _ => self.expr(callee, out),
                }
                for arg in args {
                    self.expr(&arg.value, out);
                }
            }
            ExprKind::Unary { operand, .. } => self.expr(operand, out),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(lhs, out);
                self.expr(rhs, out);
            }
            ExprKind::Try(inner) => self.expr(inner, out),
            ExprKind::Range { start, end } => {
                self.expr(start, out);
                self.expr(end, out);
            }
            ExprKind::List(elements) => {
                for e in elements {
                    self.expr(e, out);
                }
            }
            ExprKind::Closure { body, .. } => self.expr(body, out),
            ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => {
                self.expr(cond, out);
                self.block(then_block, out);
                if let Some(e) = else_branch {
                    self.expr(e, out);
                }
            }
            ExprKind::Block(b) => self.block(b, out),
            ExprKind::Scope { body, .. } => self.block(body, out),
            ExprKind::Match { scrutinee, arms } => {
                self.expr(scrutinee, out);
                for arm in arms {
                    if let Some(guard) = &arm.guard {
                        self.expr(guard, out);
                    }
                    self.expr(&arm.body, out);
                }
            }
        }
    }
}
