//! The checked program is typed (#126): every expression and slot
//! carries the type the checker inferred, and every call of a generic
//! records its type arguments, so a backend can lay out values and
//! monomorphize without re-deriving anything (ADR 0007).

use std::ops::ControlFlow;

use oska_check::program::{
    walk, Callee, Expr, ExprKind, Function, IntType, Part, Pattern, Program, Stmt, Type,
    TypeDeclKind, Visit,
};

fn program(src: &str) -> Program {
    let report = oska_check::check_module(src);
    report
        .program
        .unwrap_or_else(|| panic!("{src} should check: {:#?}", report.diagnostics))
}

fn int() -> Type {
    Type::Int(IntType::Int)
}

fn named(name: &str, args: Vec<Type>) -> Type {
    Type::Named {
        name: name.to_string(),
        args,
    }
}

fn param(name: &str) -> Type {
    Type::Param(name.to_string())
}

/// Every expression in `function`, its contracts included, in walk order.
fn exprs(function: &Function) -> Vec<Expr> {
    let mut out = Vec::new();
    for code in function.code() {
        let _ = walk(code, &mut |visit| {
            if let Visit::Expr(expr) = visit {
                out.push(expr.clone());
            }
            ControlFlow::<()>::Continue(())
        });
    }
    out
}

/// The type of the expression in `function` whose source is `text`.
fn type_of(src: &str, program: &Program, function: &str, text: &str) -> Type {
    let found: Vec<Expr> = exprs(&program.functions[function])
        .into_iter()
        .filter(|e| &src[e.span.start..e.span.end] == text)
        .collect();
    match found.as_slice() {
        [first, ..] => first.ty.clone(),
        [] => panic!("no expression `{text}` in `{function}`"),
    }
}

/// The type arguments of every call of `callee` in `function`.
fn type_args_of(program: &Program, function: &str, callee: &Callee) -> Vec<Vec<Type>> {
    exprs(&program.functions[function])
        .into_iter()
        .filter_map(|e| match e.kind {
            ExprKind::Call {
                callee: c,
                type_args,
                ..
            }
            | ExprKind::MutatingCall {
                callee: c,
                type_args,
                ..
            }
            | ExprKind::FnValue {
                callee: c,
                type_args,
            } if &c == callee => Some(type_args),
            _ => None,
        })
        .collect()
}

#[test]
fn expressions_parameters_and_results_carry_their_types() {
    let src = "fn f(a: Int, s: String) -> Bool {\n\
               \x20   let b = a + 1\n\
               \x20   let n: U8 = 3\n\
               \x20   s.len() > b\n\
               }\n";
    let program = program(src);
    let f = &program.functions["f"];
    assert_eq!(f.params[0].ty, int());
    assert_eq!(f.params[1].ty, Type::String);
    assert_eq!(f.ret, Type::Bool);
    assert_eq!(type_of(src, &program, "f", "a + 1"), int());
    assert_eq!(type_of(src, &program, "f", "s.len() > b"), Type::Bool);
    assert_eq!(type_of(src, &program, "f", "s"), Type::String);
    let ExprKind::Block { stmts, .. } = &f.body.kind else {
        panic!("a body is a block");
    };
    let lets: Vec<&Type> = stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::Let { ty, .. } => Some(ty),
            _ => None,
        })
        .collect();
    assert_eq!(lets, [&int(), &Type::Int(IntType::U8)]);
}

#[test]
fn a_generic_function_stays_generic_and_each_call_records_its_type_arguments() {
    let src = "fn id<T>(x: T) -> T { x }\n\
               fn f() -> Bool {\n\
               \x20   let n = id(3)\n\
               \x20   id(n > 2)\n\
               }\n";
    let program = program(src);
    let id = &program.functions["id"];
    assert_eq!(id.generics, ["T"]);
    assert_eq!(id.params[0].ty, param("T"));
    assert_eq!(id.ret, param("T"));
    assert_eq!(type_of(src, &program, "id", "x"), param("T"));
    assert_eq!(
        type_args_of(&program, "f", &Callee::Function("id".to_string())),
        [vec![int()], vec![Type::Bool]]
    );
    assert_eq!(type_of(src, &program, "f", "id(3)"), int());
}

