//! The human-readable tree printed by `oskac parse` without `--format=json`.

use oska_syntax::{module_to_tree, parse_module};

#[test]
fn prints_an_indented_tree() {
    let module =
        parse_module("pub fn main(io: Stdout) {\n    io.println(-1)\n}\n").expect("should parse");
    assert_eq!(
        module_to_tree(&module),
        r#"FnItem 0..46 public=true
  attributes: []
  name: Ident 7..11 name="main"
  generics: []
  params:
    Param 12..22
      name: Ident 12..14 name="io"
      type: PathType 16..22
        segments:
          Ident 16..22 name="Stdout"
        args: []
  requires: []
  ensures: []
  body: Block 24..46
    stmts:
      ExprStmt 30..44
        expr: CallExpr 30..44
          callee: FieldExpr 30..40
            base: NameExpr 30..32 name="io"
            name: Ident 33..40 name="println"
          args:
            Arg 41..43
              value: UnaryExpr 41..43 op="-"
                operand: IntExpr 42..43 value="1"
"#
    );
}
