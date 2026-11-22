//! The `reached` method of `std.mcp`'s `Toolset` (#250): the checker writes
//! it for every implementation from the capabilities the authority report
//! computed, and `serve` derives each tool's annotations from it. It must
//! fail closed: nothing a program writes can make it answer less than the
//! checker computed.

mod common;

use std::ops::ControlFlow;

use common::{check, check_modules};
use oska_check::program::{walk, Expr, ExprKind, Program, Visit};

/// Every string literal in `expr`, in order.
fn literals(expr: &Expr) -> Vec<String> {
    let mut found = Vec::new();
    let _ = walk(expr, &mut |visit| {
        if let Visit::Expr(Expr {
            kind: ExprKind::Str(text),
            ..
        }) = visit
        {
            found.push(text.clone());
        }
        ControlFlow::<()>::Continue(())
    });
    found
}

fn reached<'p>(program: &'p Program, suffix: &str) -> &'p Expr {
    let keys: Vec<&String> = program
        .functions
        .keys()
        .filter(|k| k.ends_with(suffix))
        .collect();
    let [key] = keys.as_slice() else {
        panic!("one reached method ending {suffix}: {keys:?}");
    };
    &program.functions[*key].body
}

#[test]
fn every_implementation_gets_a_reached_method_the_checker_wrote() {
    let src = "use std.json\nuse std.mcp\n\n\
               cap struct Tools { notes: DirRead, drafts: DirWrite }\n\n\
               impl mcp.Toolset for Tools {\n    \
               fn specs(self) -> List<mcp.ToolSpec> {\n        []\n    }\n\n    \
               fn call(self, name: String, args: json.Value) -> mcp.ToolResult {\n        \
               match name {\n            \"read\" => mcp.ToolResult.text(self.notes.list_dir(Path.new(\".\")).unwrap_or([]).len().to_string())\n            \
               _ => mcp.ToolResult.error(\"unknown\")\n        }\n    }\n}\n";
    let report = check(src);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let program = report.program.expect("a clean program is checked");
    let body = reached(&program, "Toolset>.reached");
    assert!(matches!(body.kind, ExprKind::If { .. }), "{body:#?}");
    assert_eq!(literals(body), ["read", "DirRead", "DirRead", "DirWrite"]);
}

#[test]
fn a_reached_method_a_program_writes_is_replaced() {
    let src = "use std.json\nuse std.mcp\n\n\
               cap struct Tools { drafts: DirWrite }\n\n\
               impl mcp.Toolset for Tools {\n    \
               fn specs(self) -> List<mcp.ToolSpec> {\n        []\n    }\n\n    \
               fn call(self, name: String, args: json.Value) -> mcp.ToolResult {\n        \
               match name {\n            \"write\" => mcp.ToolResult.text(match self.drafts.write(Path.new(\"a\"), \"\") { Ok(()) => \"ok\"\n Err(_) => \"no\"\n })\n            \
               _ => mcp.ToolResult.error(\"unknown\")\n        }\n    }\n\n    \
               fn reached(self, name: String) -> List<String> {\n        [\"Forged\"]\n    }\n}\n";
    let report = check(src);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let program = report.program.expect("a clean program is checked");
    let body = reached(&program, "Toolset>.reached");
    let found = literals(body);
    assert!(!found.contains(&"Forged".to_string()), "{found:?}");
    assert!(found.contains(&"DirWrite".to_string()), "{found:?}");
}

#[test]
fn an_implementation_in_another_module_gets_its_reached_method_too() {
    let report = check_modules(&[
        (
            "main",
            "use std.json\nuse std.mcp\nuse pkg.tools\n\n\
             fn main(fs: Fs) {\n    \
             let t = tools.Tools(drafts: fs.write_only().restrict_to(\"d\"))\n    \
             let _ = t.call(\"write\", json.Value.Null)\n}\n",
        ),
        (
            "tools",
            "use std.json\nuse std.mcp\n\n\
             pub cap struct Tools { pub drafts: DirWrite }\n\n\
             impl mcp.Toolset for Tools {\n    \
             fn specs(self) -> List<mcp.ToolSpec> {\n        []\n    }\n\n    \
             fn call(self, name: String, args: json.Value) -> mcp.ToolResult {\n        \
             match name {\n            \"write\" => mcp.ToolResult.text(match self.drafts.write(Path.new(\"a\"), \"\") { Ok(()) => \"ok\"\n Err(_) => \"no\"\n })\n            \
             _ => mcp.ToolResult.error(\"unknown\")\n        }\n    }\n}\n",
        ),
    ]);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let program = report.program.expect("a clean program is checked");
    let body = reached(&program, "Toolset>.reached");
    assert!(
        literals(body).contains(&"DirWrite".to_string()),
        "{body:#?}"
    );
}

#[test]
fn a_toolset_the_checker_cannot_fill_answers_with_the_widest_kind() {
    // The trait's own default is what runs when nothing replaces it: it
    // must name the unknown kind, never nothing.
    let source = include_str!("../src/std/mcp.osk");
    let default = source
        .split("fn reached(self, name: String) -> List<String> {")
        .nth(1)
        .and_then(|rest| rest.split('}').next())
        .expect("the trait has a default reached");
    assert!(default.contains("[\"?\"]"), "{default}");
}
