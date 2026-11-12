//! Native structs, enums, `match` and generic functions (ticket #130),
//! run on both backends through the differential harness (seam 1):
//! compound values built at run time, field reads and updates with
//! copy-on-write (a changed copy never changes the original, ADR 0001),
//! every pattern form, `main` returning `Err`, and generic functions
//! specialised per type.

#![cfg(all(target_arch = "aarch64", target_os = "linux"))]

mod differential;

use differential::{assert_same, Case};

/// A program whose `main` runs `body`, after the items `items`, with
/// `io: Stdout`. (`?` is #133's, so bodies print with
/// `let _ = io.println(..)`.)
fn program(items: &str, body: &str) -> String {
    format!("{items}\n\nfn main(io: Stdout) {{\n{body}\n}}\n")
}

/// Run `src` on both backends, require the same outcome, and that it
/// printed `stdout` and exited 0.
fn assert_prints(name: &str, src: &str, stdout: &str) {
    let outcome = assert_same(&Case::file(name, src));
    assert_eq!(outcome.stdout, stdout, "case `{name}`: {outcome:#?}");
    assert_eq!(outcome.exit_code, Some(0), "case `{name}`: {outcome:#?}");
}

#[test]
fn structs_are_built_at_run_time_and_their_fields_read() {
    let src = program(
        "struct Point { x: Int, y: Int }\n\n\
         struct Named { name: String, at: Point }\n\n\
         fn make(x: Int, label: String) -> Named {\n    \
         Named(name: \"{label}!\", at: Point(x: x, y: x * 2))\n}",
        "    let n = make(3, \"p\")\n    \
         let name = n.name\n    \
         let y = n.at.y\n    \
         let _ = io.println(\"{name} {n.at.x} {y}\")\n    \
         let x = make(7, \"q\").at.x\n    \
         let _ = io.println(\"{x}\")",
    );
    assert_prints("struct_fields", &src, "p! 3 6\n7\n");
}

#[test]
fn enums_match_every_pattern_form() {
    let src = program(
        "enum Shape {\n    Circle { radius: Int },\n    Rect(Int, Int),\n    Label(String),\n    Empty,\n}\n\n\
         enum Color {\n    Red,\n    Green,\n}\n\n\
         struct Pair { a: Int, b: Shape }\n\n\
         fn describe(s: Shape) -> String {\n    \
         match s {\n        \
         Shape.Circle(radius: 0) => \"dot\"\n        \
         Shape.Circle(radius: r) => \"circle {r}\"\n        \
         Shape.Rect(w, -1) => \"odd rect {w}\"\n        \
         Shape.Rect(w, h) if w == h => \"square {w}\"\n        \
         Shape.Rect(_, h) => \"rect by {h}\"\n        \
         Shape.Label(\"hi\") => \"greeting\"\n        \
         Shape.Label(text) => \"label {text}\"\n        \
         Shape.Empty => \"empty\"\n    }\n}\n\n\
         fn color(c: Color) -> Int {\n    \
         match c {\n        Color.Red => 1\n        Color.Green => 2\n    }\n}\n\n\
         fn nested(p: Pair) -> String {\n    \
         match p {\n        \
         Pair(a: 1, b: Shape.Rect(x, _)) => \"one and {x}\"\n        \
         Pair(a: n, b: Shape.Label(t)) if n > 5 => \"big {n} {t}\"\n        \
         Pair(a: n, b: _) => \"other {n}\"\n    }\n}\n\n\
         fn word(n: Int, flag: Bool, text: String) -> String {\n    \
         let first = match n {\n        0 => \"zero\"\n        -3 => \"minus three\"\n        _ => \"many\"\n    }\n    \
         let second = match flag {\n        true => \"yes\"\n        false => \"no\"\n    }\n    \
         let third = match text {\n        \"a\" => \"letter a\"\n        \"\" => \"nothing\"\n        other => \"text {other}\"\n    }\n    \
         let unit = match () {\n        () => \"unit\"\n    }\n    \
         \"{first} {second} {third} {unit}\"\n}",
        "    let _ = io.println(describe(Shape.Circle(radius: 0)))\n    \
         let _ = io.println(describe(Shape.Circle(radius: 4)))\n    \
         let _ = io.println(describe(Shape.Rect(5, -1)))\n    \
         let _ = io.println(describe(Shape.Rect(3, 3)))\n    \
         let _ = io.println(describe(Shape.Rect(3, 4)))\n    \
         let greeting = \"h\"\n    \
         let _ = io.println(describe(Shape.Label(\"{greeting}i\")))\n    \
         let _ = io.println(describe(Shape.Label(\"bye\")))\n    \
         let _ = io.println(describe(Shape.Empty))\n    \
         let r = color(Color.Red) + color(Color.Green) * 10\n    \
         let _ = io.println(\"{r}\")\n    \
         let _ = io.println(nested(Pair(a: 1, b: Shape.Rect(8, 9))))\n    \
         let _ = io.println(nested(Pair(a: 7, b: Shape.Label(\"x\"))))\n    \
         let _ = io.println(nested(Pair(a: 2, b: Shape.Label(\"x\"))))\n    \
         let _ = io.println(word(0, true, \"a\"))\n    \
         let _ = io.println(word(-3, false, \"\"))\n    \
         let _ = io.println(word(9, true, \"zz\"))",
    );
    assert_prints(
        "match_patterns",
        &src,
        "dot\ncircle 4\nodd rect 5\nsquare 3\nrect by 4\ngreeting\nlabel bye\nempty\n21\n\
         one and 8\nbig 7 x\nother 2\nzero yes letter a unit\nminus three no nothing unit\n\
         many yes text zz unit\n",
    );
}

