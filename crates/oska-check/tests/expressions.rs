//! Expression checking (#36): function bodies are type-checked with local,
//! bidirectional inference.

mod common;

use common::{assert_errors, assert_fix, assert_ok};

// ---- Bindings and inference ------------------------------------------------

#[test]
fn a_let_type_is_inferred_from_its_initializer() {
    assert_ok("fn f() -> Int {\n    let x = 5\n    x\n}");
    assert_errors(
        "fn f() -> String {\n    let x = 5\n    x\n}",
        &[("E0220", "x")],
    );
}

#[test]
fn shadowing_replaces_a_binding() {
    assert_ok("fn f(raw: Int) -> String {\n    let raw = \"{raw}\"\n    raw\n}");
}

#[test]
fn an_annotated_let_checks_its_initializer() {
    assert_ok("fn f() {\n    let s: String = \"hi\"\n}");
    assert_errors(
        "fn f() {\n    let s: String = true\n}",
        &[("E0220", "true")],
    );
}

#[test]
fn a_binding_with_an_unknowable_type_is_an_error() {
    assert_ok("fn f() -> List<Int> {\n    var xs = List.new()\n    xs.push(1)\n    xs\n}");
    assert_errors("fn f() {\n    let xs = List.new()\n}", &[("E0233", "xs")]);
}

// ---- Literals ----------------------------------------------------------------

#[test]
fn an_integer_literal_takes_its_expected_type() {
    assert_ok("fn f() -> U8 {\n    let x: U8 = 200\n    x\n}");
    assert_ok("fn f() -> I8 { -128 }");
}

#[test]
fn an_integer_literal_defaults_to_int() {
    assert_errors(
        "fn f() -> U8 {\n    let x = 200\n    x\n}",
        &[("E0220", "x")],
    );
}

#[test]
fn an_out_of_range_literal_is_an_error() {
    assert_errors("fn f() -> U8 { 300 }", &[("E0222", "300")]);
    assert_errors("fn f() -> I8 { -129 }", &[("E0222", "-129")]);
    assert_errors("fn f() -> U32 { -1 }", &[("E0223", "-1")]);
    assert_errors(
        "fn f() -> Int { 9_223_372_036_854_775_808 }",
        &[("E0222", "9_223_372_036_854_775_808")],
    );
}

#[test]
fn literals_have_their_types() {
    assert_ok("fn f() -> Bool { true }\nfn g() -> String { \"s\" }\nfn h() -> () { () }");
    assert_errors("fn f() -> Bool { \"s\" }", &[("E0220", "\"s\"")]);
}

#[test]
fn a_constant_is_checked_against_its_type() {
    assert_ok("const LIMIT: U16 = 1000");
    assert_errors("const LIMIT: U8 = 1000", &[("E0222", "1000")]);
}

// ---- Operators ---------------------------------------------------------------

#[test]
fn arithmetic_works_on_numbers_of_one_type() {
    assert_ok("fn f(a: U8, b: U8) -> U8 { a * b + 1 - a / b % 2 }");
    assert_ok("fn f(a: I32) -> I32 { 1 + a }");
}

#[test]
fn there_are_no_implicit_numeric_conversions() {
    assert_errors("fn f(a: Int, b: U8) -> Int { a + b }", &[("E0220", "b")]);
}

#[test]
fn plus_is_only_for_numbers() {
    let src = "fn f(a: String, b: String) -> String { a + b }";
    assert_errors(src, &[("E0223", "a + b")]);
    assert!(common::check(src).diagnostics[0]
        .message
        .contains("interpolation"));
}

#[test]
fn comparisons_produce_bool() {
    assert_ok("fn f(a: Int, b: Int) -> Bool { a < b && a != b || !(a >= b) }");
    assert_ok("fn f(a: String, b: String) -> Bool { a == b }");
    assert_errors(
        "fn f(a: Bool, b: Bool) -> Bool { a < b }",
        &[("E0223", "a < b")],
    );
    assert_errors(
        "fn f(a: Int, b: String) -> Bool { a == b }",
        &[("E0220", "b")],
    );
}

