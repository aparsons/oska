//! Native lists and value semantics (ticket #132), run on both backends
//! through the differential harness (seam 1): list literals and every
//! `List` method for scalar, text, struct and nested-list items, `for`
//! over a list, list equality, `P0002`, copy-on-write aliasing, freed
//! lists releasing their items, and the `String` methods that make or
//! take a list (`split`, `lines`, `chars`, `join`). Also the text search
//! and copy paths the native text speed-up rewrote.

#![cfg(all(
    target_os = "linux",
    any(target_arch = "aarch64", target_arch = "x86_64")
))]

mod differential;

use differential::{assert_same, Case};

/// The helpers every program has. Interpolation takes only names, so a
/// program prints through these.
const HELPERS: &str = r#"fn show(io: Stdout, text: String) {
    let _ = io.println(text)
}

fn yes(io: Stdout, b: Bool) {
    let _ = if b { io.println("true") } else { io.println("false") }
}

fn num(io: Stdout, n: Int) {
    let _ = io.println("{n}")
}
"#;

/// A program with `items` whose `main` runs `body` with `io: Stdout`.
fn program(items: &str, body: &str) -> String {
    format!("{HELPERS}\n{items}\n\nfn main(io: Stdout) {{\n{body}\n}}\n")
}

/// Run the program on both backends, require the same outcome, and that
/// it printed `stdout` and exited 0.
fn assert_prints(name: &str, items: &str, body: &str, stdout: &str) {
    let outcome = assert_same(&Case::file(name, &program(items, body)));
    assert_eq!(outcome.stdout, stdout, "case `{name}`: {outcome:#?}");
    assert_eq!(outcome.exit_code, Some(0), "case `{name}`: {outcome:#?}");
}

#[test]
fn text_search_and_copy_handle_every_needle_and_piece_size() {
    // Needles of 1 to 17 bytes, found at the start, the end, in every
    // byte of a word, many times over (the positions buffer grows), and
    // pieces of every length from 0 to 40 copied around them.
    let body = r#"    let base = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOP"
    var n = 1
    while n <= 17 {
        let needle = base.slice(3, 3 + n)
        let text = "{needle}..{needle}{needle}x{needle}"
        let replaced = text.replace(needle, "<>")
        show(io, replaced)
        yes(io, text.contains(needle))
        yes(io, base.contains("{needle}!"))
        n = n + 1
    }
    var gap = 0
    while gap <= 40 {
        let piece = base.slice(0, gap)
        let text = "|{piece}|{piece}|".repeat(3)
        let replaced = text.replace("|", "[]")
        num(io, replaced.byte_len())
        show(io, replaced.replace("[]", "|").replace(piece, "-"))
        gap = gap + 1
    }
    let many = "ab".repeat(5000)
    let swapped = many.replace("b", "cd")
    num(io, swapped.byte_len())
    yes(io, swapped.ends_with("acd"))
    let shorter = many.replace("ab", "")
    num(io, shorter.byte_len())
    let same = many.replace("zz", "y")
    yes(io, same == many)"#;
    let outcome = assert_same(&Case::file("text_search_sizes", &program("", body)));
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
    assert!(
        outcome.stdout.starts_with("<>..<><>x<>\ntrue\nfalse\n"),
        "{outcome:#?}"
    );
    assert!(
        outcome.stdout.ends_with("15000\ntrue\n0\ntrue\n"),
        "{outcome:#?}"
    );
}

#[test]
fn arguments_passed_in_any_order_arrive_intact() {
    // Parameters and arguments share registers, so a call that permutes
    // its caller's parameters needs a parallel move (with cycles).
    let items = r#"fn digits(a: Int, b: Int, c: Int, d: Int, e: Int, f: Int, g: Int, h: Int) -> Int {
    ((((((a * 10 + b) * 10 + c) * 10 + d) * 10 + e) * 10 + f) * 10 + g) * 10 + h
}

fn shuffle(a: Int, b: Int, c: Int, d: Int, e: Int, f: Int, g: Int, h: Int) -> Int {
    let x = digits(h, g, f, e, d, c, b, a)
    let y = digits(b, a, d, c, f, e, h, g)
    let z = digits(c, a, b, a, h, h, g, d)
    x + y + z
}"#;
    assert_prints(
        "argument_moves",
        items,
        "    num(io, shuffle(1, 2, 3, 4, 5, 6, 7, 8))",
        &format!("{}\n", 87654321 + 21436587 + 31218874),
    );
}

