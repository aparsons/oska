// SPDX-License-Identifier: Apache-2.0
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
//! an `else if` condition. A hole is `maybe` too when anything that runs
//! before it in its statement is not inert (a call, `?`, arithmetic, a
//! comparison, another expression hole): hoisting would move the hole's
//! expression ahead of an effect, an early exit, or a change to what it
//! reads. A hole in a closure or `match` arm whose body is
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
    // Each expression hole of a statement gets its own name, in order, so
    // fixes of two holes of one statement never both write `let value`.
    let earlier = others
        .iter()
        .filter(|h| site.start <= h.start && h.start < hole.start && h.end <= site.end)
        .count();
    let name = fresh_nth(&[src.as_str()], "value", earlier);
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
    oska_syntax::parse_module(&apply(src, &checked)?).ok()?;
    // Another expression hole earlier in the statement runs before this
    // one, and its expression may have an effect.
    let certain = !site.reorders
        && earlier == 0
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
        match oska_syntax::parse_module(&apply(src, &edits)?) {
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

/// `src` with `edits` applied, or `None` when they cannot be.
fn apply(src: &str, edits: &[Edit]) -> Option<String> {
    oska_syntax::edits::apply(src, edits).ok()
}

/// The statement a hoisted `let` goes before.
struct Site {
    /// Where the statement starts.
    start: usize,
    /// Where the statement ends.
    end: usize,
    /// Hoisting may change what the program does: the expression runs only
    /// sometimes, or more than once, where it is, or something that runs
    /// before it in the statement may have an effect, leave early, or
    /// change what it reads.
    reorders: bool,
}

/// What a walk found of the hole.
enum Walk {
    /// It is not in this code.
    Missing,
    /// It is in this code, in no block of it: the statement around it is
    /// where to hoist. `reorders` as in [`Site`].
    Here { reorders: bool },
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
            Walk::Here { reorders } => Walk::Found(Some(Site {
                start: s.span.start,
                end: s.span.end,
                reorders,
            })),
            found => found,
        };
    }
    Walk::Missing
}