#[test]
fn logical_operators_need_bool() {
    assert_ok("fn f(a: Bool, b: Bool) -> Bool { a implies b }");
    assert_errors("fn f(a: Int) -> Bool { !a }", &[("E0220", "a")]);
    assert_errors(
        "fn f(a: Int, b: Bool) -> Bool { a && b }",
        &[("E0220", "a")],
    );
}

#[test]
fn negation_needs_a_signed_number() {
    assert_ok("fn f(a: I16) -> I16 { -a }");
    assert_errors("fn f(a: U16) -> U16 { -a }", &[("E0223", "-a")]);
}

// ---- Calls and construction ------------------------------------------------

const COPY: &str = "fn copy(src: String, dst: String) -> Int { 0 }\n";

#[test]
fn a_function_call_is_checked_against_its_signature() {
    assert_ok(&format!("{COPY}fn f() -> Int {{ copy(\"a\", \"b\") }}"));
    let src = format!("{COPY}fn f() -> Int {{ copy(\"a\", 2) }}");
    assert_errors(&src, &[("E0220", "2")]);
    let src = format!("{COPY}fn f() -> String {{ copy(\"a\", \"b\") }}");
    assert_errors(&src, &[("E0220", "copy(\"a\", \"b\")")]);
}

#[test]
fn a_call_with_the_wrong_number_of_arguments_is_an_error() {
    let src = format!("{COPY}fn f() -> Int {{ copy(\"a\") }}");
    assert_errors(&src, &[("E0224", "copy(\"a\")")]);
    // Extra arguments are still checked.
    let src = format!("{COPY}fn f() -> Int {{ copy(\"a\", \"b\", nope) }}");
    assert_errors(
        &src,
        &[("E0224", "copy(\"a\", \"b\", nope)"), ("E0221", "nope")],
    );
}

#[test]
fn argument_labels_must_match_parameter_names() {
    assert_ok(&format!(
        "{COPY}fn f() -> Int {{ copy(src: \"a\", dst: \"b\") }}"
    ));
    let src = format!("{COPY}fn f() -> Int {{ copy(src: \"a\", dest: \"b\") }}");
    assert_errors(&src, &[("E0225", "dest")]);
    assert_fix(
        &src,
        "E0225",
        &format!("{COPY}fn f() -> Int {{ copy(src: \"a\", dst: \"b\") }}"),
    );
}

const POINT: &str = "struct Point { x: Int, y: Int }\nstruct Meters(Int)\n";

#[test]
fn named_fields_are_constructed_by_name_in_order() {
    assert_ok(&format!("{POINT}fn f() -> Point {{ Point(x: 1, y: 2) }}"));
    let src = format!("{POINT}fn f() -> Point {{ Point(x: 1, z: 2) }}");
    assert_errors(&src, &[("E0225", "z")]);
    let src = format!("{POINT}fn f() -> Point {{ Point(1, y: 2) }}");
    assert_errors(&src, &[("E0226", "1")]);
    assert_fix(
        &src,
        "E0226",
        &format!("{POINT}fn f() -> Point {{ Point(x: 1, y: 2) }}"),
    );
}

#[test]
fn positional_fields_are_constructed_by_position() {
    assert_ok(&format!("{POINT}fn f() -> Meters {{ Meters(3) }}"));
    let src = format!("{POINT}fn f() -> Meters {{ Meters(m: 3) }}");
    assert_errors(&src, &[("E0225", "m")]);
    assert_fix(
        &src,
        "E0225",
        &format!("{POINT}fn f() -> Meters {{ Meters(3) }}"),
    );
}

#[test]
fn enum_variants_are_constructed_through_their_type() {
    let shape = "enum Shape {\n    Circle(Int),\n    Square { side: Int },\n    Empty,\n}\n";
    assert_ok(&format!(
        "{shape}fn f() -> List<Shape> {{ [Shape.Circle(1), Shape.Square(side: 2), Shape.Empty] }}"
    ));
    let src = format!("{shape}fn f() -> Shape {{ Shape.Circel(1) }}");
    assert_errors(&src, &[("E0228", "Circel")]);
    assert_fix(
        &src,
        "E0228",
        &format!("{shape}fn f() -> Shape {{ Shape.Circle(1) }}"),
    );
}

#[test]
fn a_type_is_not_a_value() {
    assert_errors(
        &format!("{POINT}fn f() {{\n    let p = Point\n}}"),
        &[("E0234", "Point")],
    );
}

