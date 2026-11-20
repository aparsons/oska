//! The fix of `E0102` (spec #367, Q12): an interpolation hole that holds
//! an expression, such as `"next is {n + 1}"`, is repaired by hoisting the
//! expression into `let <fresh name> = <expression>` on its own line just
//! before the statement, and putting the name in the hole.
//!
//! The parser reports `E0102` and stops, so the fix is made here, where
//! the whole package is at hand. To see the statement, the module is
//! parsed again with every hole that holds an expression replaced by a
//! name of the same length, so every span stays where it was.
//!
//! The fix is `machine` only when moving the evaluation earlier cannot
//! change what the program does: the expression calls only pure
//! functions, which a probe asks the checker by wrapping it in a closure
//! passed where a plain `fn` is expected (a closure that captures a
//! capability is a `cap fn`, `E0270`); the probe program must check, apart
//! from the other holes; and the expression has no `?`, `return`, `break`
//! or `continue`. Otherwise it is `maybe`, and so is a hole whose
//! expression runs only sometimes or more than once where it is: in a
//! `while` condition, on the right side of `&&`, `||` or `implies`, or in
//! an `else if` condition. A hole in a closure or `match` arm whose body is
//! not a block, or in a guard, has no fix: its names may be bound there.
//!
//! The name is `value`, or `value2` and so on when the module's text
//! already has that word anywhere, so it cannot clash with or shadow a name
//! the code uses.

use oska_syntax::ast::{Block, Expr, ExprKind, ItemKind, Module, Stmt, StmtKind};
use oska_syntax::{Applicability, Diagnostic, Edit, Fix, Span};

use crate::package::{check_package_unfixed, Package};
use crate::CheckOptions;

/// The diagnostic code this module gives a fix.
const HOLE: &str = "E0102";

/// Give each `E0102` among `diagnostics` its fix, when it has one.
pub(crate) fn add_fixes(package: &Package, diagnostics: &mut [Diagnostic]) {
    for d in diagnostics.iter_mut() {
        if d.code != HOLE || d.fix.is_some() {
            continue;
        }
        let index = match &d.location {
            Some(location) => package.modules.iter().position(|m| m.path == location.path),
            None if package.modules.len() == 1 => Some(0),
            None => None,
        };
        if let Some(fix) = index.and_then(|i| fix(package, i, d.span)) {
            d.fix = Some(Box::new(fix));
        }
    }
}

/// The fix of the hole at `hole` in the module at `index`.
fn fix(package: &Package, index: usize, hole: Span) -> Option<Fix> {
    let src = &package.modules[index].source;
    let inner = src
        .get(hole.start..hole.end)?
        .strip_prefix('{')?
        .strip_suffix('}')?;
    let expr_text = inner.trim();
    // A backslash is an escape of the string, not of the expression.
    if expr_text.is_empty() || expr_text.contains(['\\', '\n', '\r']) {
        return None;
    }
    let expr = oska_syntax::parse_expr(expr_text).ok()?;
    let (others, module) = parse_with_placeholders(src, hole)?;
    let site = statement_of(&module, hole)?;
    let name = fresh(&[src.as_str()], "value");
    let indent = indent_before(src, site.start);
    let edits = vec![
        Edit {
            span: Span::at(site.start),
            replacement: format!("let {name} = {expr_text}\n{indent}"),
        },
        Edit {
            span: hole,
            replacement: format!("{{{name}}}"),
        },
    ];
    // The fixed module must parse, the other holes aside.
    let mut checked = edits.clone();
    checked.extend(others.iter().map(|h| placeholder(*h)));
    oska_syntax::parse_module(&apply(src, &checked).0).ok()?;
    let certain = !site.later
        && !leaves(&expr)
        && pure(package, index, site.start, hole, &others, &name, expr_text);
    Some(Fix {
        message: format!("compute `{expr_text}` first, with `let {name}`"),
        edits,
        path: None,
        applicability: if certain {
            Applicability::Machine
        } else {
            Applicability::Maybe
        },
    })
}