/// Run the program on both backends, require the same outcome, and that
/// it printed `stdout`, then panicked with a report containing `code` and
/// `message`, exit 101.
fn assert_panics(name: &str, items: &str, body: &str, stdout: &str, code: &str, message: &str) {
    let outcome = assert_same(&Case::file(name, &program(items, body)));
    assert_eq!(outcome.stdout, stdout, "case `{name}`: {outcome:#?}");
    assert!(
        outcome
            .stderr
            .contains(&format!("error[{code}]: {message} (bytes")),
        "case `{name}`: {outcome:#?}"
    );
    assert_eq!(outcome.exit_code, Some(101), "case `{name}`: {outcome:#?}");
}

#[test]
fn list_methods_read_scalar_items() {
    assert_prints(
        "list_scalars",
        "fn shown(o: Option<Int>) -> String {\n    match o {\n        Some(n) => \"some {n}\"\n        None => \"none\"\n    }\n}",
        r#"    let xs = [3, 1, 4, 1, 5]
    num(io, xs.len())
    num(io, xs.count())
    num(io, xs.at(0) + xs.at(4))
    show(io, shown(xs.get(2)))
    show(io, shown(xs.get(5)))
    show(io, shown(xs.get(-1)))
    var built: List<Int> = List.new()
    num(io, built.len())
    var i = 0
    while i < 100 {
        built.push(i * i)
        i = i + 1
    }
    num(io, built.len())
    num(io, built.at(99))
    var sum = 0
    for x in built {
        sum = sum + x
    }
    num(io, sum)
    let flags = [true, false, true]
    for f in flags {
        yes(io, f)
    }
    let empty: List<Int> = []
    for e in empty {
        num(io, e)
    }
    num(io, empty.len())
    let small: List<U8> = [255, 0]
    num(io, small.len())"#,
        "5\n5\n8\nsome 4\nnone\nnone\n0\n100\n9801\n328350\ntrue\nfalse\ntrue\n0\n2\n",
    );
}

#[test]
fn list_items_may_be_text_structs_and_lists() {
    assert_prints(
        "list_compound",
        "struct Point { x: Int, label: String }\n\n\
         fn label(p: Point) -> String {\n    p.label\n}\n\n\
         fn make(n: Int) -> List<Point> {\n    var out: List<Point> = []\n    var i = 0\n    while i < n {\n        out.push(Point(x: i, label: \"p{i}\"))\n        i = i + 1\n    }\n    out\n}",
        r#"    let words = ["alpha", "beta", "gamma"]
    for w in words {
        show(io, w)
    }
    show(io, words.at(1))
    var more = words
    let n = words.len()
    more.push("delta {n}")
    show(io, more.at(3))
    num(io, words.len())
    let points = make(30)
    show(io, label(points.at(29)))
    num(io, points.at(7).x)
    var grid: List<List<Int>> = []
    var r = 0
    while r < 4 {
        var row: List<Int> = []
        var c = 0
        while c < r {
            row.push(c)
            c = c + 1
        }
        grid.push(row)
        r = r + 1
    }
    num(io, grid.len())
    num(io, grid.at(3).len())
    num(io, grid.at(3).at(2))
    let nested = [[1, 2], [], [3]]
    for row in nested {
        num(io, row.len())
    }"#,
        "alpha\nbeta\ngamma\nbeta\ndelta 3\n3\np29\n7\n4\n3\n2\n2\n0\n1\n",
    );
}

#[test]
fn changing_a_copy_of_a_list_never_changes_the_original() {
    assert_prints(
        "list_aliasing",
        "struct Bag { items: List<String>, count: Int }\n\n\
         impl Bag {\n    fn add(var self, item: String) {\n        self.items.push(item)\n        self.count = self.count + 1\n    }\n}\n\n\
         fn grow(xs: List<Int>) -> List<Int> {\n    var copy = xs\n    copy.push(99)\n    copy\n}",
        r#"    let original = [1, 2, 3]
    var copy = original
    copy.push(4)
    num(io, original.len())
    num(io, copy.len())
    let grown = grow(original)
    num(io, grown.len())
    num(io, original.len())
    var a: List<String> = List.new()
    a.push("x")
    var b = a
    b.push("y")
    a.push("z")
    show(io, a.at(1))
    show(io, b.at(1))
    var bag = Bag(items: ["first"], count: 1)
    let before = bag
    bag.add("second")
    num(io, before.items.len())
    num(io, bag.items.len())
    show(io, bag.items.at(1))
    var lists = [[1], [2]]
    let snapshot = lists
    var inner = lists.at(0)
    inner.push(5)
    lists.push(inner)
    num(io, snapshot.len())
    num(io, lists.at(2).len())
    num(io, snapshot.at(0).len())
    var looped = [10, 20]
    for x in looped {
        looped.push(x + 1)
    }
    num(io, looped.len())"#,
        "3\n4\n4\n3\nz\ny\n1\n2\nsecond\n2\n2\n1\n4\n",
    );
}