#[test]
fn calling_a_non_function_is_an_error() {
    assert_errors("fn f(x: Int) -> Int { x(1) }", &[("E0231", "x")]);
}

#[test]
fn a_function_typed_value_can_be_called() {
    assert_ok("fn apply(f: fn(Int) -> Int, x: Int) -> Int { f(x) }");
}

// ---- Members -----------------------------------------------------------------

#[test]
fn field_access_has_the_fields_type() {
    assert_ok(&format!("{POINT}fn f(p: Point) -> Int {{ p.x + p.y }}"));
    assert_errors(
        &format!("{POINT}fn f(p: Point) -> String {{ p.x }}"),
        &[("E0220", "p.x")],
    );
}

#[test]
fn an_unknown_field_suggests_the_closest() {
    let src = format!("{POINT}fn f(p: Point) -> Int {{ p.z + p.yy }}");
    assert_errors(&src, &[("E0227", "z"), ("E0227", "yy")]);
    assert_fix(
        &format!("{POINT}fn f(p: Point) -> Int {{ p.yy }}"),
        "E0227",
        &format!("{POINT}fn f(p: Point) -> Int {{ p.y }}"),
    );
}

#[test]
fn a_generic_structs_field_is_instantiated() {
    assert_ok("struct Pair<A, B> { a: A, b: B }\nfn f(p: Pair<Int, String>) -> String { p.b }");
}

#[test]
fn inherent_methods_are_checked() {
    let account = "struct Account { balance: Int }\n\
                   impl Account {\n    fn total(self, extra: Int) -> Int { self.balance + extra }\n    \
                   fn open() -> Account { Account(balance: 0) }\n}\n";
    assert_ok(&format!(
        "{account}fn f() -> Int {{ Account.open().total(5) }}"
    ));
    assert_errors(
        &format!("{account}fn f(a: Account) -> Int {{ a.total(\"5\") }}"),
        &[("E0220", "\"5\"")],
    );
    let src = format!("{account}fn f(a: Account) -> Int {{ a.totl(5) }}");
    assert_errors(&src, &[("E0228", "totl")]);
    assert_fix(
        &src,
        "E0228",
        &format!("{account}fn f(a: Account) -> Int {{ a.total(5) }}"),
    );
}

#[test]
fn prelude_methods_are_available() {
    assert_ok(
        "fn f(text: String, args: Args) -> Int {\n    \
         let first: Option<Untrusted<String>> = args.get(1)\n    \
         text.lines().count() + text.len()\n}",
    );
}

#[test]
fn a_method_used_without_a_call_is_an_error() {
    assert_errors("fn f(s: String) -> Int { s.len }", &[("E0227", "len")]);
}

// ---- Control flow ----------------------------------------------------------

#[test]
fn if_branches_must_agree() {
    assert_ok("fn f(c: Bool) -> Int { if c { 1 } else { 2 } }");
    assert_errors(
        "fn f(c: Bool) -> Int { if c { 1 } else { \"two\" } }",
        &[("E0220", "\"two\"")],
    );
    assert_errors(
        "fn f(c: Bool) {\n    let x = if c { 1 } else { \"two\" }\n}",
        &[("E0220", "\"two\"")],
    );
}

#[test]
fn a_condition_must_be_bool() {
    assert_errors("fn f(n: Int) { if n { } }", &[("E0220", "n")]);
    assert_errors("fn f(n: Int) {\n    while n { }\n}", &[("E0220", "n")]);
}

#[test]
fn return_is_checked_against_the_declared_type() {
    assert_ok("fn f(c: Bool) -> Int {\n    if c {\n        return 1\n    }\n    2\n}");
    assert_ok("fn f(c: Bool) -> Int {\n    if c { return 1 } else { return 2 }\n}");
    assert_errors(
        "fn f(c: Bool) -> Int {\n    if c {\n        return \"one\"\n    }\n    2\n}",
        &[("E0220", "\"one\"")],
    );
    assert_errors("fn f() -> Int {\n    return\n}", &[("E0220", "return")]);
}