/// Parse `src` with the hole at `hole`, and every other hole that holds
/// an expression, replaced by a name of the same length: the module, and
/// the other holes' spans.
fn parse_with_placeholders(src: &str, hole: Span) -> Option<(Vec<Span>, Module)> {
    let mut holes = vec![hole];
    // Each round replaces one more hole; a module has only so many.
    for _ in 0..256 {
        let edits: Vec<Edit> = holes.iter().map(|h| placeholder(*h)).collect();
        match oska_syntax::parse_module(&apply(src, &edits).0) {
            Ok(module) => return Some((holes.split_off(1), module)),
            Err(err) if err.code == HOLE && is_expression_hole(src, err.span) => {
                if holes.contains(&err.span) {
                    return None;
                }
                holes.push(err.span);
            }
            Err(_) => return None,
        }
    }
    None
}

/// Whether `span` of `src` is a hole with something in it: `{...}`.
fn is_expression_hole(src: &str, span: Span) -> bool {
    src.get(span.start..span.end)
        .is_some_and(|t| t.len() > 2 && t.starts_with('{') && t.ends_with('}'))
}

/// The hole at `hole` holding a name of the same length in bytes.
fn placeholder(hole: Span) -> Edit {
    Edit {
        span: hole,
        replacement: format!("{{{}}}", "x".repeat(hole.end - hole.start - 2)),
    }
}

/// `src` with `edits` applied, which do not overlap, and the span each
/// edit's replacement has in the result, in the order of `edits`.
fn apply(src: &str, edits: &[Edit]) -> (String, Vec<Span>) {
    let mut order: Vec<usize> = (0..edits.len()).collect();
    order.sort_by_key(|&i| (edits[i].span.start, edits[i].span.end));
    let mut out = String::with_capacity(src.len());
    let mut spans = vec![Span::at(0); edits.len()];
    let mut at = 0;
    for i in order {
        let edit = &edits[i];
        out.push_str(&src[at..edit.span.start]);
        let start = out.len();
        out.push_str(&edit.replacement);
        spans[i] = Span {
            start,
            end: out.len(),
        };
        at = edit.span.end;
    }
    out.push_str(&src[at..]);
    (out, spans)
}

/// The statement a hoisted `let` goes before.
struct Site {
    /// Where the statement starts.
    start: usize,
    /// Hoisting changes when the expression runs: it runs only sometimes,
    /// or more than once, where it is.
    later: bool,
}

/// What a walk found of the hole.
enum Walk {
    /// It is not in this code.
    Missing,
    /// It is in this code, in no block of it: the statement around it is
    /// where to hoist. `later` as in [`Site`].
    Here { later: bool },
    /// The innermost statement around it, or `None` when it has no fix.
    Found(Option<Site>),
}

fn statement_of(module: &Module, hole: Span) -> Option<Site> {
    for item in &module.items {
        if !contains(item.span, hole) {
            continue;
        }
        let walked = match &item.kind {
            ItemKind::Fn(f) => f.body.as_ref().map_or(Walk::Missing, |b| block(b, hole)),
            ItemKind::Test(t) => block(&t.body, hole),
            ItemKind::Impl(imp) => methods(&imp.methods, hole),
            ItemKind::Trait(t) => methods(&t.methods, hole),
            _ => Walk::Missing,
        };
        return match walked {
            Walk::Found(site) => site,
            Walk::Missing | Walk::Here { .. } => None,
        };
    }
    None
}

fn methods(items: &[oska_syntax::ast::Item], hole: Span) -> Walk {
    for item in items {
        if let ItemKind::Fn(f) = &item.kind {
            if let Some(body) = f.body.as_ref().filter(|b| contains(b.span, hole)) {
                return block(body, hole);
            }
        }
    }
    Walk::Missing
}

fn contains(outer: Span, inner: Span) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

fn block(b: &Block, hole: Span) -> Walk {
    for s in &b.stmts {
        if !contains(s.span, hole) {
            continue;
        }
        return match stmt(s, hole) {
            Walk::Here { later } => Walk::Found(Some(Site {
                start: s.span.start,
                later,
            })),
            found => found,
        };
    }
    Walk::Missing
}