#[test]
fn changing_a_copy_never_changes_the_original() {
    let src = program(
        "struct Inner { n: Int, tag: String }\n\n\
         struct Outer { inner: Inner, count: Int }\n\n\
         fn show(o: Outer) -> String {\n    \"{o.inner.n}/{o.inner.tag}/{o.count}\"\n}\n\n\
         fn bump(o: Outer) -> Outer {\n    var copy = o\n    copy.count = copy.count + 1\n    copy\n}",
        "    var a = Outer(inner: Inner(n: 1, tag: \"x\"), count: 0)\n    \
         var b = a\n    \
         b.inner.n = 2\n    \
         b.inner.tag = \"y\"\n    \
         let c = b\n    \
         b.count = 9\n    \
         let d = bump(a)\n    \
         a.inner = Inner(n: 5, tag: \"{c.inner.tag}z\")\n    \
         let _ = io.println(show(a))\n    \
         let _ = io.println(show(b))\n    \
         let _ = io.println(show(c))\n    \
         let _ = io.println(show(d))\n    \
         var s = Outer(inner: Inner(n: 0, tag: \"static\"), count: 0)\n    \
         let t = s\n    \
         s.inner.n = 1\n    \
         let _ = io.println(\"{s.inner.n} {t.inner.n}\")\n    \
         var e = Inner(n: 0, tag: \"e\")\n    \
         for i in 0..3 {\n        let before = e\n        e.n = e.n + i\n        \
         let _ = io.println(\"{before.n} -> {e.n}\")\n    }",
    );
    assert_prints(
        "copy_on_write",
        &src,
        "5/yz/0\n2/y/9\n2/y/0\n1/x/1\n1 0\n0 -> 0\n0 -> 1\n1 -> 3\n",
    );
}

