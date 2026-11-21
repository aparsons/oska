# Testing with `oskac test`

Tests are top-level items in the same file as the code they test:

```oska tests
fn double(n: Int) -> Int {
    n * 2
}

test "double doubles" {
    t.assert_eq(double(21), 42)
}
```

`oskac test file.osk` checks the file, then runs every `test` block in source order and reports each as passed or failed. A test takes no parameters. Its name is a plain string with no `{...}` interpolation, and a test is never `pub`.

## The built-in value `t`

Inside a test, `t` holds deterministic stand-in capabilities, so a test needs no setup and sees the same world on every run:

| Value | What it is |
| --- | --- |
| `t.fs` | An empty in-memory `Fs`, with its working directory at `/`. Attenuate it like any `Fs`, for example `t.fs.restrict_to("/")` |
| `t.clock` | A `Clock` fixed at the Unix epoch: `now_millis()` is `0` |
| `t.random` | A `Random` with the same fixed seed in every test |
| `t.stdout` | A recording `Stdout`. `t.printed()` returns one entry per `println`, without its line break. |
| `t.approver` | An `Approver` that answers from the test's script: `t.answer_approval(true)` or `t.answer_approval(false)` queues its next answer, and a question with no answer left is refused. `t.approvals_asked()` returns each line it showed, as the terminal approver writes them to stderr. It answers from the script under `--integration` too. |

Each test gets a fresh `t` on a fresh in-memory host, so nothing one test does is visible to another.

The assertions are methods of `t`:

| Call | Fails with |
| --- | --- |
| `t.assert(cond)` | `P0010` when `cond` is `false` |
| `t.assert_eq(actual, expected)` | `P0011` when the values differ through `Eq` |
| `t.assert_ne(actual, other)` | `P0012` when the values are equal |
| `t.fail(message)` | `P0013`, always |
| `t.assert_snapshot(value)` | `P0014` when the value's stored snapshot is missing or differs (see [Snapshots](#snapshots)) |

A failed assertion is a **panic**. It stops that test only, and the other tests still run. A panic from a bug, such as division by zero (`P0004`), fails its test the same way.

## Testing code that needs capabilities

Because capabilities are ordinary values, a function that takes one can be tested by passing it a stand-in. No mocking framework is needed:

```oska tests
fn greet(out: Stdout, name: String) -> Result<(), IoError> {
    out.println("hello, {name}")
}

fn save(out: DirWrite, text: String) -> Result<(), IoError> {
    out.write(Path.new("note.txt"), text)
}

fn word_count(docs: DirRead, path: Path) -> Result<Int, IoError> {
    let text = docs.read_to_string(path)?
    Ok(text.split(" ").len())
}

test "greet prints a greeting" {
    let _ = greet(t.stdout, "Ada")
    let lines = t.printed()
    t.assert_eq(lines.len(), 1)
    t.assert_eq(lines.get(0), Some("hello, Ada"))
}

test "a saved note can be counted" {
    let saved = save(t.fs.restrict_to("/").write_only(), "one two three")
    t.assert(saved.is_ok())
    let counted = word_count(t.fs.restrict_to("/").read_only(), Path.new("note.txt"))
    t.assert_eq(counted, Ok(3))
}

test "a missing file is an error, not a panic" {
    let counted = word_count(t.fs.restrict_to("/").read_only(), Path.new("absent.txt"))
    t.assert(!counted.is_ok())
}

test "the clock is fixed" {
    t.assert_eq(t.clock.now_millis(), 0)
}
```

Notice that `greet` shows its authority in its signature: the test could only pass it a `Stdout`, and cannot give it anything else.

## Reading a report

Without `--format=json` there is one line per test, then the failures, then a summary.

```oska
fn double(n: Int) -> Int {
    n * 2
}

test "adds up" {
    t.assert_eq(double(2), 4)
}

test "gets it wrong" {
    t.assert_eq(double(2), 5)
}

#[integration]
test "reads the real clock" {
    t.assert(t.clock.now_millis() > 0)
}
```

```console cmd=test
test adds up ... ok
test gets it wrong ... FAILED
test reads the real clock ... skipped (#[integration]; run with --integration)

failures:
test gets it wrong: main.osk:10:5: error[P0011]: assertion failed: `actual == expected`: actual 4, expected 5 (bytes 117..142)
    at test "gets it wrong" (main.osk:10:5)

3 tests: 1 passed, 1 failed, 1 skipped
```

Each failure is named by its test and carries a stable code and the byte span of the failing call. Control characters in test names and messages are escaped, so a message cannot forge a line of the report.

With `--format=json` the report is one JSON object on stdout. `status` is `passed`, `failed`, or `skipped`, and `failure` is a diagnostic object or `null`. `duration_ms` is how long the test took, in whole milliseconds, so the slow test is easy to find. Natively it is the test's process from start to exit, process start included; with `--interpret` it is the test's run on a fresh interpreter. A skipped test has 0. Durations vary from run to run:

```console cmd=test json
{"tests":[{"name":"adds up","status":"passed","duration_ms":0,"failure":null},{"name":"gets it wrong","status":"failed","duration_ms":1,"failure":{"code":"P0011","message":"assertion failed: `actual == expected`: actual 4, expected 5","path":"main.osk","span":{"start":117,"end":142},"line":10,"column":5,"end_line":10,"end_column":30,"severity":"error","url":"https://oskalang.dev/codes/P0011","related":[],"suggestions":[],"frames":[{"function":"test \"gets it wrong\"","path":"main.osk","span":{"start":117,"end":142},"line":10,"column":5}],"frames_omitted":0}},{"name":"reads the real clock","status":"skipped","duration_ms":0,"failure":null}],"passed":1,"failed":1,"skipped":1,"diagnostics":[]}
```

