//! The JSON export of the syntax tree, as documented in `docs/ast.md`.
//!
//! The envelope is `{ast_version, file, items}`. Every node is an object
//! whose first two fields are `kind` (`PascalCase`) and `span`; its other
//! fields are `snake_case`. Optional fields are omitted when absent, never
//! written as `null`.
//!
//! Serialization is hand-written so the crate stays dependency-free. Nodes
//! are first built as a small [`Json`] value, then printed.

use crate::ast::{
    Arg, Block, ClosureParam, EnumDecl, Expr, ExprKind, Field, Fields, FnDecl, Ident, Item,
    ItemKind, Module, Param, Stmt, StmtKind, StrPart, StructDecl, Type, TypeKind, UnaryOp, Variant,
};
use crate::{json_escape, Span};

/// Version of the export format. Bump only for breaking changes.
pub const AST_VERSION: u64 = 1;

/// Export `module` as pretty-printed JSON. `file` is recorded in the
/// envelope as given.
pub fn module_to_json(module: &Module, file: &str) -> String {
    let envelope = Json::Obj(vec![
        ("ast_version", Json::Num(AST_VERSION)),
        ("file", Json::Str(file.to_string())),
        ("items", Json::Arr(module.items.iter().map(item).collect())),
    ]);
    let mut out = String::new();
    envelope.pretty(&mut out, 0);
    out.push('\n');
    out
}

/// Render `module` as an indented, human-readable tree: one node per line
/// as `Kind start..end`, followed by its scalar fields as `key=value`.
/// Child nodes are indented under the field that holds them.
pub fn module_to_tree(module: &Module) -> String {
    let mut out = String::new();
    for node in module.items.iter().map(item) {
        tree_node(&node, 0, &mut out);
    }
    out
}

/// Print `node`'s header line (without leading padding) and its children.
fn tree_node(node: &Json, indent: usize, out: &mut String) {
    let Json::Obj(fields) = node else {
        node.compact(out);
        out.push('\n');
        return;
    };
    let mut children = Vec::new();
    for (key, value) in fields {
        match (*key, value) {
            ("kind", Json::Str(kind)) => out.push_str(kind),
            ("span", Json::Obj(span)) => {
                for (i, (_, n)) in span.iter().enumerate() {
                    out.push_str(if i == 0 { " " } else { ".." });
                    n.compact(out);
                }
            }
            (_, Json::Obj(_) | Json::Arr(_)) => children.push((*key, value)),
            (key, scalar) => {
                out.push(' ');
                out.push_str(key);
                out.push('=');
                scalar.compact(out);
            }
        }
    }
    out.push('\n');
    let pad = " ".repeat(indent + 2);
    for (key, value) in children {
        out.push_str(&pad);
        out.push_str(key);
        match value {
            Json::Arr(elements) if elements.is_empty() => out.push_str(": []\n"),
            Json::Arr(elements) => {
                out.push_str(":\n");
                for element in elements {
                    out.push_str(&pad);
                    out.push_str("  ");
                    tree_node(element, indent + 4, out);
                }
            }
            node => {
                out.push_str(": ");
                tree_node(node, indent + 2, out);
            }
        }
    }
}