#[test]
fn lists_compare_item_by_item() {
    assert_prints(
        "list_equality",
        "struct Point { x: Int, y: Int }\n\n\
         impl Eq for Point {\n    fn eq(self, other: Point) -> Bool {\n        self.x == other.x\n    }\n}",
        r#"    yes(io, [1, 2, 3] == [1, 2, 3])
    yes(io, [1, 2, 3] == [1, 2])
    yes(io, [1, 2, 3] != [1, 2, 4])
    let empty: List<Int> = []
    yes(io, empty == List.new())
    yes(io, ["a", "b"] == ["a", "b"])
    yes(io, ["a", "b"] == ["a", "c"])
    yes(io, [[1], [2, 3]] == [[1], [2, 3]])
    yes(io, [[1], [2, 3]] == [[1], [3, 2]])
    yes(io, [Point(x: 1, y: 2)] == [Point(x: 1, y: 9)])
    yes(io, [Point(x: 1, y: 2)] == [Point(x: 2, y: 2)])
    yes(io, [true] == [false])"#,
        "true\nfalse\ntrue\ntrue\ntrue\nfalse\ntrue\nfalse\ntrue\nfalse\nfalse\n",
    );
}

#[test]
fn an_index_out_of_bounds_panics_with_p0002() {
    for (name, index, length) in [("list_at_past", "3", 3), ("list_at_negative", "-1", 3)] {
        assert_panics(
            name,
            "",
            &format!("    let xs = [\"a\", \"b\", \"c\"]\n    show(io, xs.at(0))\n    show(io, xs.at({index}))"),
            "a\n",
            "P0002",
            &format!("index {index} is out of bounds for a list of length {length}"),
        );
    }
}

#[test]
fn freed_lists_release_their_items() {
    // About 1.5 GB of text goes through lists that are built, copied,
    // changed and dropped. Run with its address space limited to 512 MB,
    // a list that kept its items, or an item never freed, runs out of
    // memory.
    let body = r#"    var total = 0
    var round = 0
    while round < 3000 {
        let piece = "x".repeat(1000)
        var xs: List<String> = []
        var i = 0
        while i < 500 {
            xs.push("{piece}{i}")
            i = i + 1
        }
        var copy = xs
        copy.push(piece)
        let nested = [xs, copy]
        for inner in nested {
            total = total + inner.len()
        }
        round = round + 1
    }
    num(io, total)"#;
    let case = Case::file("list_release", &program("", body));
    let (exe, dir) = differential::build_native(&case);
    let output = std::process::Command::new("sh")
        .args([
            "-c",
            "ulimit -v 524288 && exec \"$0\"",
            exe.to_str().unwrap_or_default(),
        ])
        .current_dir(&dir)
        .output()
        .expect("the program should run");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "3003000\n",
        "{output:?}"
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
}

#[test]
fn leaving_a_loop_over_a_list_early_releases_what_it_holds() {
    assert_prints(
        "list_leaving",
        "fn first_long(words: List<String>) -> String {\n    for w in words {\n        if w.len() > 3 {\n            return w\n        }\n    }\n    \"none\"\n}\n\n\
         fn pick(word: String, o: Option<String>) -> String {\n    match o {\n        Some(w) => w\n        None => word\n    }\n}\n\n\
         fn words(n: Int) -> List<String> {\n    var out: List<String> = []\n    var i = 0\n    while i < n {\n        out.push(\"w{i}\".repeat(i % 3 + 1))\n        i = i + 1\n    }\n    out\n}",
        r#"    var round = 0
    var total = 0
    var last = ""
    while round < 2000 {
        let ws = words(round % 7 + 2)
        last = first_long(ws)
        var seen = 0
        for w in words(5) {
            if w.len() == 2 {
                continue
            }
            seen = seen + w.len()
            if seen > 8 {
                break
            }
        }
        total = total + seen
        last = pick(last, ws.get(1))
        round = round + 1
    }
    show(io, last)
    num(io, total)
    let ws = words(3)
    show(io, pick("none", ws.get(2)))
    show(io, pick("none", ws.get(3)))"#,
        "w1w1\n20000\nw2w2w2\nnone\n",
    );
}

