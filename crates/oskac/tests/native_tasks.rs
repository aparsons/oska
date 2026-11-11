//! Native scopes, tasks and `std.html` (ticket #138), run on both backends
//! through the differential harness (seam 1). The scope programs are the
//! interpreter suite's (`oska-run/tests/scopes.rs`), plus the ones that
//! stress what the native representation adds: counted results, tasks
//! outliving their scope, every way out of a block, and memory. The
//! `std.html` programs are `tests/html.rs`'s, plus hostile input.

#![cfg(all(target_arch = "aarch64", target_os = "linux"))]

mod differential;

use differential::{assert_same, run_native, Case};

/// A program of `items` and a `main` that runs `body` with `io: Stdout`.
fn program(items: &str, body: &str) -> String {
    format!("{items}\nfn main(io: Stdout) -> Result<(), IoError> {{\n{body}\n}}\n")
}

/// Run on both backends, require the same outcome, exit 0 and `stdout`.
fn assert_prints(name: &str, src: &str, stdout: &str) {
    let outcome = assert_same(&Case::file(name, src));
    assert_eq!(outcome.stdout, stdout, "case `{name}`: {outcome:#?}");
    assert_eq!(outcome.exit_code, Some(0), "case `{name}`: {outcome:#?}");
}

// ---- The interpreter's scope suite -------------------------------------------

#[test]
fn join_returns_the_result_of_the_task() {
    let body = "    let n = scope |s| {
        let task = s.spawn(|| 20 + 1)
        task.join() * 2
    }
    io.println(\"{n}\")";
    assert_prints("tasks_join_result", &program("", body), "42\n");
}

#[test]
fn each_join_returns_its_own_task_result() {
    let body = "    let total = scope |s| {
        let a = s.spawn(|| 1)
        let b = s.spawn(|| 10)
        let c = s.spawn(|| 100)
        c.join() + a.join() + b.join()
    }
    io.println(\"{total}\")";
    assert_prints("tasks_each_join", &program("", body), "111\n");
}