fn stmt(s: &Stmt, hole: Span) -> Walk {
    match &s.kind {
        StmtKind::Let { value, .. } => expr(value, hole, false),
        // Either side may run first, so each is before the other.
        StmtKind::Assign { target, value } => match expr(target, hole, !inert(value)) {
            Walk::Missing => expr(value, hole, !inert(target)),
            walked => walked,
        },
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

/// The first of `exprs`, which run in this order, each with its
/// `reorders`, that the hole is in. One that runs after an expression that
/// is not [`inert`] reorders.
fn first(exprs: &[(&Expr, bool)], hole: Span) -> Walk {
    let mut before = false;
    for (e, reorders) in exprs {
        match expr(e, hole, *reorders || before) {
            Walk::Missing => before = before || !inert(e),
            walked => return walked,
        }
    }
    Walk::Missing
}

/// Whether running `e` can have no effect, cannot leave early or stop the
/// program, and changes nothing: literals, names, fields and lists of
/// them, and `!`, `&&`, `||` and `implies` of them. A call, `?`, a
/// comparison (it may call an `Eq` implementation) and arithmetic (it may
/// overflow or divide by zero) are not.
fn inert(e: &Expr) -> bool {
    use oska_syntax::ast::BinaryOp::{And, Implies, Or};
    use oska_syntax::ast::UnaryOp::Not;
    match &e.kind {
        ExprKind::Int(_)
        | ExprKind::Str(_)
        | ExprKind::SinkStr { .. }
        | ExprKind::Bool(_)
        | ExprKind::Unit
        | ExprKind::Name(_)
        | ExprKind::SelfValue => true,
        ExprKind::Field { base, .. } => inert(base),
        ExprKind::Unary { op: Not, operand } => inert(operand),
        ExprKind::Binary {
            op: And | Or | Implies,
            lhs,
            rhs,
        } => inert(lhs) && inert(rhs),
        ExprKind::Range { start, end } => inert(start) && inert(end),
        ExprKind::List(items) => items.iter().all(inert),
        _ => false,
    }
}

fn expr(e: &Expr, hole: Span, reorders: bool) -> Walk {
    if !contains(e.span, hole) {
        return Walk::Missing;
    }
    match &e.kind {
        ExprKind::Str(_) => Walk::Here { reorders },
        ExprKind::Field { base, .. } => expr(base, hole, reorders),
        ExprKind::Call { callee, args } => {
            let mut all = vec![(callee.as_ref(), reorders)];
            all.extend(args.iter().map(|a| (&a.value, reorders)));
            first(&all, hole)
        }
        ExprKind::Unary { operand, .. } | ExprKind::Try(operand) => expr(operand, hole, reorders),
        ExprKind::Binary { op, lhs, rhs } => {
            use oska_syntax::ast::BinaryOp::{And, Implies, Or};
            let short = matches!(op, And | Or | Implies);
            first(&[(lhs, reorders), (rhs, reorders || short)], hole)
        }
        ExprKind::Range { start, end } => first(&[(start, reorders), (end, reorders)], hole),
        ExprKind::List(items) => {
            let all: Vec<(&Expr, bool)> = items.iter().map(|i| (i, reorders)).collect();
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
        } => match expr(cond, hole, reorders) {
            Walk::Missing if contains(then_block.span, hole) => block(then_block, hole),
            // An `else if` condition runs only when `cond` is false.
            Walk::Missing => else_branch
                .as_ref()
                .map_or(Walk::Missing, |e| expr(e, hole, true)),
            walked => walked,
        },
        ExprKind::Block(b) | ExprKind::Scope { body: b, .. } => block(b, hole),
        ExprKind::Match { scrutinee, arms } => match expr(scrutinee, hole, reorders) {
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
    let Some(text) = apply(src, &edits) else {
        return false;
    };
    // A placeholder keeps its hole's length, so only where it starts moves.
    let placeholders: Vec<Span> = others
        .iter()
        .map(|h| {
            let start = oska_syntax::edits::moved(h.start, &edits);
            Span {
                start,
                end: start + (h.end - h.start),
            }
        })
        .collect();
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
    fresh_nth(texts, base, 0)
}

/// The name after `skip` others of [`fresh`]'s names: of `base`, `base2`,
/// `base3` and so on, the ones that are words none of `texts` has.
fn fresh_nth(texts: &[&str], base: &str, skip: usize) -> String {
    let taken = |name: &str| {
        texts.iter().any(|text| {
            text.split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .any(|word| word == name)
        })
    };
    std::iter::once(base.to_string())
        .chain((2..).map(|n| format!("{base}{n}")))
        .filter(|name| !taken(name))
        .nth(skip)
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

#[cfg(test)]
mod tests {
    use super::add_fixes;
    use crate::package::Package;
    use oska_syntax::{Applicability, Diagnostic, Span};

    const PROGRAM: &str = "fn main() {\n    let n = 1\n    let s = \"{n + 1} and {n * 3}\"\n}\n";

    /// The fix of the hole holding `hole_text` in [`PROGRAM`], given a
    /// diagnostic at it as a parser that went on would report.
    fn fix_of(hole_text: &str) -> oska_syntax::Fix {
        let start = PROGRAM.find(hole_text).expect("hole");
        let span = Span {
            start,
            end: start + hole_text.len(),
        };
        let package = Package::single_file("app.osk", PROGRAM);
        let mut diagnostics = vec![Diagnostic::new("E0102", "hole", span)];
        add_fixes(&package, &mut diagnostics);
        *diagnostics.remove(0).fix.expect("a fix")
    }

    #[test]
    fn two_holes_of_one_statement_get_different_names() {
        let first = fix_of("{n + 1}");
        let second = fix_of("{n * 3}");
        assert_eq!(first.edits[1].replacement, "{value}");
        assert_eq!(second.edits[1].replacement, "{value2}");
        assert_eq!(second.edits[0].replacement, "let value2 = n * 3\n    ");
        // The second runs after the first's expression, so it is `maybe`.
        assert_eq!(first.applicability, Applicability::Machine);
        assert_eq!(second.applicability, Applicability::Maybe);
    }
}
