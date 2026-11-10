//! The checked program a backend runs (ADR 0007): what the checker
//! exports beyond its diagnostics.

use oska_check::program::{Closure, Expr, ExprKind, Program, Stmt};

fn program(src: &str) -> Program {
    let report = oska_check::check_module(src);
    report
        .program
        .unwrap_or_else(|| panic!("{src} should check: {:#?}", report.diagnostics))
}

/// Every closure in `expr`, outermost first.
fn closures(expr: &Expr, out: &mut Vec<Closure>) {
    match &expr.kind {
        ExprKind::Closure(closure) => {
            out.push((**closure).clone());
            closures(&closure.body, out);
        }
        ExprKind::Block { stmts, value } => {
            for stmt in stmts {
                if let Stmt::Let { value, .. } | Stmt::Expr(value) = stmt {
                    closures(value, out);
                }
            }
            if let Some(value) = value {
                closures(value, out);
            }
        }
        ExprKind::Call { args, .. } => args.iter().for_each(|a| closures(a, out)),
        _ => {}
    }
}

fn closures_of(program: &Program, function: &str) -> Vec<Closure> {
    let mut out = Vec::new();
    closures(&program.functions[function].body, &mut out);
    out
}

#[test]
fn a_closure_exports_the_slots_it_reads_and_where_its_parameters_start() {
    let src = "fn f(a: Int, b: Int, c: Int) -> Int {\n\
               \x20   let g = |x: Int| x + c + a\n\
               \x20   g(1)\n\
               }\n";
    let found = closures_of(&program(src), "f");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].captures, [0, 2], "`a` and `c`, not `b`");
    assert_eq!(found[0].height, 3, "`x` takes slot 3");
    assert!(!found[0].captures_self);
}

#[test]
fn a_closure_captures_what_a_closure_inside_it_reads_from_outside_both() {
    let src = "fn f(a: Int, b: Int) -> Int {\n\
               \x20   let outer = |x: Int| {\n\
               \x20       let inner = |y: Int| y + x + b\n\
               \x20       inner(1)\n\
               \x20   }\n\
               \x20   outer(2)\n\
               }\n";
    let found = closures_of(&program(src), "f");
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].captures, [1], "the outer closure keeps `b` only");
    assert_eq!(found[0].height, 2);
    assert_eq!(found[1].captures, [1, 2], "the inner one, `b` and `x`");
    assert_eq!(found[1].height, 3, "`y` takes the slot after `a`, `b`, `x`");
}

#[test]
fn a_closure_that_reads_self_captures_the_receiver() {
    let src = "struct Counter { n: Int }\n\
               impl Counter {\n\
               \x20   fn adder(self, unused: Int) -> Int {\n\
               \x20       let add = |x: Int| x + self.n\n\
               \x20       add(1)\n\
               \x20   }\n\
               }\n";
    let found = closures_of(&program(src), "Counter.adder");
    assert_eq!(found.len(), 1);
    assert!(found[0].captures_self);
    assert!(found[0].captures.is_empty(), "{:?}", found[0].captures);
}