#[test]
fn calls_into_prelude_generics_record_their_type_arguments() {
    let src = "fn f(xs: List<Int>) -> Int {\n\
               \x20   let flags = xs.map(|x| x > 1)\n\
               \x20   let first = xs.get(0)\n\
               \x20   let r: Result<Int, String> = first.ok_or(\"empty\")\n\
               \x20   r.unwrap_or(0)\n\
               }\n";
    let program = program(src);
    let intrinsic = |name: &str| Callee::Intrinsic(name.to_string());
    assert_eq!(
        type_args_of(&program, "f", &intrinsic("List.map")),
        [vec![int(), Type::Bool]],
        "the impl's `T`, then the method's own `U`"
    );
    assert_eq!(
        type_args_of(&program, "f", &intrinsic("List.get")),
        [vec![int()]]
    );
    assert_eq!(
        type_args_of(&program, "f", &intrinsic("Option.ok_or")),
        [vec![int(), Type::String]]
    );
    assert_eq!(
        type_args_of(&program, "f", &intrinsic("Result.unwrap_or")),
        [vec![int(), Type::String]]
    );
    assert_eq!(
        type_of(src, &program, "f", "xs.map(|x| x > 1)"),
        named("List", vec![Type::Bool])
    );
    assert_eq!(
        type_of(src, &program, "f", "|x| x > 1"),
        Type::Fn {
            params: vec![int()],
            ret: Box::new(Type::Bool)
        }
    );
}

#[test]
fn a_generic_impls_methods_name_the_impls_generics_first() {
    let src = "struct Pair<A> { left: A, right: A }\n\
               impl<A> Pair<A> {\n\
               \x20   fn pick<B>(self, other: B) -> B { other }\n\
               }\n\
               fn f(p: Pair<Int>) -> String {\n\
               \x20   p.pick(\"x\")\n\
               }\n";
    let program = program(src);
    let pick = &program.functions["Pair.pick"];
    assert_eq!(pick.generics, ["A", "B"]);
    assert_eq!(pick.self_ty, Some(named("Pair", vec![param("A")])));
    assert_eq!(pick.params[0].ty, param("B"));
    assert_eq!(
        type_args_of(&program, "f", &Callee::Function("Pair.pick".to_string())),
        [vec![int(), Type::String]]
    );
    let pair = &program.types["Pair"];
    assert_eq!(pair.generics, ["A"]);
    let TypeDeclKind::Struct(fields) = &pair.kind else {
        panic!("`Pair` is a struct");
    };
    assert_eq!(fields[0].name.as_deref(), Some("left"));
    assert_eq!(fields[0].ty, param("A"));
}

#[test]
fn the_program_declares_the_prelude_types_it_can_name() {
    let program = program("fn main() {}\n");
    let option = &program.types["Option"];
    assert_eq!(option.generics, ["T"]);
    let TypeDeclKind::Enum(variants) = &option.kind else {
        panic!("`Option` is an enum");
    };
    let shapes: Vec<(&str, Vec<&Type>)> = variants
        .iter()
        .map(|v| (v.name.as_str(), v.fields.iter().map(|f| &f.ty).collect()))
        .collect();
    assert_eq!(shapes, [("Some", vec![&param("T")]), ("None", vec![])]);
    let TypeDeclKind::Struct(fields) = &program.types["IoError"].kind else {
        panic!("`IoError` is a struct");
    };
    assert_eq!(fields[0].ty, Type::String, "trust is not part of a type");
}

#[test]
fn a_default_method_lowered_into_an_impl_sees_the_impls_types() {
    let src = "trait Named {\n\
               \x20   fn name(self) -> String\n\
               \x20   fn greet(self) -> Untrusted<String> {\n\
               \x20       let me = self\n\
               \x20       me.name()\n\
               \x20   }\n\
               }\n\
               struct Cat { age: Int }\n\
               impl Named for Cat {\n\
               \x20   fn name(self) -> String { \"cat\" }\n\
               }\n";
    let program = program(src);
    let greet = &program.functions["<Cat as Named>.greet"];
    assert_eq!(greet.self_ty, Some(named("Cat", vec![])));
    assert!(greet.generics.is_empty());
    let cat = named("Cat", vec![]);
    assert_eq!(type_of(src, &program, "<Cat as Named>.greet", "self"), cat);
    assert_eq!(
        type_of(src, &program, "<Cat as Named>.greet", "me.name()"),
        Type::String
    );
}