#[test]
fn text_splits_into_lists_and_joins_from_them() {
    assert_prints(
        "list_text",
        "fn all(io: Stdout, xs: List<String>) {\n    let joined = \"|\".join(xs)\n    let n = xs.len()\n    show(io, \"{n}:{joined}\")\n}",
        r#"    all(io, "a,b,,c".split(","))
    all(io, "".split(","))
    all(io, ",".split(","))
    all(io, "abc".split(""))
    all(io, "".split(""))
    all(io, "héllo wörld".split("ö"))
    all(io, "a--b--".split("--"))
    all(io, "one\ntwo\r\nthree".lines())
    all(io, "a\n\nb\n".lines())
    all(io, "".lines())
    all(io, "\n".lines())
    all(io, "x\r".lines())
    all(io, "\r\n\r\n".lines())
    all(io, "aé😀".chars())
    all(io, "".chars())
    show(io, ", ".join(["x", "y", "z"]))
    show(io, ", ".join([]))
    show(io, "".join(["solo"]))
    show(io, "-".join("a b c".split(" ")))"#,
        "4:a|b||c\n1:\n2:|\n5:|a|b|c|\n2:|\n2:héllo w|rld\n3:a|b|\n3:one|two|three\n3:a||b\n0:\n1:\n1:x\r\n2:|\n3:a|é|😀\n0:\nx, y, z\n\nsolo\na-b-c\n",
    );
}

#[test]
fn for_over_chars_walks_the_text_and_leaves_it_at_any_point() {
    // The native backend walks `for c in text.chars()` a character at a
    // time with no list: returning a character, `break`, `continue`,
    // nested loops, a binding of `_`, characters kept after the loop, and
    // a receiver that is a temporary text, over one- to four-byte ones.
    assert_prints(
        "for_chars",
        "fn first_non_space(line: String) -> String {\n    for c in line.chars() {\n        if c != \" \" {\n            return c\n        }\n    }\n    \"\"\n}\n\n\
         fn count_until(line: String, stop: String) -> Int {\n    var n = 0\n    for c in line.chars() {\n        if c == stop {\n            break\n        }\n        if c == \"é\" {\n            continue\n        }\n        n = n + 1\n    }\n    n\n}\n\n\
         fn twice(line: String) -> List<String> {\n    var out: List<String> = List.new()\n    for c in \"{line}!\".chars() {\n        out.push(c)\n        out.push(c)\n    }\n    out\n}",
        r#"    show(io, first_non_space("   héllo"))
    show(io, first_non_space("    "))
    show(io, first_non_space("😀x"))
    let a = count_until("aébécd", "c")
    let b = count_until("", "c")
    show(io, "{a} {b}")
    show(io, "-".join(twice("aé😀")))
    var pairs = 0
    for x in "ab".chars() {
        for y in "xyz".chars() {
            if y == "z" {
                break
            }
            pairs = pairs + 1
        }
    }
    var n = 0
    for _ in "añb😀".chars() {
        n = n + 1
    }
    show(io, "{pairs} {n}")"#,
        "h\n\n😀\n2 0\na-a-é-é-😀-😀-!-!\n4 4\n",
    );
}

#[test]
fn dbg_shows_a_list_item_by_item() {
    let items = "struct Point { x: Int, y: Int }";
    let body = r#"    let empty: List<Int> = []
    let a = dbg(empty)
    let b = dbg([1, -2, 3])
    let c = dbg(["a", "b\"c"])
    let d = dbg([[1], [], [2, 3]])
    let e = dbg([Point(x: 1, y: 2)])
    var built: List<Option<Int>> = []
    built.push(Some(1))
    built.push(None)
    let f = dbg(built)
    num(io, b.len() + d.len() + f.len())"#;
    let outcome = assert_same(&Case::file("list_dbg", &program(items, body)));
    assert_eq!(
        outcome.stderr,
        "[dbg] []\n[dbg] [1, -2, 3]\n[dbg] [\"a\", \"b\\\"c\"]\n[dbg] [[1], [], [2, 3]]\n\
         [dbg] [Point(1, 2)]\n[dbg] [Option.Some(1), Option.None]\n",
        "{outcome:#?}"
    );
    assert_eq!(outcome.stdout, "8\n", "{outcome:#?}");
}

#[test]
fn list_map_builds_a_new_list_of_the_closures_results() {
    let body = "    let xs = [1, 2]\n    let ys = xs.map(|x| x + 1)\n    num(io, ys.len())\n    num(io, ys.at(0) + ys.at(1))\n    num(io, xs.at(0))";
    assert_prints("list_map", "", body, "2\n5\n1\n");
}