#[test]
fn mutating_methods_change_their_receiver_and_only_it() {
    let src = program(
        "struct Counter { count: Int, step: Int, log: String }\n\n\
         impl Counter {\n    \
         fn current(self) -> Int {\n        self.count\n    }\n\n    \
         fn tick(var self) -> Int {\n        \
         self.count = self.count + self.step\n        \
         let old = self.log\n        \
         self.log = \"{old}+\"\n        \
         self.count\n    }\n\n    \
         fn twice(var self) {\n        let _ = self.tick()\n        let _ = self.tick()\n    }\n\n    \
         fn reset(var self) {\n        self = Counter(count: 0, step: self.step, log: \"\")\n    }\n\n    \
         fn early(var self, stop: Bool) -> String {\n        \
         self.count = 100\n        \
         if stop {\n            return \"stopped\"\n        }\n        \
         self.count = 200\n        \"ran\"\n    }\n}\n\n\
         struct Holder { counter: Counter, name: String }",
        "    var c = Counter(count: 0, step: 5, log: \"\")\n    \
         let before = c\n    \
         let first = c.tick()\n    \
         c.twice()\n    \
         let _ = io.println(\"{first} {c.count} {c.log} {before.count} {before.log}\")\n    \
         var h = Holder(counter: c, name: \"h\")\n    \
         let saved = h\n    \
         h.counter.twice()\n    \
         let x = h.counter.current()\n    \
         let y = saved.counter.current()\n    \
         let z = c.current()\n    \
         let _ = io.println(\"{x} {y} {z}\")\n    \
         let said = h.counter.early(true)\n    \
         let _ = io.println(\"{said} {h.counter.count}\")\n    \
         let said = h.counter.early(false)\n    \
         let _ = io.println(\"{said} {h.counter.count}\")\n    \
         c.reset()\n    \
         let _ = io.println(\"{c.count} [{c.log}] {before.count}\")",
    );
    assert_prints(
        "mutating_methods",
        &src,
        "5 15 +++ 0 \n25 15 15\nstopped 100\nran 200\n0 [] 0\n",
    );
}

#[test]
fn a_generic_function_runs_at_every_type_it_is_used_at() {
    let src = program(
        "struct Pair<A, B> { first: A, second: B }\n\n\
         enum Tree<T> {\n    Leaf(T),\n    Node(Tree<T>, Tree<T>),\n}\n\n\
         struct Box<T> { value: T }\n\n\
         impl<T> Box<T> {\n    \
         fn get(self) -> T {\n        self.value\n    }\n\n    \
         fn put(var self, value: T) {\n        self.value = value\n    }\n}\n\n\
         impl<T> Display for Box<T> {\n    fn to_string(self) -> String {\n        \"a box\"\n    }\n}\n\n\
         fn pick<T>(first: Bool, a: T, b: T) -> T {\n    if first { a } else { b }\n}\n\n\
         fn swap<A, B>(p: Pair<A, B>) -> Pair<B, A> {\n    Pair(first: p.second, second: p.first)\n}\n\n\
         fn leftmost<T>(t: Tree<T>) -> T {\n    \
         match t {\n        Tree.Leaf(v) => v\n        Tree.Node(l, _) => leftmost(l)\n    }\n}\n\n\
         fn size<T>(t: Tree<T>) -> Int {\n    \
         match t {\n        Tree.Leaf(_) => 1\n        Tree.Node(l, r) => size(l) + size(r)\n    }\n}\n\n\
         fn wrap<T>(value: T) -> Option<T> {\n    Some(value)\n}",
        "    let n = pick(false, 1, 2)\n    \
         let s = pick(true, \"one\", \"two\")\n    \
         let p = swap(Pair(first: 7, second: \"seven\"))\n    \
         let q = swap(p)\n    \
         let _ = io.println(\"{n} {s} {p.first} {p.second} {q.first} {q.second}\")\n    \
         let t = Tree.Node(Tree.Leaf(\"a\"), Tree.Node(Tree.Leaf(\"b\"), Tree.Leaf(\"c\")))\n    \
         let u = Tree.Node(Tree.Node(Tree.Leaf(4), Tree.Leaf(5)), Tree.Leaf(6))\n    \
         let first = leftmost(t)\n    \
         let k = leftmost(u)\n    \
         let sizes = size(t) * 10 + size(u)\n    \
         let _ = io.println(\"{first} {k} {sizes}\")\n    \
         var b = Box(value: \"in\")\n    \
         let kept = b\n    \
         b.put(\"out\")\n    \
         var c = Box(value: 3)\n    \
         c.put(4)\n    \
         let got = b.get()\n    \
         let old = kept.get()\n    \
         let num = c.get()\n    \
         let _ = io.println(\"{got} {old} {num} {b}\")\n    \
         let w = wrap(\"w\")\n    \
         let v = wrap(8)\n    \
         let ws = w.unwrap_or(\"none\")\n    \
         let vs = v.unwrap_or(0)\n    \
         let _ = io.println(\"{ws} {vs}\")",
    );
    assert_prints(
        "generics",
        &src,
        "2 one seven 7 7 seven\na 4 33\nout in 4 a box\nw 8\n",
    );
}