fn stmt(s: &Stmt, hole: Span) -> Walk {
    match &s.kind {
        StmtKind::Let { value, .. } => expr(value, hole, false),
        StmtKind::Assign { target, value } => first(&[(target, false), (value, false)], hole),
        StmtKind::Return(Some(value)) => expr(value, hole, false),
        StmtKind::Return(None) | StmtKind::Break | StmtKind::Continue => Walk::Missing,
        StmtKind::For { iter, body, .. } => match expr(iter, hole, false) {
            Walk::Missing => block(body, hole),
            walked => walked,
        },
        StmtKind::While { cond, body } => match expr(cond, hole, true) {
            Walk::Missing => block(body, hole),
            walked => walked,
        },
        StmtKind::Expr(e) => expr(e, hole, false),
    }
}

/// The first of `exprs`, each with its `later`, that the hole is in.
fn first(exprs: &[(&Expr, bool)], hole: Span) -> Walk {
    for (e, later) in exprs {
        match expr(e, hole, *later) {
            Walk::Missing => {}
            walked => return walked,
        }
    }
    Walk::Missing
}

fn expr(e: &Expr, hole: Span, later: bool) -> Walk {
    if !contains(e.span, hole) {
        return Walk::Missing;
    }
    match &e.kind {
        ExprKind::Str(_) => Walk::Here { later },
        ExprKind::Field { base, .. } => expr(base, hole, later),
        ExprKind::Call { callee, args } => {
            let mut all = vec![(callee.as_ref(), later)];
            all.extend(args.iter().map(|a| (&a.value, later)));
            first(&all, hole)
        }
        ExprKind::Unary { operand, .. } | ExprKind::Try(operand) => expr(operand, hole, later),
        ExprKind::Binary { op, lhs, rhs } => {
            use oska_syntax::ast::BinaryOp::{And, Implies, Or};
            let short = matches!(op, And | Or | Implies);
            first(&[(lhs, later), (rhs, later || short)], hole)
        }
        ExprKind::Range { start, end } => first(&[(start, later), (end, later)], hole),
        ExprKind::List(items) => {
            let all: Vec<(&Expr, bool)> = items.iter().map(|i| (i, later)).collect();
            first(&all, hole)
        }
        ExprKind::Closure { body, .. } => match &body.kind {
            ExprKind::Block(b) => block(b, hole),
            _ => Walk::Found(None),
        },
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => match expr(cond, hole, later) {
            Walk::Missing if contains(then_block.span, hole) => block(then_block, hole),
            // An `else if` condition runs only when `cond` is false.
            Walk::Missing => else_branch
                .as_ref()
                .map_or(Walk::Missing, |e| expr(e, hole, true)),
            walked => walked,
        },
        ExprKind::Block(b) | ExprKind::Scope { body: b, .. } => block(b, hole),
        ExprKind::Match { scrutinee, arms } => match expr(scrutinee, hole, later) {
            Walk::Missing => match arms.iter().find(|a| contains(a.span, hole)) {
                Some(arm) => match &arm.body.kind {
                    ExprKind::Block(b) if contains(b.span, hole) => block(b, hole),
                    _ => Walk::Found(None),
                },
                None => Walk::Missing,
            },
            walked => walked,
        },
        ExprKind::Int(_)
        | ExprKind::SinkStr { .. }
        | ExprKind::Bool(_)
        | ExprKind::Unit
        | ExprKind::Name(_)
        | ExprKind::SelfValue => Walk::Missing,
    }
}