#[test]
fn tasks_run_in_spawn_order() {
    let body = "    scope |s| {
        let a = s.spawn(|| io.println(\"first\"))
        let b = s.spawn(|| io.println(\"second\"))
        let c = s.spawn(|| io.println(\"third\"))
        let _ = a.join()
        let _ = b.join()
        c.join()
    }";
    assert_prints(
        "tasks_spawn_order",
        &program("", body),
        "first\nsecond\nthird\n",
    );
}

#[test]
fn joining_a_later_task_first_still_runs_the_earlier_ones_first() {
    let body = "    scope |s| {
        let a = s.spawn(|| io.println(\"first\"))
        let b = s.spawn(|| io.println(\"second\"))
        let _ = b.join()
        a.join()
    }";
    assert_prints("tasks_join_later", &program("", body), "first\nsecond\n");
}

#[test]
fn a_task_runs_to_completion_before_the_next_starts() {
    let body = "    scope |s| {
        let outer = s.spawn(|| {
            let _ = io.println(\"a1\")
            let inner = s.spawn(|| io.println(\"inner\"))
            let _ = io.println(\"a2\")
            inner
        })
        let b = s.spawn(|| {
            let _ = io.println(\"b1\")
            io.println(\"b2\")
        })
        let _ = outer.join()
        b.join()
    }";
    assert_prints(
        "tasks_run_to_completion",
        &program("", body),
        "a1\na2\nb1\nb2\ninner\n",
    );
}

#[test]
fn a_scope_finishes_its_tasks_before_it_ends() {
    let body = "    let _ = scope |s| {
        let _ = s.spawn(|| io.println(\"one\"))
        let _ = s.spawn(|| io.println(\"two\"))
        io.println(\"body done\")
    }
    io.println(\"after\")";
    assert_prints(
        "tasks_finish_before_end",
        &program("", body),
        "body done\none\ntwo\nafter\n",
    );
}

#[test]
fn a_task_sees_the_values_captured_when_it_was_spawned() {
    let body = "    var n = 1
    let total = scope |s| {
        let a = s.spawn(|| n + 1)
        n = 100
        let b = s.spawn(|| n + 1)
        a.join() + b.join()
    }
    io.println(\"{total}\")";
    assert_prints("tasks_capture_by_value", &program("", body), "103\n");
}

#[test]
fn a_task_can_be_joined_after_its_scope_ends() {
    let body = "    let task = scope |s| { s.spawn(|| 7) }
    let n = task.join()
    io.println(\"{n}\")";
    assert_prints("tasks_join_after_scope", &program("", body), "7\n");
}

#[test]
fn a_function_can_spawn_on_a_scope_it_is_given() {
    let items = "fn double_later(s: Scope, n: Int) -> Task<Int> {
    s.spawn(|| n * 2)
}";
    let body = "    let total = scope |s| {
        let a = double_later(s, 4)
        let b = double_later(s, 5)
        a.join() + b.join()
    }
    io.println(\"{total}\")";
    assert_prints("tasks_function_spawns", &program(items, body), "18\n");
}

#[test]
fn nested_scopes_each_run_their_own_tasks() {
    let body = "    scope |outer| {
        let a = outer.spawn(|| io.println(\"outer task\"))
        let _ = scope |inner| {
            inner.spawn(|| io.println(\"inner task\")).join()
        }
        a.join()
    }";
    assert_prints(
        "tasks_nested",
        &program("", body),
        "inner task\nouter task\n",
    );
}

#[test]
fn a_scope_can_be_returned_from_inside() {
    let items = "fn early(n: Int) -> Int {
    scope |s| {
        let task = s.spawn(|| n + 1)
        return task.join()
    }
}";
    let body = "    let n = early(1)\n    io.println(\"{n}\")";
    assert_prints("tasks_return_inside", &program(items, body), "2\n");
}

#[test]
fn a_panic_in_a_task_stops_the_program_at_the_task() {
    let body = "    let n = scope |s| {
        let task = s.spawn(|| 1 / 0)
        task.join()
    }
    io.println(\"{n}\")";
    let outcome = assert_same(&Case::file("tasks_panic_in_task", &program("", body)));
    assert_eq!(outcome.exit_code, Some(101));
    assert!(outcome.stderr.contains("error[P0004]"), "{outcome:#?}");
}

#[test]
fn a_program_with_scopes_gives_identical_results_on_every_run() {
    let items = "fn work(n: Int) -> Int {
    n * n
}";
    let body = "    let total = scope |s| {
        let tasks = [1, 2, 3, 4, 5].map(|n| s.spawn(|| work(n)))
        var sum = 0
        for t in tasks {
            sum = sum + t.join()
        }
        let _ = s.spawn(|| io.println(\"late\"))
        sum
    }
    io.println(\"total {total}\")";
    assert_prints(
        "tasks_repeatable",
        &program(items, body),
        "late\ntotal 55\n",
    );
}

// ---- What the native representation adds ---------------------------------------

#[test]
fn results_that_are_counted_values_survive_joins_and_the_scope() {
    let items = "struct Card { title: String, tags: List<String> }

fn card(n: Int) -> Card {
    Card(title: \"card {n}\", tags: [\"a\", \"b\"])
}";
    let body = "    let tasks = scope |s| {
        let a = s.spawn(|| card(1))
        let b = s.spawn(|| card(2))
        let words = s.spawn(|| \"x y z\".split(\" \"))
        let _ = a.join()
        [a, b]
    }
    for t in tasks {
        let c = t.join()
        let again = t.join()
        let _ = io.println(c.title)
        let _ = io.println(again.title)
    }
    Ok(())";
    assert_prints(
        "tasks_counted_results",
        &program(items, body),
        "card 1\ncard 1\ncard 2\ncard 2\n",
    );
}

#[test]
fn a_task_may_join_a_task_spawned_before_it() {
    let body = "    let n = scope |s| {
        let a = s.spawn(|| 5)
        let b = s.spawn(|| a.join() + 1)
        let c = s.spawn(|| b.join() * 2)
        c.join()
    }
    io.println(\"{n}\")";
    assert_prints("tasks_join_in_task", &program("", body), "12\n");
}

#[test]
fn a_task_spawns_on_its_own_scope_and_the_scope_runs_it() {
    let body = "    scope |s| {
        let _ = s.spawn(|| {
            let _ = io.println(\"parent\")
            let _ = s.spawn(|| {
                let _ = io.println(\"child\")
                s.spawn(|| io.println(\"grandchild\"))
            })
            io.println(\"parent done\")
        })
        io.println(\"body\")
    }";
    assert_prints(
        "tasks_spawn_in_task",
        &program("", body),
        "body\nparent\nparent done\nchild\ngrandchild\n",
    );
}

#[test]
fn a_scope_in_a_loop_leaves_by_break_and_continue_after_running_its_tasks() {
    let body = "    var i = 0
    while i < 5 {
        i = i + 1
        let _ = scope |s| {
            let _ = s.spawn(|| io.println(\"task {i}\"))
            if i == 2 {
                continue
            }
            if i == 4 {
                break
            }
            io.println(\"body {i}\")
        }
    }
    io.println(\"end\")";
    assert_prints(
        "tasks_break_continue",
        &program("", body),
        "body 1\ntask 1\ntask 2\nbody 3\ntask 3\ntask 4\nend\n",
    );
}

#[test]
fn the_question_mark_leaves_a_scope_after_running_its_tasks() {
    let items = "fn first(io: Stdout) -> Result<Int, IoError> {
    scope |s| {
        let _ = s.spawn(|| io.println(\"queued\"))
        io.println(\"before\")?
        io.println(\"after\")?
        Ok(1)
    }
}";
    let body = "    let n = first(io)?
    io.println(\"{n}\")";
    assert_prints(
        "tasks_question_mark",
        &program(items, body),
        "before\nafter\nqueued\n1\n",
    );
}

#[test]
fn a_task_that_panics_before_unstarted_ones_stops_the_program() {
    let body = "    scope |s| {
        let _ = s.spawn(|| io.println(\"one\"))
        let _ = s.spawn(|| 1 / 0)
        let _ = s.spawn(|| io.println(\"never\"))
        io.println(\"body\")
    }";
    let outcome = assert_same(&Case::file("tasks_panic_queue", &program("", body)));
    assert_eq!(outcome.stdout, "body\none\n");
    assert_eq!(outcome.exit_code, Some(101));
}

#[test]
fn function_values_run_as_tasks() {
    let items = "fn seven() -> Int {
    7
}

fn run(s: Scope, f: cap fn() -> Int) -> Int {
    s.spawn(f).join()
}";
    let body = "    let n = scope |s| {
        run(s, seven) + run(s, || 1)
    }
    io.println(\"{n}\")";
    assert_prints("tasks_function_values", &program(items, body), "8\n");
}

#[test]
fn a_task_depth_limit_counts_like_the_interpreter() {
    let items = "fn deep(s: Scope, n: Int) -> Int {
    if n == 0 {
        0
    } else {
        s.spawn(|| deep(s, n - 1)).join() + 1
    }
}";
    let body = "    let n = scope |s| { deep(s, 200) }
    io.println(\"{n}\")";
    assert_prints("tasks_nested_join_depth", &program(items, body), "200\n");
}

#[test]
fn tasks_in_a_loop_free_their_memory() {
    // 30000 rounds of a scope with tasks over a 4 KB text: it would use
    // 100 MB if the tasks, their closures or their results leaked.
    let body = "    let big = \"x\".repeat(4096)
    var total = 0
    var i = 0
    while i < 30000 {
        i = i + 1
        total = total + scope |s| {
            let a = s.spawn(|| big.len())
            let b = s.spawn(|| big)
            let _ = s.spawn(|| big)
            a.join() + b.join().len()
        }
    }
    io.println(\"{total}\")";
    let case = Case::file("tasks_no_leak", &program("", body));
    let outcome = run_native(&case);
    assert_eq!(outcome.stdout, "245760000\n", "{outcome:#?}");
    assert_eq!(outcome.exit_code, Some(0));
}

// ---- A closed scope (P0005) ------------------------------------------------------

/// The checker keeps a handle inside its block (`E0292`), so no source
/// program spawns on a finished scope; a checked program built by hand
/// does, as the interpreter's unit test does. Built natively, it panics
/// with `P0005` at the `spawn` call.
#[test]
fn spawning_on_a_finished_scope_panics_with_p0005_at_the_call() {
    use oska_check::program::{
        Callee, Closure, Contracts, Expr, ExprKind, Function, Program, Stmt, Type,
    };
    use oska_syntax::Span;

    fn at(start: usize, end: usize, kind: ExprKind) -> Expr {
        Expr {
            kind,
            ty: Type::Unit,
            span: Span { start, end },
        }
    }
    let scope_ty = Type::Named {
        name: "Scope".to_string(),
        args: Vec::new(),
    };
    let escaped = at(
        10,
        30,
        ExprKind::Scope(Box::new(at(
            20,
            30,
            ExprKind::Block {
                stmts: Vec::new(),
                value: Some(Box::new(Expr {
                    ty: scope_ty.clone(),
                    ..at(22, 23, ExprKind::Local(0))
                })),
            },
        ))),
    );
    let task = at(
        47,
        51,
        ExprKind::Closure(std::sync::Arc::new(Closure {
            params: Vec::new(),
            captures: Vec::new(),
            captures_self: false,
            height: 1,
            body: at(50, 51, ExprKind::Unit),
        })),
    );
    let spawn = at(
        40,
        52,
        ExprKind::Call {
            callee: Callee::Intrinsic("Scope.spawn".to_string()),
            type_args: vec![Type::Unit],
            args: vec![
                Expr {
                    ty: scope_ty.clone(),
                    ..at(40, 41, ExprKind::Local(0))
                },
                Expr {
                    ty: Type::Fn {
                        params: Vec::new(),
                        ret: Box::new(Type::Unit),
                    },
                    ..task
                },
            ],
        },
    );
    let spawn = Expr {
        ty: Type::Named {
            name: "Task".to_string(),
            args: vec![Type::Unit],
        },
        ..spawn
    };
    let body = at(
        0,
        60,
        ExprKind::Block {
            stmts: vec![Stmt::Let {
                binds: true,
                ty: scope_ty.clone(),
                value: Expr {
                    ty: scope_ty,
                    ..escaped
                },
            }],
            value: Some(Box::new(spawn)),
        },
    );
    let mut program = Program::default();
    program.functions.insert(
        "main".to_string(),
        Function {
            name: "main".to_string(),
            generics: Vec::new(),
            self_ty: None,
            receiver: false,
            params: Vec::new(),
            ret: Type::Unit,
            contracts: Contracts::default(),
            body,
        },
    );
    let image = oska_native::build(&program, &|d| d).expect("the program should build");
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("tasks_p0005");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let exe = dir.join("program");
    std::fs::write(&exe, image).expect("write the executable");
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755))
            .expect("make it executable");
    }
    let output = std::process::Command::new(&exe)
        .output()
        .expect("run the executable");
    assert_eq!(output.status.code(), Some(101));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "error[P0005]: `spawn` was called on a scope whose block has already ended \
         (bytes 40..52)\n"
    );
}

// ---- std.html -------------------------------------------------------------------

/// A program that prints `expr`, an `html.Html`, rendered.
fn printing(expr: &str) -> String {
    format!(
        "use std.html\n\
         fn main(io: Stdout) -> Result<(), IoError> {{\n    \
         io.println({expr}.render())?\n    \
         Ok(())\n}}\n"
    )
}

#[test]
fn text_is_escaped_for_html() {
    assert_prints(
        "html_text",
        &printing("html.text(\"<script>alert('x & \\\"y\\\"')</script>\")"),
        "&lt;script&gt;alert(&#39;x &amp; &quot;y&quot;&#39;)&lt;/script&gt;\n",
    );
}

#[test]
fn trusted_markup_is_inserted_unescaped_around_escaped_text() {
    assert_prints(
        "html_trusted",
        &printing(
            "html.Html.raw(html.TrustedHtml.new(\"<p class=\\\"x\\\">\"))\
             .concat(html.text(\"a < b\"))\
             .concat(html.Html.raw(html.TrustedHtml.new(\"</p>\")))",
        ),
        "<p class=\"x\">a &lt; b</p>\n",
    );
}

/// Every line of stdin, escaped, one per line.
const ESCAPE_STDIN: &str = "use std.html
fn main(io: Stdout, input: Stdin) -> Result<(), IoError> {
    var line = input.read_line()?
    while line.is_some() {
        io.println(html.text(line.unwrap_or(\"\")).render())?
        line = input.read_line()?
    }
    Ok(())
}
";

#[test]
fn hostile_input_comes_out_escaped() {
    let hostile = [
        "<img src=x onerror=alert(1)>",
        "\"><script>alert(document.cookie)</script>",
        "' onmouseover='alert(1)' x='",
        "&lt;already&gt; &amp;amp; &#39; &#x27;",
        "&&&&<<<<>>>>\"\"\"\"''''",
        "a\u{0}b<\u{0}>",
        "caf\u{e9} <\u{65e5}\u{672c}\u{8a9e}> \u{1f600} & \u{1f4a9}'",
        "\u{202e}<b>rtl</b>\u{2028}\u{85}",
        "<!-- comment --> <![CDATA[x]]> <?php ?> </script",
        "",
        "plain text with nothing to escape",
        "<",
        "&",
        "'",
        "x",
    ];
    let stdin = hostile.join("\n") + "\n";
    let case = Case::file("html_hostile", ESCAPE_STDIN).with_stdin(&stdin);
    let outcome = assert_same(&case);
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
    assert!(
        outcome
            .stdout
            .starts_with("&lt;img src=x onerror=alert(1)&gt;\n"),
        "{outcome:#?}"
    );
    assert!(!outcome.stdout.contains('<') && !outcome.stdout.contains('>'));
}

#[test]
fn escaping_a_long_text_and_a_long_chain_of_concats_matches() {
    let src = "use std.html
fn main(io: Stdout) -> Result<(), IoError> {
    let chunk = \"<a href=\\\"x?a=1&b=2\\\">it's</a>\"
    let big = chunk.repeat(5000)
    let escaped = html.text(big).render()
    let size = escaped.len()
    io.println(\"{size}\")?
    var page = html.Html.raw(html.TrustedHtml.new(\"<ul>\"))
    var i = 0
    while i < 300 {
        i = i + 1
        page = page.concat(html.Html.raw(html.TrustedHtml.new(\"<li>\"))).concat(html.text(\"<{i}>\")).concat(html.Html.raw(html.TrustedHtml.new(\"</li>\")))
    }
    io.println(page.concat(html.Html.raw(html.TrustedHtml.new(\"</ul>\"))).render())?
    Ok(())
}
";
    let outcome = assert_same(&Case::file("html_long", src));
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
}

#[test]
fn html_from_untrusted_text_needs_assume_trusted_and_is_then_unescaped() {
    let src = "use std.html
fn main(io: Stdout, input: Stdin) -> Result<(), IoError> {
    let line = input.read_line()?.unwrap_or(\"\")
    let vouched = html.TrustedHtml.new(line.assume_trusted())
    io.println(html.Html.raw(vouched).concat(html.text(line)).render())?
    Ok(())
}
";
    let case = Case::file("html_assume_trusted", src).with_stdin("<b>&</b>\n");
    let outcome = assert_same(&case);
    assert_eq!(outcome.stdout, "<b>&</b>&lt;b&gt;&amp;&lt;/b&gt;\n");
}

#[test]
fn a_program_declaring_one_of_the_modules_names_still_reaches_it() {
    let src = "use std.html
fn text(n: Int) -> Int {
    n
}
fn main(io: Stdout) -> Result<(), IoError> {
    let _ = text(1)
    io.println(html.text(\"&\").render())?
    Ok(())
}
";
    assert_prints("html_shadowed_name", src, "&amp;\n");
}

#[test]
fn html_values_are_shared_and_freed_without_leaking() {
    let src = "use std.html
fn main(io: Stdout) -> Result<(), IoError> {
    let base = html.text(\"<\".repeat(2000))
    var i = 0
    var total = 0
    while i < 20000 {
        i = i + 1
        let page = base.concat(base).concat(html.text(\"&\"))
        total = total + page.render().len()
    }
    io.println(\"{total}\")?
    Ok(())
}
";
    let outcome = run_native(&Case::file("html_no_leak", src));
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
    assert_eq!(outcome.stdout, "320100000\n");
}

#[test]
fn a_task_closure_capturing_a_directory_capability_reads_inside_the_scope() {
    let src = "fn main(io: Stdout, fs: FsRead) -> Result<(), IoError> {
    let root = fs.restrict_to(\"data\")
    let text = scope |s| {
        let a = s.spawn(|| root.read_to_string(Path.new(\"a.txt\")))
        let b = s.spawn(|| root.read_to_string(Path.new(\"missing.txt\")))
        let _ = io.println(\"spawned\")
        let first = a.join()
        let second = b.join()
        let a_text = match first {
            Ok(x) => x
            Err(e) => \"{e}\"
        }
        let b_text = match second {
            Ok(x) => x
            Err(e) => \"{e}\"
        }
        \"{a_text}|{b_text}\"
    }
    io.println(text)
}
";
    let case = Case::file("tasks_dir_capability", src).with_file("data/a.txt", "alpha");
    let outcome = assert_same(&case);
    assert_eq!(outcome.exit_code, Some(0), "{outcome:#?}");
    assert!(
        outcome.stdout.starts_with("spawned\nalpha|"),
        "{outcome:#?}"
    );
}