#[test]
fn option_and_result_methods_read_the_variant() {
    let src = program(
        "fn find(n: Int) -> Option<String> {\n    \
         if n > 0 {\n        Some(\"found {n}\")\n    } else {\n        None\n    }\n}\n\n\
         fn yes(b: Bool) -> String {\n    if b { \"true\" } else { \"false\" }\n}",
        "    let a = find(3)\n    \
         let b = find(0)\n    \
         let ra = a.ok_or(\"missing\")\n    \
         let has_a = yes(a.is_some())\n    \
         let has_b = yes(b.is_some())\n    \
         let rb = b.ok_or(\"missing {has_b}\")\n    \
         let x = ra.unwrap_or(\"x\")\n    \
         let y = rb.unwrap_or(\"y\")\n    \
         let ok_a = yes(ra.is_ok())\n    \
         let ok_b = yes(rb.is_ok())\n    \
         let _ = io.println(\"{x} {y} {has_a} {has_b} {ok_a} {ok_b}\")\n    \
         let e = match rb {\n        Ok(_) => \"ok\"\n        Err(message) => message\n    }\n    \
         let _ = io.println(e)",
    );
    assert_prints(
        "option_result_methods",
        &src,
        "found 3 y true false true false\nmissing false\n",
    );
}

#[test]
fn an_err_built_at_run_time_from_main_prints_and_exits_1() {
    for (name, items, err, stderr) in [
        ("err_text", "", "Err(\"bad {n}\")", "bad 3\n"),
        (
            "err_io_error",
            "",
            "Err(IoError(message: \"io {n}\"))",
            "io 3\n",
        ),
        (
            "err_struct",
            "struct Failure { code: Int, why: String }\n\n\
             impl Display for Failure {\n    fn to_string(self) -> String {\n        \
             let why = self.why\n        let code = self.code\n        \
             \"failed with {code}: {why}\"\n    }\n}\n\n",
            "Err(Failure(code: n, why: \"no\"))",
            "failed with 3: no\n",
        ),
        (
            "err_generic",
            "struct Wrapped<T> { inner: T }\n\n\
             impl<T> Display for Wrapped<T> {\n    fn to_string(self) -> String {\n        \
             \"wrapped\"\n    }\n}\n\n",
            "Err(Wrapped(inner: n))",
            "wrapped\n",
        ),
    ] {
        let error_type = match name {
            "err_text" => "String",
            "err_io_error" => "IoError",
            "err_struct" => "Failure",
            _ => "Wrapped<Int>",
        };
        let src = format!(
            "{items}fn main(io: Stdout) -> Result<(), {error_type}> {{\n    \
             let n = 3\n    let _ = io.println(\"before\")\n    {err}\n}}\n"
        );
        let outcome = assert_same(&Case::file(name, &src));
        assert_eq!(outcome.stdout, "before\n", "case `{name}`: {outcome:#?}");
        assert_eq!(outcome.stderr, stderr, "case `{name}`: {outcome:#?}");
        assert_eq!(outcome.exit_code, Some(1), "case `{name}`: {outcome:#?}");
    }
}