#[test]
fn a_trait_call_in_generic_code_records_only_the_methods_own_type_arguments() {
    let src = "struct Wrap<T> { inner: T }\n\
               impl<T: Display> Display for Wrap<T> {\n\
               \x20   fn to_string(self) -> String { \"wrap\" }\n\
               }\n\
               fn show<T: Display>(x: T) -> Untrusted<String> { x.to_string() }\n\
               fn f(w: Wrap<Int>) -> Untrusted<String> {\n\
               \x20   \"{w}\"\n\
               }\n";
    let program = program(src);
    let trait_call = Callee::Trait {
        trait_name: "Display".to_string(),
        method: "to_string".to_string(),
    };
    assert_eq!(
        type_args_of(&program, "show", &trait_call),
        [Vec::<Type>::new()]
    );
    let impl_fn = &program.functions["<Wrap as Display>.to_string"];
    assert_eq!(impl_fn.generics, ["T"]);
    // A backend binds the impl's generics from the receiver's type.
    let mut bound = Vec::new();
    let receiver = named("Wrap", vec![int()]);
    assert!(impl_fn
        .self_ty
        .as_ref()
        .is_some_and(|s| s.bind(&receiver, &mut bound)));
    assert_eq!(bound, [("T".to_string(), int())]);
    // The checker records them for an interpolation hole too.
    let holes: Vec<Vec<Type>> = exprs(&program.functions["f"])
        .into_iter()
        .flat_map(|e| match e.kind {
            ExprKind::Interpolated(parts) => parts,
            _ => Vec::new(),
        })
        .filter_map(|part| match part {
            Part::Hole { type_args, .. } => Some(type_args),
            Part::Text(_) => None,
        })
        .collect();
    assert_eq!(holes, [vec![int()]]);
}

#[test]
fn bindings_carry_their_slot_types() {
    let src = "fn f(xs: List<String>, o: Option<U8>) -> Int {\n\
               \x20   var n = 0\n\
               \x20   for x in xs { n = n + x.len() }\n\
               \x20   match o {\n\
               \x20       Some(b) => n\n\
               \x20       None => 0\n\
               \x20   }\n\
               }\n";
    let program = program(src);
    let f = &program.functions["f"];
    let ExprKind::Block { stmts, value } = &f.body.kind else {
        panic!("a body is a block");
    };
    let fors: Vec<&Type> = stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::For { ty, .. } => Some(ty),
            _ => None,
        })
        .collect();
    assert_eq!(fors, [&Type::String]);
    let Some(ExprKind::Match { arms, .. }) = value.as_ref().map(|v| &v.kind) else {
        panic!("the body ends in a match");
    };
    let Pattern::Construct { fields, .. } = &arms[0].pattern else {
        panic!("`Some(b)` is a constructor pattern");
    };
    assert_eq!(fields[0], Pattern::Bind(Type::Int(IntType::U8)));
}

#[test]
fn contracts_and_tests_are_typed() {
    let src = "fn half(n: Int) -> Int\n\
               \x20   requires n >= 0\n\
               \x20   ensures |r| r * 2 <= n\n\
               {\n\
               \x20   n / 2\n\
               }\n\
               test \"halves\" {\n\
               \x20   let h = half(4)\n\
               \x20   t.assert(h == 2)\n\
               }\n";
    let program = program(src);
    assert_eq!(type_of(src, &program, "half", "n >= 0"), Type::Bool);
    assert_eq!(type_of(src, &program, "half", "r * 2"), int());
    let test = &program.tests[0];
    let ExprKind::Block { stmts, .. } = &test.body.kind else {
        panic!("a test body is a block");
    };
    let Stmt::Let { ty, value, .. } = &stmts[0] else {
        panic!("the test starts with a `let`");
    };
    assert_eq!(ty, &int());
    assert_eq!(value.ty, int());
}