/// A JSON value. Object fields keep their insertion order.
#[derive(Debug, Clone)]
pub(crate) enum Json {
    Bool(bool),
    Num(u64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(&'static str, Json)>),
}

/// Values whose compact form fits in this many columns print on one line.
const LINE_WIDTH: usize = 80;

impl Json {
    pub(crate) fn compact(&self, out: &mut String) {
        match self {
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Num(n) => out.push_str(&n.to_string()),
            Json::Str(s) => {
                out.push('"');
                out.push_str(&json_escape(s));
                out.push('"');
            }
            Json::Arr(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    item.compact(out);
                }
                out.push(']');
            }
            Json::Obj(fields) => {
                out.push('{');
                for (i, (key, value)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push('"');
                    out.push_str(key);
                    out.push_str("\": ");
                    value.compact(out);
                }
                out.push('}');
            }
        }
    }

    /// Print with two-space indentation, keeping short values on one line.
    pub(crate) fn pretty(&self, out: &mut String, indent: usize) {
        let mut flat = String::new();
        self.compact(&mut flat);
        if indent + flat.len() <= LINE_WIDTH {
            out.push_str(&flat);
            return;
        }
        let pad = "  ".repeat(indent / 2 + 1);
        match self {
            Json::Arr(items) => {
                out.push_str("[\n");
                for (i, item) in items.iter().enumerate() {
                    out.push_str(&pad);
                    item.pretty(out, pad.len());
                    if i + 1 < items.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                out.push_str(&pad[2..]);
                out.push(']');
            }
            Json::Obj(fields) => {
                out.push_str("{\n");
                for (i, (key, value)) in fields.iter().enumerate() {
                    out.push_str(&pad);
                    out.push('"');
                    out.push_str(key);
                    out.push_str("\": ");
                    value.pretty(out, pad.len());
                    if i + 1 < fields.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                out.push_str(&pad[2..]);
                out.push('}');
            }
            scalar => out.push_str(&{
                let mut s = String::new();
                scalar.compact(&mut s);
                s
            }),
        }
    }
}

/// Builder for a node object: `kind` and `span` first, then fields.
pub(crate) struct Node(Vec<(&'static str, Json)>);

impl Node {
    pub(crate) fn new(kind: &'static str, span: Span) -> Self {
        Node(vec![
            ("kind", Json::Str(kind.to_string())),
            ("span", span_json(span)),
        ])
    }

    pub(crate) fn field(mut self, key: &'static str, value: Json) -> Self {
        self.0.push((key, value));
        self
    }

    /// Add `key` only when `value` is present.
    pub(crate) fn opt(self, key: &'static str, value: Option<Json>) -> Self {
        match value {
            Some(value) => self.field(key, value),
            None => self,
        }
    }

    pub(crate) fn done(self) -> Json {
        Json::Obj(self.0)
    }
}

fn span_json(span: Span) -> Json {
    Json::Obj(vec![
        ("start", Json::Num(span.start as u64)),
        ("end", Json::Num(span.end as u64)),
    ])
}

fn list<T>(items: &[T], f: impl Fn(&T) -> Json) -> Json {
    Json::Arr(items.iter().map(f).collect())
}

// ----- items -----

fn item(item: &Item) -> Json {
    match &item.kind {
        ItemKind::Fn(decl) => fn_item(item, decl),
        ItemKind::Struct(decl) => struct_item(item, decl),
        ItemKind::Enum(decl) => enum_item(item, decl),
    }
}

fn enum_item(item: &Item, decl: &EnumDecl) -> Json {
    Node::new("EnumItem", item.span)
        .field("public", Json::Bool(item.public))
        .field("cap", Json::Bool(decl.cap))
        .field("name", ident(&decl.name))
        .field("variants", list(&decl.variants, variant))
        .done()
}

fn variant(variant: &Variant) -> Json {
    let (form, fields) = fields_json(&variant.fields);
    Node::new("Variant", variant.span)
        .field("name", ident(&variant.name))
        .field("form", str_json(form))
        .field("fields", fields)
        .done()
}

fn struct_item(item: &Item, decl: &StructDecl) -> Json {
    let (form, fields) = fields_json(&decl.fields);
    Node::new("StructItem", item.span)
        .field("public", Json::Bool(item.public))
        .field("cap", Json::Bool(decl.cap))
        .field("name", ident(&decl.name))
        .field("form", str_json(form))
        .field("fields", fields)
        .done()
}

/// The `form` string and `fields` array for a struct or variant.
fn fields_json(fields: &Fields) -> (&'static str, Json) {
    match fields {
        Fields::Named(fields) => ("named", list(fields, field)),
        Fields::Positional(fields) => ("positional", list(fields, field)),
        Fields::Unit => ("unit", Json::Arr(Vec::new())),
    }
}

fn field(field: &Field) -> Json {
    Node::new("Field", field.span)
        .field("public", Json::Bool(field.public))
        .opt("name", field.name.as_ref().map(ident))
        .field("type", ty(&field.ty))
        .done()
}

fn fn_item(item: &Item, decl: &FnDecl) -> Json {
    Node::new("FnItem", item.span)
        .field("public", Json::Bool(item.public))
        .field("name", ident(&decl.name))
        .field("params", list(&decl.params, param))
        .opt("return_type", decl.ret.as_ref().map(ty))
        .field("body", block(&decl.body))
        .done()
}

fn param(param: &Param) -> Json {
    Node::new("Param", param.span)
        .field("name", ident(&param.name))
        .field("type", ty(&param.ty))
        .done()
}

fn ident(ident: &Ident) -> Json {
    Node::new("Ident", ident.span)
        .field("name", Json::Str(ident.name.clone()))
        .done()
}

fn block(block: &Block) -> Json {
    Node::new("Block", block.span)
        .field("stmts", list(&block.stmts, stmt))
        .done()
}

// ----- statements -----

fn stmt(stmt: &Stmt) -> Json {
    let span = stmt.span;
    match &stmt.kind {
        StmtKind::Let {
            mutable,
            name,
            ty,
            value,
        } => Node::new("LetStmt", span)
            .field("mutable", Json::Bool(*mutable))
            .field("name", ident(name))
            .opt("type", ty.as_ref().map(self::ty))
            .field("value", expr(value)),
        StmtKind::Assign { target, value } => Node::new("AssignStmt", span)
            .field("target", expr(target))
            .field("value", expr(value)),
        StmtKind::Return(value) => {
            Node::new("ReturnStmt", span).opt("value", value.as_ref().map(expr))
        }
        StmtKind::Break => Node::new("BreakStmt", span),
        StmtKind::Continue => Node::new("ContinueStmt", span),
        StmtKind::For {
            binding,
            iter,
            body,
        } => Node::new("ForStmt", span)
            .field("binding", ident(binding))
            .field("iter", expr(iter))
            .field("body", block(body)),
        StmtKind::While { cond, body } => Node::new("WhileStmt", span)
            .field("cond", expr(cond))
            .field("body", block(body)),
        StmtKind::Expr(e) => Node::new("ExprStmt", span).field("expr", expr(e)),
    }
    .done()
}

// ----- expressions -----

fn str_json(s: &str) -> Json {
    Json::Str(s.to_string())
}

fn expr(e: &Expr) -> Json {
    let span = e.span;
    match &e.kind {
        ExprKind::Int(digits) => Node::new("IntExpr", span).field("value", str_json(digits)),
        ExprKind::Str(parts) => Node::new("StrExpr", span).field("parts", list(parts, str_part)),
        ExprKind::SinkStr { tag, value } => Node::new("SinkStrExpr", span)
            .field("tag", str_json(tag))
            .field("value", str_json(value)),
        ExprKind::Bool(b) => Node::new("BoolExpr", span).field("value", Json::Bool(*b)),
        ExprKind::Unit => Node::new("UnitExpr", span),
        // A name's span is the expression's span, so the name is a plain
        // string rather than an `Ident` node.
        ExprKind::Name(name) => Node::new("NameExpr", span).field("name", str_json(&name.name)),
        ExprKind::SelfValue => Node::new("SelfExpr", span),
        ExprKind::Field { base, name } => Node::new("FieldExpr", span)
            .field("base", expr(base))
            .field("name", ident(name)),
        ExprKind::Call { callee, args } => Node::new("CallExpr", span)
            .field("callee", expr(callee))
            .field("args", list(args, arg)),
        ExprKind::Unary { op, operand } => Node::new("UnaryExpr", span)
            .field(
                "op",
                str_json(match op {
                    UnaryOp::Neg => "-",
                    UnaryOp::Not => "!",
                }),
            )
            .field("operand", expr(operand)),
        ExprKind::Binary { op, lhs, rhs } => Node::new("BinaryExpr", span)
            .field("op", str_json(op.text()))
            .field("lhs", expr(lhs))
            .field("rhs", expr(rhs)),
        ExprKind::Try(operand) => Node::new("TryExpr", span).field("operand", expr(operand)),
        ExprKind::Range { start, end } => Node::new("RangeExpr", span)
            .field("start", expr(start))
            .field("end", expr(end)),
        ExprKind::List(elements) => {
            Node::new("ListExpr", span).field("elements", list(elements, expr))
        }
        ExprKind::Closure { params, body } => Node::new("ClosureExpr", span)
            .field("params", list(params, closure_param))
            .field("body", expr(body)),
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => Node::new("IfExpr", span)
            .field("cond", expr(cond))
            .field("then_block", block(then_block))
            .opt("else_branch", else_branch.as_deref().map(expr)),
        // The block's span is the expression's span, so its statements
        // sit directly on the node.
        ExprKind::Block(b) => Node::new("BlockExpr", span).field("stmts", list(&b.stmts, stmt)),
        ExprKind::Scope { handle, body } => Node::new("ScopeExpr", span)
            .field("handle", ident(handle))
            .field("body", block(body)),
    }
    .done()
}

fn str_part(part: &StrPart) -> Json {
    match part {
        StrPart::Text { value, span } => {
            Node::new("StrText", *span).field("value", str_json(value))
        }
        StrPart::Hole { path, span } => {
            Node::new("StrHole", *span).field("path", list(path, ident))
        }
    }
    .done()
}

fn arg(arg: &Arg) -> Json {
    let start = arg.label.as_ref().map_or(arg.value.span, |l| l.span);
    let span = Span {
        start: start.start,
        end: arg.value.span.end,
    };
    Node::new("Arg", span)
        .opt("label", arg.label.as_ref().map(ident))
        .field("value", expr(&arg.value))
        .done()
}

fn closure_param(param: &ClosureParam) -> Json {
    let end = param.ty.as_ref().map_or(param.name.span, |t| t.span);
    let span = Span {
        start: param.name.span.start,
        end: end.end,
    };
    Node::new("ClosureParam", span)
        .field("name", ident(&param.name))
        .opt("type", param.ty.as_ref().map(ty))
        .done()
}

// ----- types -----

fn ty(ty: &Type) -> Json {
    match &ty.kind {
        TypeKind::Path { segments, args } => Node::new("PathType", ty.span)
            .field("segments", list(segments, ident))
            .field("args", list(args, self::ty))
            .done(),
        TypeKind::Unit => Node::new("UnitType", ty.span).done(),
        TypeKind::Fn { cap, params, ret } => Node::new("FnType", ty.span)
            .field("cap", Json::Bool(*cap))
            .field("params", list(params, self::ty))
            .opt("return_type", ret.as_deref().map(self::ty))
            .done(),
    }
}