#[test]
fn many_values_built_and_dropped_stay_intact() {
    // Freed objects are reused: a count released once too often, or a
    // field read after its object is freed, shows up as a wrong total.
    let src = program(
        "struct Cell { value: Int, label: String }\n\n\
         enum Step {\n    Add(Cell),\n    Skip,\n}\n\n\
         fn step(i: Int) -> Step {\n    \
         if i % 3 == 0 {\n        return Step.Skip\n    }\n    \
         Step.Add(Cell(value: i, label: \"{i}\"))\n}",
        "    var total = 0\n    \
         var last = Cell(value: 0, label: \"start\")\n    \
         for i in 0..20000 {\n        \
         let kept = last\n        \
         match step(i) {\n            \
         Step.Add(cell) => {\n                \
         last = cell\n                \
         last.value = last.value + kept.value % 2\n                \
         total = total + last.value\n            }\n            \
         Step.Skip => {\n                continue\n            }\n        }\n    }\n    \
         let _ = io.println(\"{total} {last.value} {last.label}\")",
    );
    // Values 1..20000 not divisible by 3, plus one for each whose
    // predecessor kept is odd.
    assert_prints("drop_stress", &src, "133336666 19999 19999\n");
}

#[test]
fn arms_that_leave_and_static_values_that_change_behave_as_interpreted() {
    let src = program(
        "struct Point { x: Int, y: Int }\n\n\
         struct Line { from: Point, to: Point, name: String }\n\n\
         impl Line {\n    \
         fn shift(var self, by: Int) {\n        \
         self.from.x = self.from.x + by\n        self.to.y = self.to.y + by\n    }\n}\n\n\
         struct Labelled<T> { label: String, value: T }\n\n\
         impl<T> Display for Labelled<T> {\n    fn to_string(self) -> String {\n        \
         let label = self.label\n        \"<{label}>\"\n    }\n}\n\n\
         fn label<T>(label: String, value: T) -> Labelled<T> {\n    Labelled(label: label, value: value)\n}\n\n\
         fn twice<T>(value: T) -> Labelled<Labelled<T>> {\n    label(\"outer\", label(\"inner\", value))\n}\n\n\
         fn first_long(words: Option<String>, fallback: String) -> String {\n    \
         match words {\n        Some(w) => {\n            return \"early {w}\"\n        }\n        \
         None => {}\n    }\n    fallback\n}",
        "    var p = Point(x: 1, y: 2)\n    \
         let q = p\n    \
         p.x = 10\n    \
         let _ = io.println(\"{p.x} {q.x}\")\n    \
         var l = Line(from: p, to: q, name: \"l\")\n    \
         let m = l\n    \
         l.shift(5)\n    \
         let _ = io.println(\"{l.from.x} {l.to.y} {m.from.x} {m.to.y} {p.x}\")\n    \
         let a = label(\"int\", 3)\n    \
         let b = label(\"text\", \"t\")\n    \
         let c = twice(42)\n    \
         let inner = c.value\n    \
         let _ = io.println(\"{a} {b} {c} {inner} {inner.value}\")\n    \
         let _ = io.println(first_long(Some(\"word\"), \"none\"))\n    \
         let _ = io.println(first_long(None, \"none\"))\n    \
         var found = \"\"\n    \
         for i in 0..10 {\n        \
         let text = \"n{i}\"\n        \
         match Some(text) {\n            \
         Some(t) => {\n                if i == 4 {\n                    found = t\n                    break\n                }\n            }\n            \
         None => {}\n        }\n    }\n    \
         let _ = io.println(found)",
    );
    assert_prints(
        "leaving_arms",
        &src,
        "10 1\n15 7 10 2 10\n<int> <text> <outer> <inner> 42\nearly word\nnone\nn4\n",
    );
}