/// Whether `e` can leave the code around it: a `?`, `return`, `break` or
/// `continue` anywhere in it.
fn leaves(e: &Expr) -> bool {
    let in_block = |b: &Block| b.stmts.iter().any(stmt_leaves);
    match &e.kind {
        ExprKind::Try(_) => true,
        ExprKind::Field { base, .. } => leaves(base),
        ExprKind::Call { callee, args } => leaves(callee) || args.iter().any(|a| leaves(&a.value)),
        ExprKind::Unary { operand, .. } => leaves(operand),
        ExprKind::Binary { lhs, rhs, .. } => leaves(lhs) || leaves(rhs),
        ExprKind::Range { start, end } => leaves(start) || leaves(end),
        ExprKind::List(items) => items.iter().any(leaves),
        ExprKind::Closure { body, .. } => leaves(body),
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => leaves(cond) || in_block(then_block) || else_branch.as_deref().is_some_and(leaves),
        ExprKind::Block(b) | ExprKind::Scope { body: b, .. } => in_block(b),
        ExprKind::Match { scrutinee, arms } => {
            leaves(scrutinee)
                || arms
                    .iter()
                    .any(|a| leaves(&a.body) || a.guard.as_ref().is_some_and(leaves))
        }
        ExprKind::Int(_)
        | ExprKind::Str(_)
        | ExprKind::SinkStr { .. }
        | ExprKind::Bool(_)
        | ExprKind::Unit
        | ExprKind::Name(_)
        | ExprKind::SelfValue => false,
    }
}

fn stmt_leaves(s: &Stmt) -> bool {
    match &s.kind {
        StmtKind::Return(_) | StmtKind::Break | StmtKind::Continue => true,
        StmtKind::Let { value, .. } => leaves(value),
        StmtKind::Assign { target, value } => leaves(target) || leaves(value),
        StmtKind::For { iter, body, .. } => leaves(iter) || body.stmts.iter().any(stmt_leaves),
        StmtKind::While { cond, body } => leaves(cond) || body.stmts.iter().any(stmt_leaves),
        StmtKind::Expr(e) => leaves(e),
    }
}

/// Whether the hoisted expression calls only pure functions: the probe
/// program, the fixed one with the expression wrapped in a closure passed
/// where a plain `fn` is expected, checks with no diagnostic but those of
/// the other holes, which hold placeholders.
fn pure(
    package: &Package,
    index: usize,
    start: usize,
    hole: Span,
    others: &[Span],
    name: &str,
    expr_text: &str,
) -> bool {
    let src = &package.modules[index].source;
    let sources: Vec<&str> = package.modules.iter().map(|m| m.source.as_str()).collect();
    let probe = fresh(&sources, "hoisted_probe");
    let indent = indent_before(src, start);
    let mut edits = vec![
        Edit {
            span: Span::at(start),
            replacement: format!("let {name} = {probe}(|| {expr_text})\n{indent}"),
        },
        Edit {
            span: hole,
            replacement: format!("{{{name}}}"),
        },
        Edit {
            span: Span::at(src.len()),
            replacement: format!("\n\nfn {probe}<T>(f: fn() -> T) -> T {{\n    f()\n}}\n"),
        },
    ];
    edits.extend(others.iter().map(|h| placeholder(*h)));
    let (text, spans) = apply(src, &edits);
    let placeholders = &spans[3..];
    let mut program = package.clone();
    program.modules[index].source = text;
    let path = &program.modules[index].path;
    let report = check_package_unfixed(&program, CheckOptions::default());
    report.diagnostics.iter().all(|d| {
        let here = match &d.location {
            Some(location) => &location.path == path,
            None => true,
        };
        here && placeholders.iter().any(|p| contains(*p, d.span))
    })
}

/// `base`, or `base` with the first number from 2 that makes it, a word
/// that none of `texts` has.
fn fresh(texts: &[&str], base: &str) -> String {
    let taken = |name: &str| {
        texts.iter().any(|text| {
            text.split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .any(|word| word == name)
        })
    };
    if !taken(base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base}{n}"))
        .find(|name| !taken(name))
        .unwrap_or_else(|| base.to_string())
}

/// The indentation of the line that `start` is on, for the hoisted line.
fn indent_before(src: &str, start: usize) -> String {
    let line_start = src[..start].rfind('\n').map_or(0, |i| i + 1);
    src[line_start..start]
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect()
}