#[test]
fn a_body_without_a_final_value_is_unit() {
    assert_errors(
        "fn f() -> Int {\n    let x = 1\n}",
        &[("E0220", "{\n    let x = 1\n}")],
    );
}

#[test]
fn for_loops_iterate_over_lists_and_ranges() {
    assert_ok(
        "fn f(xs: List<U8>) -> U8 {\n    var total: U8 = 0\n    \
         for x in xs {\n        total = total + x\n    }\n    \
         for i in 0..10 {\n        let j: Int = i\n    }\n    total\n}",
    );
    assert_errors("fn f(n: Int) {\n    for x in n { }\n}", &[("E0235", "n")]);
}

#[test]
fn break_and_continue_need_a_loop() {
    assert_ok("fn f(c: Bool) {\n    while c {\n        break\n    }\n}");
    assert_errors("fn f() {\n    continue\n}", &[("E0232", "continue")]);
}

#[test]
fn a_blocks_value_is_its_last_expression() {
    assert_ok("fn f() -> Int {\n    let x = {\n        let y = 2\n        y * 2\n    }\n    x\n}");
}

// ---- Mutation --------------------------------------------------------------

#[test]
fn a_var_binding_can_be_assigned() {
    assert_ok("fn f() -> Int {\n    var x = 1\n    x = 2\n    x\n}");
}

#[test]
fn assigning_to_a_let_binding_is_an_error() {
    let src = "fn f() -> Int {\n    let x = 1\n    x = 2\n    x\n}";
    assert_errors(src, &[("E0229", "x")]);
    assert_fix(
        src,
        "E0229",
        "fn f() -> Int {\n    var x = 1\n    x = 2\n    x\n}",
    );
}

#[test]
fn assigning_a_value_of_the_wrong_type_is_an_error() {
    assert_errors(
        "fn f() {\n    var x = 1\n    x = \"two\"\n}",
        &[("E0220", "\"two\"")],
    );
}

#[test]
fn assigning_to_a_field_of_a_let_binding_is_an_error() {
    let point = "struct Point { x: Int, y: Int }\n";
    let src = format!("{point}fn f() {{\n    let p = Point(x: 1, y: 2)\n    p.x = 3\n}}");
    assert_errors(&src, &[("E0229", "p.x")]);
    assert_fix(
        &src,
        "E0229",
        &format!("{point}fn f() {{\n    var p = Point(x: 1, y: 2)\n    p.x = 3\n}}"),
    );
}

#[test]
fn parameters_cannot_be_assigned() {
    let report = common::check("fn f(n: Int) {\n    n = 2\n}");
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(report.diagnostics[0].code, "E0229");
    assert_eq!(report.diagnostics[0].fix, None);
}

#[test]
fn a_mutating_method_needs_a_var_binding() {
    assert_ok("fn f() {\n    var xs: List<Int> = List.new()\n    xs.push(1)\n}");
    let src = "fn f() {\n    let xs: List<Int> = List.new()\n    xs.push(1)\n}";
    assert_errors(src, &[("E0230", "xs")]);
    assert_fix(
        src,
        "E0230",
        "fn f() {\n    var xs: List<Int> = List.new()\n    xs.push(1)\n}",
    );
}

#[test]
fn mutating_self_needs_var_self() {
    let method = |receiver: &str| {
        format!(
            "struct Account {{ balance: Int }}\nimpl Account {{\n    \
             fn deposit({receiver}, n: Int) {{\n        \
             self.balance = self.balance + n\n    }}\n}}"
        )
    };
    assert_ok(&method("var self"));
    assert_errors(&method("self"), &[("E0229", "self.balance")]);
    assert_fix(&method("self"), "E0229", &method("var self"));
}

// ---- Closures --------------------------------------------------------------

#[test]
fn closure_parameters_are_inferred_from_the_expected_type() {
    assert_ok("fn f(xs: List<Int>) -> List<Int> { xs.map(|x| x + 1) }");
    assert_ok("fn f(xs: List<Int>) -> List<String> { xs.map(|x| \"{x}\") }");
    assert_errors(
        "fn f(xs: List<String>) -> List<String> { xs.map(|s| s + 1) }",
        &[("E0223", "s + 1")],
    );
}