#[test]
fn parse_int_gives_an_option_as_the_interpreter_does() {
    let src = program(
        "fn show(text: String) -> String {\n    \
         match text.parse_int() {\n        Some(n) => \"{n}\"\n        None => \"none\"\n    }\n}\n\n\
         fn line(io: Stdout, text: String) {\n    \
         let shown = show(text)\n    let _ = io.println(\"[{text}] {shown}\")\n}",
        &[
            "0",
            "42",
            "-17",
            "+5",
            "007",
            "",
            "-",
            "+",
            "1a",
            " 1",
            "1.5",
            "9223372036854775807",
            "9223372036854775808",
            "-9223372036854775808",
            "-9223372036854775809",
            "99999999999999999999",
            "٣",
        ]
        .iter()
        .map(|text| format!("    line(io, \"{text}\")"))
        .collect::<Vec<_>>()
        .join("\n"),
    );
    assert_prints(
        "parse_int",
        &src,
        "[0] 0\n[42] 42\n[-17] -17\n[+5] 5\n[007] 7\n[] none\n[-] none\n[+] none\n[1a] none\n\
         [ 1] none\n[1.5] none\n[9223372036854775807] 9223372036854775807\n\
         [9223372036854775808] none\n[-9223372036854775808] -9223372036854775808\n\
         [-9223372036854775809] none\n[99999999999999999999] none\n[٣] none\n",
    );
}

#[test]
fn paths_released_in_a_loop_keep_their_text() {
    // A `Path`'s text is released with it (its drop function): reused
    // memory would show up as wrong text or a wrong comparison.
    let src = program(
        "",
        "    var same = 0\n    \
         var kept = Path.new(\"start\")\n    \
         for i in 0..5000 {\n        \
         let a = Path.new(\"dir/{i}\")\n        \
         let b = Path.new(\"dir/{i}\")\n        \
         if a == b {\n            same = same + 1\n        }\n        \
         if i % 1000 == 0 {\n            kept = a\n        }\n    }\n    \
         let check = Path.new(\"dir/4000\")\n    \
         let ok = if kept == check { \"kept\" } else { \"lost\" }\n    \
         let _ = io.println(\"{same} {ok}\")",
    );
    assert_prints("path_drop", &src, "5000 kept\n");
}

/// A program that builds a structure `n` levels deep (`build`, a
/// statement list making `c` from `c`, starting at `start`) from its
/// first argument, then drops it.
fn deep(name: &str, items: &str, start: &str, build: &str, n: u32) -> Case {
    let src = format!(
        "{items}\n\nfn main(io: Stdout, args: Args) -> Result<(), IoError> {{\n    \
         let n = args.get(1).unwrap_or(\"0\").parse_int().unwrap_or(0)\n    \
         var c = {start}\n    var i = 0\n    \
         while i < n {{\n{build}\n        i = i + 1\n    }}\n    \
         io.println(\"built {{n}}\")?\n    c = {start}\n    \
         io.println(\"dropped\")?\n    Ok(())\n}}\n"
    );
    Case::file(&format!("{name}_{n}"), &src).with_args(&[&n.to_string()])
}

/// The structures of `deeply_nested_values_are_dropped_in_bounded_stack`.
fn deep_cases(n: u32) -> Vec<Case> {
    vec![
        deep(
            "deep_enum",
            "enum Chain {\n    Link(Int, Chain),\n    End,\n}",
            "Chain.End",
            "        c = Chain.Link(i, c)",
            n,
        ),
        deep(
            "deep_option",
            "struct Node { value: String, next: Option<Node> }",
            "Node(value: \"end\", next: None)",
            "        c = Node(value: \"v{i}\", next: Some(c))",
            n,
        ),
        deep(
            "deep_list",
            "struct Tree { kids: List<Tree> }",
            "Tree(kids: [])",
            "        c = Tree(kids: [c])",
            n,
        ),
        deep(
            "deep_closure",
            "",
            "|x: Int| x",
            "        let f = c\n        c = |x: Int| f(x) + 1",
            n,
        ),
    ]
}

#[test]
fn deeply_nested_values_are_dropped_the_same() {
    for case in deep_cases(100_000) {
        let outcome = assert_same(&case);
        assert_eq!(outcome.stdout, "built 100000\ndropped\n", "{outcome:#?}");
    }
}

#[test]
fn deeply_nested_values_are_dropped_in_bounded_stack() {
    // A release interpreter drops five million links (exit 0); a debug one
    // overflows its own stack first, so this depth is native-only.
    for case in deep_cases(5_000_000) {
        let outcome = differential::run_native(&case);
        assert_eq!(outcome.stdout, "built 5000000\ndropped\n", "{outcome:#?}");
        assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
    }
}
