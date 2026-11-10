//! Scopes and tasks at runtime (#65): spawned tasks run one at a time, in
//! spawn order, `join` returns each task's result, and the same program
//! gives the same result on every run.

mod common;

use common::{assert_panics, assert_runs, run_fake};

/// A program of `items` and a `main` that runs `body` with `io: Stdout`.
fn program(items: &str, body: &str) -> String {
    format!("{items}\nfn main(io: Stdout) -> Result<(), IoError> {{\n{body}\n}}\n")
}

#[test]
fn join_returns_the_result_of_the_task() {
    assert_runs(
        &program(
            "",
            "    let n = scope |s| {
        let task = s.spawn(|| 20 + 1)
        task.join() * 2
    }
    io.println(\"{n}\")",
        ),
        &[],
        "42\n",
    );
}

#[test]
fn each_join_returns_its_own_task_result() {
    assert_runs(
        &program(
            "",
            "    let total = scope |s| {
        let a = s.spawn(|| 1)
        let b = s.spawn(|| 10)
        let c = s.spawn(|| 100)
        c.join() + a.join() + b.join()
    }
    io.println(\"{total}\")",
        ),
        &[],
        "111\n",
    );
}

#[test]
fn tasks_run_in_spawn_order() {
    assert_runs(
        &program(
            "",
            "    scope |s| {
        let a = s.spawn(|| io.println(\"first\"))
        let b = s.spawn(|| io.println(\"second\"))
        let c = s.spawn(|| io.println(\"third\"))
        let _ = a.join()
        let _ = b.join()
        c.join()
    }",
        ),
        &[],
        "first\nsecond\nthird\n",
    );
}

#[test]
fn joining_a_later_task_first_still_runs_the_earlier_ones_first() {
    assert_runs(
        &program(
            "",
            "    scope |s| {
        let a = s.spawn(|| io.println(\"first\"))
        let b = s.spawn(|| io.println(\"second\"))
        let _ = b.join()
        a.join()
    }",
        ),
        &[],
        "first\nsecond\n",
    );
}

#[test]
fn a_task_runs_to_completion_before_the_next_starts() {
    // Each task prints twice. Run one at a time, their lines never mix,
    // even though the first task spawns another halfway through.
    assert_runs(
        &program(
            "",
            "    scope |s| {
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
    }",
        ),
        &[],
        "a1\na2\nb1\nb2\ninner\n",
    );
}

#[test]
fn a_scope_finishes_its_tasks_before_it_ends() {
    // Nothing joins the tasks, yet the scope does not exit until they
    // have run: the line after the scope comes last.
    assert_runs(
        &program(
            "",
            "    let _ = scope |s| {
        let _ = s.spawn(|| io.println(\"one\"))
        let _ = s.spawn(|| io.println(\"two\"))
        io.println(\"body done\")
    }
    io.println(\"after\")",
        ),
        &[],
        "body done\none\ntwo\nafter\n",
    );
}

#[test]
fn a_task_sees_the_values_captured_when_it_was_spawned() {
    assert_runs(
        &program(
            "",
            "    var n = 1
    let total = scope |s| {
        let a = s.spawn(|| n + 1)
        n = 100
        let b = s.spawn(|| n + 1)
        a.join() + b.join()
    }
    io.println(\"{total}\")",
        ),
        &[],
        "103\n",
    );
}

#[test]
fn a_task_can_be_joined_after_its_scope_ends() {
    assert_runs(
        &program(
            "",
            "    let task = scope |s| { s.spawn(|| 7) }
    let n = task.join()
    io.println(\"{n}\")",
        ),
        &[],
        "7\n",
    );
}

#[test]
fn a_function_can_spawn_on_a_scope_it_is_given() {
    assert_runs(
        &program(
            "fn double_later(s: Scope, n: Int) -> Task<Int> {
    s.spawn(|| n * 2)
}",
            "    let total = scope |s| {
        let a = double_later(s, 4)
        let b = double_later(s, 5)
        a.join() + b.join()
    }
    io.println(\"{total}\")",
        ),
        &[],
        "18\n",
    );
}

#[test]
fn nested_scopes_each_run_their_own_tasks() {
    assert_runs(
        &program(
            "",
            "    scope |outer| {
        let a = outer.spawn(|| io.println(\"outer task\"))
        let _ = scope |inner| {
            inner.spawn(|| io.println(\"inner task\")).join()
        }
        a.join()
    }",
        ),
        &[],
        "inner task\nouter task\n",
    );
}

#[test]
fn a_scope_can_be_returned_from_inside() {
    assert_runs(
        &program(
            "fn early(n: Int) -> Int {
    scope |s| {
        let task = s.spawn(|| n + 1)
        return task.join()
    }
}",
            "    let n = early(1)\n    io.println(\"{n}\")",
        ),
        &[],
        "2\n",
    );
}

#[test]
fn a_panic_in_a_task_stops_the_program_at_the_task() {
    let src = program(
        "",
        "    let n = scope |s| {
        let task = s.spawn(|| 1 / 0)
        task.join()
    }
    io.println(\"{n}\")",
    );
    assert_panics(&src, "P0004", "1 / 0");
}

#[test]
fn a_program_with_scopes_gives_identical_results_on_every_run() {
    let src = program(
        "fn work(n: Int) -> Int {
    n * n
}",
        "    let total = scope |s| {
        let tasks = [1, 2, 3, 4, 5].map(|n| s.spawn(|| work(n)))
        var sum = 0
        for t in tasks {
            sum = sum + t.join()
        }
        let _ = s.spawn(|| io.println(\"late\"))
        sum
    }
    io.println(\"total {total}\")",
    );
    let first = run_fake(&src, &[]);
    assert_eq!(first.stdout, "late\ntotal 55\n");
    for _ in 0..20 {
        assert_eq!(run_fake(&src, &[]), first, "a repeated run differs");
    }
}