#[test]
fn a_closure_body_is_checked_against_the_expected_result() {
    assert_errors(
        "fn apply(f: fn(Int) -> Bool) { }\nfn g() {\n    apply(|x| x + 1)\n}",
        &[("E0220", "x + 1")],
    );
}

#[test]
fn an_annotated_closure_can_stand_alone() {
    assert_ok("fn f() -> Int {\n    let double = |x: Int| x * 2\n    double(4)\n}");
}

// ---- Generics --------------------------------------------------------------

#[test]
fn generic_types_are_instantiated_from_the_expected_type() {
    assert_ok("fn f() -> List<String> {\n    let xs: List<String> = List.new()\n    xs\n}");
    assert_ok("fn f() -> Result<U8, String> { Ok(200) }");
    assert_ok("fn f() -> Option<Int> { None }");
}

#[test]
fn generic_types_are_instantiated_from_their_arguments() {
    assert_ok("fn f() -> Option<String> {\n    let x = Some(\"s\")\n    x\n}");
    assert_errors(
        "fn f() -> Option<Int> {\n    let x = Some(\"s\")\n    x\n}",
        &[("E0220", "x")],
    );
}

#[test]
fn generic_functions_are_instantiated_per_call() {
    let id = "fn id<T>(x: T) -> T { x }\n";
    assert_ok(&format!(
        "{id}fn f() -> String {{\n    let n: Int = id(1)\n    id(\"s\")\n}}"
    ));
    assert_errors(
        &format!("{id}fn f() -> String {{ id(1) }}"),
        &[("E0220", "1")],
    );
}

#[test]
fn a_generic_parameter_is_its_own_type() {
    assert_errors("fn f<T>(x: T) -> Int { x }", &[("E0220", "x")]);
}

// ---- Names -----------------------------------------------------------------

#[test]
fn an_unknown_value_suggests_the_closest_name() {
    let src = "fn f(count: Int) -> Int { cout + 1 }";
    assert_errors(src, &[("E0221", "cout")]);
    assert_fix(src, "E0221", "fn f(count: Int) -> Int { count + 1 }");
    assert_fix(
        "fn f() -> Option<Int> { Nnoe }",
        "E0221",
        "fn f() -> Option<Int> { None }",
    );
}

#[test]
fn a_binding_is_scoped_to_its_block() {
    assert_errors(
        "fn f() -> Int {\n    let a = {\n        let b = 1\n        b\n    }\n    b\n}",
        &[("E0221", "b")],
    );
}

#[test]
fn names_in_interpolation_are_resolved() {
    let point = "struct Point { x: Int, y: Int }\n";
    assert_ok(&format!(
        "{point}fn f(p: Point) -> String {{ \"at {{p.x}}\" }}"
    ));
    assert_errors(
        &format!("{point}fn f(p: Point) -> String {{ \"at {{q}} {{p.z}}\" }}"),
        &[("E0221", "q"), ("E0227", "z")],
    );
}

#[test]
fn self_needs_a_receiver() {
    assert_errors("fn f() -> Int { self }", &[("E0221", "self")]);
}

// ---- The error type --------------------------------------------------------

#[test]
fn one_bad_expression_produces_exactly_one_error() {
    // `nope` is unknown; everything built from it is absorbed.
    assert_errors(
        "fn f(n: Int) -> String {\n    let a = nope + 1\n    \
         let b = a.field.method(a)\n    let c: Bool = b\n    b\n}",
        &[("E0221", "nope")],
    );
}

#[test]
fn every_independent_error_is_reported() {
    assert_errors(
        "fn f() -> Int {\n    let a: String = 1\n    let b: Bool = \"s\"\n    true\n}\n\
         fn g() -> U8 { 256 }",
        &[
            ("E0220", "1"),
            ("E0220", "\"s\""),
            ("E0220", "true"),
            ("E0222", "256"),
        ],
    );
}

#[test]
fn an_unknown_type_in_a_signature_does_not_cascade() {
    assert_errors(
        "fn f(w: Widget) -> Int { w.size + 1 }",
        &[("E0200", "Widget")],
    );
}

// ---- Conformance -----------------------------------------------------------

#[test]
fn the_examples_check_cleanly() {
    assert_ok(include_str!("../../../examples/hello.osk"));
    assert_ok(include_str!("../../../examples/capabilities.osk"));
}