If the file does not check, no test runs and the report has no tests and the checker's diagnostics in `diagnostics`.

### Exit codes

| Code | Meaning |
| --- | --- |
| `0` | No test failed. Skipped tests do not count. |
| `1` | A test failed, the checker refused the file, or `--filter` matched no test. |
| `2` | The file uses something the interpreter cannot run yet (`E0371`, `E0380`). No test ran. |

## Running some of the tests

`--filter <substring>` runs only the tests whose names contain the substring, case-sensitive, so checking one fix costs one test. The tests it leaves out are neither run nor listed, and the summary says how many were filtered out. On the program above:

```console cmd=test filter=adds
test adds up ... ok

1 tests: 1 passed, 0 failed, 0 skipped, 2 filtered out
```

With `--format=json` the report has `"filtered"`, the number of tests left out, after `skipped`:

```console cmd=test json filter=adds
{"tests":[{"name":"adds up","status":"passed","duration_ms":0,"failure":null}],"passed":1,"failed":0,"skipped":0,"filtered":2,"diagnostics":[]}
```

A filter that matches no test is an error, with exit code 1, so a typo is never mistaken for a passing run:

```console cmd=test filter=Adds stream=stderr
error: no test name contains `Adds`; 3 tests filtered out
```

```sh
oskac test app.osk --filter "gets it"
```

## Snapshots

`t.assert_snapshot(value)` checks a value that is easier to review than to spell out in `assert_eq`. It renders the value as `dbg` does and compares it with a stored snapshot, one file per assertion:

```oska
pub fn evens(n: Int) -> List<Int> {
    var out: List<Int> = []
    for i in 0..n {
        out.push(i * 2)
    }
    out
}

test "shows evens" {
    t.assert_snapshot(evens(3))
}
```

The file is `snapshots/<test-file>/<test-name>.snap`: in a package, under the package's directory, with `<test-file>` the test's file under `src/` without `.osk` (`snapshots/lib/shows evens.snap` for a test in `src/lib.osk`); for a single file, beside it (`snapshots/app/shows evens.snap` for `app.osk`). A test that runs more than one snapshot assertion numbers them in run order, the order the assertions run in: `shows evens-1.snap`, `shows evens-2.snap`. For a straight-line test, run order is source order. An assertion in a helper or a loop gets a number each time it runs. A character a file name cannot hold, such as `/` or `:`, becomes `_`, and a name Windows keeps for a device, such as `CON`, gets a `_` before it. Two tests whose snapshots would get the same file, such as `a/b` and `a_b`, both fail with `P0014`: rename one of them.

The first run has no snapshot, so the test fails with `P0014`, which names the file and `oskac test --update`, and shows the value as the lines to add:

```text
test lib: shows evens ... FAILED

failures:
test lib: shows evens: src/lib.osk:10:5: error[P0014]: no snapshot `snapshots/lib/shows evens.snap` yet; run `oskac test --update` to write it (bytes 150..177)
    at test "shows evens" (src/lib.osk:10:5)
    + [0, 2, 4]
```

`oskac test --update` writes each missing or changed snapshot instead of failing, lists what it wrote, and never deletes one:

```text
test lib: shows evens ... ok

snapshots written:
new snapshots/lib/shows evens.snap

1 tests: 1 passed, 0 failed, 0 skipped, 1 snapshot written
```

With `--format=json` the report lists them in `"snapshots"`, after `skipped` (and after `filtered`, with `--filter`), as `"snapshots":[{"path":"snapshots/lib/shows evens.snap","status":"new"}]`, each `new` or `changed`. Commit the snapshots with the tests. When the value changes, the test fails with the difference, `- ` for the stored line and `+ ` for the new one; if the change is intended, run `oskac test --update` and review the snapshot's diff.

The test runner, not the test, reads and writes the snapshots: a test keeps only its stand-in capabilities, so `t.fs` cannot see them. A test that panics fails with its panic, and its snapshots are neither compared nor written. The runner follows no symlink under `snapshots/`: a symlinked directory or snapshot fails the test with `P0014`. Snapshots behave the same natively and with `--interpret`.

## Integration tests

A test marked `#[integration]` is skipped unless you pass `--integration`. It then runs on the real operating system: `t.fs` is an `Fs` over the real filesystem (attenuate it like any `Fs`), `t.clock` is the real time, and `t.random` is seeded from the clock. Its `t.stdout` is still recorded and not written to the terminal, so `t.printed()` works and the report stays the only thing on stdout.

```sh
oskac test app.osk
oskac test app.osk --integration
```

`#[integration]` is the only attribute in the language, and it is allowed only on `test` items.

## Output, `dbg`, and isolation

What a test writes to standard error, including `dbg`, goes to the real standard error as it happens, with each line prefixed by the test's name in brackets, such as `[gets it wrong] [dbg] 4`. A runaway recursion fails only the test that caused it (`P0021`).

There is no way to assert that a call panics: a panic always fails the test it happens in. Test the function that returns a `Result` instead, and keep panics for bugs.
