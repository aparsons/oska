// SPDX-License-Identifier: Apache-2.0
//! The pages a newcomer meets first (#188): the home page, the install
//! guide, the guide index, the README, and "What works today".
//!
//! "What works today" (`docs/guide/what-works-today.md`) is held to the
//! toolchain the way the CLI reference is held to `oskac --help`. Each row
//! of its table has a probe here: a small program that uses the feature.
//! The probe must check, run on the interpreter, and build natively
//! exactly as the row says, for aarch64 and for x86_64 Linux, and a refusal
//! must carry the code the row names. A feature that starts or stops running fails this test until the
//! page says so.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &str) -> String {
    let file = repo_root().join(path);
    std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("cannot read {}: {e}", file.display()))
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Words shaped like `E0371`.
fn codes_in(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| {
            w.len() == 5 && w.starts_with(['E', 'P']) && w[1..].chars().all(|c| c.is_ascii_digit())
        })
        .map(str::to_string)
        .collect()
}

// ---- What works today -------------------------------------------------------------

const WHAT_WORKS: &str = "docs/guide/what-works-today.md";

/// The secret the `Env` probe reads, set for every probe.
const SECRET: (&str, &str) = ("OSKA_PROBE_TOKEN", "probe");

/// How a feature is tried.
enum Probe {
    /// A program with a `main`, saved as `main.osk`.
    Program(&'static str),
    /// A package: its files, by path from the package directory.
    Package(&'static [(&'static str, &'static str)]),
    /// A file of `test` blocks.
    Tests(&'static str),
}

/// One probe per row of the table, by the row's Feature cell.
const PROBES: &[(&str, Probe)] = &[
    (
        "Core language: structs, enums, `match`, generics, closures, lists",
        Probe::Program(
            "struct Point { x: Int, y: Int }\n\n\
             enum Shape {\n    Dot(Point),\n    Line(Point, Point),\n}\n\n\
             fn width(shape: Shape) -> Int {\n    match shape {\n        Shape.Dot(_) => 0\n        Shape.Line(a, b) => b.x - a.x\n    }\n}\n\n\
             fn first<T>(items: List<T>) -> Option<T> {\n    items.get(0)\n}\n\n\
             fn main(io: Stdout) -> Result<(), IoError> {\n    \
             let shapes = [Shape.Dot(Point(x: 1, y: 2)), Shape.Line(Point(x: 0, y: 0), Point(x: 3, y: 4))]\n    \
             let widths = shapes.map(|s| width(s))\n    \
             let w = first(widths).unwrap_or(0)\n    \
             io.println(\"{w}\")\n}\n",
        ),
    ),
    (
        "Traits and `impl`",
        Probe::Program(
            "trait Area {\n    fn area(self) -> Int\n}\n\n\
             struct Square { side: Int }\n\n\
             impl Area for Square {\n    fn area(self) -> Int {\n        self.side * self.side\n    }\n}\n\n\
             fn total<T: Area>(items: List<T>) -> Int {\n    var sum = 0\n    for item in items {\n        sum = sum + item.area()\n    }\n    sum\n}\n\n\
             fn main(io: Stdout) -> Result<(), IoError> {\n    \
             let n = total([Square(side: 2), Square(side: 3)])\n    \
             io.println(\"{n}\")\n}\n",
        ),
    ),
    (
        "Contracts: `requires` and `ensures`",
        Probe::Program(
            "fn halve(n: Int) -> Int\n    requires n % 2 == 0\n    ensures |r| r * 2 == n\n{\n    n / 2\n}\n\n\
             fn main(io: Stdout) -> Result<(), IoError> {\n    let h = halve(10)\n    io.println(\"{h}\")\n}\n",
        ),
    ),
    (
        "Packages: `oska.toml` and `use pkg.…`",
        Probe::Package(&[
            (
                "oska.toml",
                "[package]\nname = \"probe\"\nversion = \"0.1.0\"\n",
            ),
            (
                "src/main.osk",
                "use pkg.greet\n\nfn main(io: Stdout) -> Result<(), IoError> {\n    io.println(greet.hello(\"world\"))\n}\n",
            ),
            (
                "src/greet.osk",
                "pub(pkg) fn hello(name: String) -> String {\n    \"Hello, {name}!\"\n}\n",
            ),
        ]),
    ),
    (
        "`test` blocks and `oskac test`",
        Probe::Tests(
            "fn double(n: Int) -> Int {\n    n * 2\n}\n\n\
             test \"doubling\" {\n    t.assert_eq(double(2), 4)\n}\n",
        ),
    ),
    (
        "`Stdout`, `Stderr` and `Args`",
        Probe::Program(
            "fn main(out: Stdout, err: Stderr, args: Args) -> Result<(), IoError> {\n    \
             let name = args.get(1).unwrap_or(\"world\")\n    \
             out.println(name.assume_trusted())?\n    \
             err.println(\"done\")\n}\n",
        ),
    ),
    (
        "`Env` and `Secret`",
        Probe::Program(
            "fn main(io: Stdout, env: Env) -> Result<(), IoError> {\n    \
             let token = env.secret(\"OSKA_PROBE_TOKEN\")\n    \
             let same = token.constant_time_eq(env.get(\"OSKA_PROBE_UNSET\").unwrap_or(\"\"))\n    \
             let shown = if same { \"same\" } else { \"different\" }\n    \
             io.println(shown)\n}\n",
        ),
    ),
    (
        "`Clock` and `Random`",
        Probe::Program(
            "fn main(io: Stdout, clock: Clock, random: Random) -> Result<(), IoError> {\n    \
             let now = clock.now_millis()\n    \
             let roll = random.next_int() % 6 + 1\n    \
             io.println(\"{now} {roll}\")\n}\n",
        ),
    ),
    (
        "`Budget`",
        Probe::Program(
            "fn main(io: Stdout, budget: Budget) -> Result<(), IoError> {\n    \
             let small = budget.limit(100, 1)\n    \
             let shown = match small.reserve(500) {\n        \
             Ok(r) => {\n            \
             r.release()\n            \
             \"reserved\"\n        \
             }\n        \
             Err(e) => e.to_string()\n    \
             }\n    \
             io.println(shown)\n}\n",
        ),
    ),
    (
        "`Approval` and `Approver`",
        Probe::Tests(
            "fn send(approval: Approval) -> Bool {\n    \
             approval.consume(\"send\", [\"to: a\"]).is_ok()\n}\n\n\
             test \"one yes, one use\" {\n    \
             t.answer_approval(true)\n    \
             match t.approver.confirm(\"send\", [\"to: a\"]) {\n        \
             Some(a) => {\n            \
             t.assert(send(a))\n            \
             t.assert(!send(a))\n        \
             }\n        \
             None => t.fail(\"refused\")\n    \
             }\n}\n",
        ),
    ),
    (
        "Files: `Fs`, `FsRead`, `FsWrite`, and the directory and file capabilities",
        Probe::Program(
            "fn main(io: Stdout, read: FsRead) -> Result<(), IoError> {\n    \
             let here = read.restrict_to(\".\")\n    \
             let names = here.list_dir(Path.new(\".\"))?\n    \
             let n = names.len()\n    \
             io.println(\"{n}\")\n}\n",
        ),
    ),
    (
        "Structured concurrency: `scope`, `spawn` and `join`",
        Probe::Program(
            "fn main(io: Stdout) -> Result<(), IoError> {\n    \
             let total = scope |s| {\n        \
             let a = s.spawn(|| 2 + 3)\n        \
             let b = s.spawn(|| 4 * 5)\n        \
             a.join() + b.join()\n    }\n    \
             io.println(\"{total}\")\n}\n",
        ),
    ),
    (
        "`Untrusted` text and `assume_trusted`",
        Probe::Program(
            "fn main(io: Stdout, env: Env) -> Result<(), IoError> {\n    \
             let name = env.get(\"OSKA_PROBE_UNSET\").unwrap_or(\"world\")\n    \
             io.println(name.assume_trusted())\n}\n",
        ),
    ),
    (
        "`std.html`",
        Probe::Program(
            "use std.html\n\n\
             fn main(io: Stdout, env: Env) -> Result<(), IoError> {\n    \
             let name = env.get(\"OSKA_PROBE_UNSET\").unwrap_or(\"<world>\")\n    \
             io.println(html.text(name).render().assume_trusted())\n}\n",
        ),
    ),
    (
        "`std.json`",
        Probe::Program(
            "use std.json\n\n\
             fn main(io: Stdout, env: Env) -> Result<(), IoError> {\n    \
             let name = env.get(\"OSKA_PROBE_UNSET\").unwrap_or(\"ada\")\n    \
             let value = json.Value.Object([json.Member(key: \"name\", value: json.Value.Text(name))])\n    \
             io.println(json.encode(value))\n}\n",
        ),
    ),
    (
        "`std.mcp`: `Toolset`, `ToolSpec` and `ToolResult`",
        Probe::Program(
            "use std.json\nuse std.mcp\n\n\
             cap struct Tools { out: Stdout }\n\n\
             impl mcp.Toolset for Tools {\n    \
             fn specs(self) -> List<mcp.ToolSpec> {\n        \
             [mcp.ToolSpec.new(\"say\", \"Say hello.\", json.Schema.object([]))]\n    }\n\n    \
             fn call(self, name: String, args: json.Value) -> mcp.ToolResult {\n        \
             match name {\n            \
             \"say\" => mcp.ToolResult.json(json.Value.Text(\"hello\"))\n            \
             _ => mcp.ToolResult.error(\"unknown tool\")\n        }\n    }\n}\n\n\
             fn main(io: Stdout) -> Result<(), IoError> {\n    \
             let tools = Tools(out: io)\n    \
             io.println(tools.call(\"say\", json.Value.Null).content())\n}\n",
        ),
    ),
    (
        "`std.mcp`: `serve` and `handle`, an MCP server over standard input and output",
        Probe::Program(
            "use std.json\nuse std.mcp\n\n\
             cap struct Tools { out: Stdout }\n\n\
             impl mcp.Toolset for Tools {\n    \
             fn specs(self) -> List<mcp.ToolSpec> {\n        \
             [mcp.ToolSpec.new(\"say\", \"Say hello.\", json.Schema.object([]))]\n    }\n\n    \
             fn call(self, name: String, args: json.Value) -> mcp.ToolResult {\n        \
             mcp.ToolResult.text(\"hello\")\n    }\n}\n\n\
             fn main(io: Stdout, stdin: Stdin) -> Result<(), IoError> {\n    \
             let info = mcp.ServerInfo.new(\"probe\", \"1.0.0\")\n    \
             let line = \"not json\"\n    \
             let step = mcp.handle(mcp.Session.new(info, Tools(out: io)), Tools(out: io), line)\n    \
             io.println(step.reply.unwrap_or(\"no reply\"))?\n    \
             match mcp.serve(stdin, io, info, Tools(out: io)) {\n        \
             Ok(()) => io.println(\"served\")\n        \
             Err(_) => io.println(\"refused\")\n    }\n}\n",
        ),
    ),
    (
        "`std.sql`",
        Probe::Program(
            "use std.sql\n\n\
             fn main(io: Stdout, net: Net, env: Env) -> Result<(), IoError> {\n    \
             let name = env.get(\"OSKA_PROBE_UNSET\").unwrap_or(\"ada\")\n    \
             match sql.connect(net.connect_only(), \"postgres://localhost/app\") {\n        \
             Ok(db) => {\n            \
             let rows = db.query(sql\"SELECT id FROM users WHERE name = ?\", [name])\n            \
             io.println(\"queried\")\n        }\n        \
             Err(_) => io.println(\"no database\")\n    }\n}\n",
        ),
    ),
    (
        "`std.proc` and `Spawn`",
        Probe::Program(
            "use std.proc\n\n\
             fn main(io: Stdout, spawn: Spawn, env: Env) -> Result<(), IoError> {\n    \
             let file = env.get(\"OSKA_PROBE_UNSET\").unwrap_or(\"notes.txt\")\n    \
             match proc.run(spawn, \"wc\", [file]) {\n        \
             Ok(_) => io.println(\"ran\")\n        \
             Err(_) => io.println(\"failed\")\n    }\n}\n",
        ),
    ),
    (
        "`Net`, `NetConnect` and `NetListen`",
        Probe::Program(
            "fn main(io: Stdout, net: Net) -> Result<(), IoError> {\n    \
             let listener = net.listen_only().restrict_to_loopback(0)\n    \
             let client = net.restrict_to(\"127.0.0.1\", listener.port()).connect()?\n    \
             let server = listener.accept()?\n    \
             client.send(\"ping\")?\n    \
             let got = server.receive()?\n    \
             io.println(got)\n}\n",
        ),
    ),
];

/// A row of the table, by column.
struct Row {
    feature: String,
    checks: String,
    interpreter: String,
    native: String,
    platforms: String,
}

/// The rows of the page's feature table: the table whose header starts
/// `| Feature |`.
fn table_rows() -> Vec<Row> {
    let page = read(WHAT_WORKS);
    let mut lines = page.lines().skip_while(|l| !l.starts_with("| Feature |"));
    let header = lines
        .next()
        .unwrap_or_else(|| panic!("{WHAT_WORKS} has no `| Feature |` table"));
    assert_eq!(
        cells(header),
        ["Feature", "Checks", "Interpreter", "Native", "Platforms"],
        "{WHAT_WORKS}: the table's columns"
    );
    lines
        .skip(1)
        .take_while(|l| l.starts_with('|'))
        .map(|line| {
            let c = cells(line);
            assert_eq!(c.len(), 5, "{WHAT_WORKS}: a row needs five cells: {line}");
            Row {
                feature: c[0].clone(),
                checks: c[1].clone(),
                interpreter: c[2].clone(),
                native: c[3].clone(),
                platforms: c[4].clone(),
            }
        })
        .collect()
}

/// The cells of a table row, trimmed.
fn cells(line: &str) -> Vec<String> {
    line.trim()
        .trim_start_matches('|')
        .trim_end_matches('|')
        .split('|')
        .map(|c| c.trim().to_string())
        .collect()
}

/// What a cell claims: `Yes`, or `No` with the codes of the refusal, as in
/// "No (`E0371`)".
fn claim(cell: &str, at: &str) -> Option<Vec<String>> {
    if cell == "Yes" {
        return None;
    }
    assert!(
        cell.starts_with("No"),
        "{at}: a cell is `Yes` or `No (code)`, not {cell:?}"
    );
    let codes = codes_in(cell);
    assert!(!codes.is_empty(), "{at}: a `No` names the refusal's code");
    Some(codes)
}

/// The platforms `oskac build` writes executables for, by the name `--target`
/// takes. A native cell holds only if the probe builds for every one.
const TARGETS: [&str; 5] = [
    "linux-aarch64",
    "linux-x86_64",
    "macos-aarch64",
    "macos-x86_64",
    "windows-x86_64",
];

/// What the Platforms cell says, from the backends that run the feature:
/// whether the interpreter runs it, and for which of [`TARGETS`] the probe
/// built. A cell names a platform only if the probe built for it.
fn platforms(interpreter: bool, built: [bool; 5]) -> String {
    if !interpreter {
        return "None yet: checks only".to_string();
    }
    if built.iter().all(|b| *b) {
        return "Every host; native on Linux, macOS and Windows".to_string();
    }
    let native: Vec<&str> = TARGETS
        .iter()
        .zip(built)
        .filter(|(_, b)| *b)
        .map(|(t, _)| *t)
        .collect();
    if native.is_empty() {
        "Every host, on the interpreter".to_string()
    } else {
        format!("Every host; native on {}", native.join(", "))
    }
}

/// A scratch directory holding `probe`, and the path to give `oskac`.
fn lay_out(feature: &str, probe: &Probe) -> (PathBuf, String) {
    let name: String = feature
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(40)
        .collect();
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("works-today-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("should create the scratch directory");
    let write = |path: &str, source: &str| {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().expect("a parent")).expect("should create it");
        std::fs::write(file, source).expect("should write the probe");
    };
    let target = match probe {
        Probe::Program(source) | Probe::Tests(source) => {
            write("main.osk", source);
            "main.osk".to_string()
        }
        Probe::Package(files) => {
            for (path, source) in *files {
                write(&format!("app/{path}"), source);
            }
            "app".to_string()
        }
    };
    (dir, target)
}

fn oskac(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .env(SECRET.0, SECRET.1)
        .current_dir(dir)
        .output()
        .expect("oskac should run")
}

/// Whether `output` bears out `claim`: success for `Yes`, and for `No` the
/// "cannot run yet" exit code, 2, with every code named.
fn bears_out(output: &Output, claim: &Option<Vec<String>>) -> Result<(), String> {
    let stderr = text(&output.stderr);
    let code = output.status.code();
    match claim {
        None if code == Some(0) => Ok(()),
        None => Err(format!("it said Yes, but exit {code:?}: {stderr}")),
        Some(codes) if code == Some(2) && codes.iter().all(|c| stderr.contains(c.as_str())) => {
            Ok(())
        }
        Some(codes) => Err(format!(
            "it said No ({codes:?}), but exit {code:?}: {stderr}"
        )),
    }
}

/// Whether this host runs `oskac run` and `oskac test` natively: Linux and
/// macOS on aarch64 or x86_64, and Windows on x86_64.
fn native_host() -> bool {
    cfg!(any(
        all(
            any(target_os = "linux", target_os = "macos"),
            any(target_arch = "aarch64", target_arch = "x86_64")
        ),
        all(windows, target_arch = "x86_64")
    ))
}

#[test]
fn every_row_of_what_works_today_holds_on_the_toolchain() {
    let rows = table_rows();
    let mut problems = Vec::new();
    for row in &rows {
        let at = format!("{WHAT_WORKS}: {}", row.feature);
        let probe = PROBES
            .iter()
            .find(|(feature, _)| *feature == row.feature)
            .map(|(_, probe)| probe)
            .unwrap_or_else(|| {
                panic!("{at}: no probe in crates/oskac/tests/front_door.rs tries this feature")
            });
        let (dir, target) = lay_out(&row.feature, probe);
        let checks = oskac(&dir, &["check", &target]);
        if let Err(e) = bears_out(&checks, &claim(&row.checks, &at)) {
            problems.push(format!("{at}: Checks: {e}"));
        }
        let interpreter = claim(&row.interpreter, &at);
        let native = claim(&row.native, &at);
        let ran = match probe {
            Probe::Tests(_) => oskac(&dir, &["test", "--interpret", &target]),
            _ => oskac(&dir, &["run", "--interpret", &target]),
        };
        if let Err(e) = bears_out(&ran, &interpreter) {
            problems.push(format!("{at}: Interpreter: {e}"));
        }
        // `oskac build` compiles for each of `TARGETS` on every host, and
        // the platforms the probe really built for decide the Platforms
        // cell. A test file is compiled natively only by `oskac test`, on
        // the hosts that run natively, where a fallback to the interpreter
        // says so with `E0390`; it cannot be built, so its native cell
        // stands for both targets.
        let mut built = [native.is_none(); 5];
        match probe {
            Probe::Tests(_) if native_host() => {
                let output = oskac(&dir, &["test", &target]);
                let fell_back = text(&output.stderr).contains("E0390");
                let result = if fell_back && native.is_none() {
                    Err(format!("it said Yes, but: {}", text(&output.stderr)))
                } else {
                    bears_out(&output, &native)
                };
                if let Err(e) = result {
                    problems.push(format!("{at}: Native: {e}"));
                }
            }
            Probe::Tests(_) => {}
            _ => {
                for (i, platform) in TARGETS.iter().enumerate() {
                    let out = format!("probe-bin-{platform}");
                    let output = oskac(&dir, &["build", &target, "--target", platform, "-o", &out]);
                    built[i] = output.status.code() == Some(0);
                    if let Err(e) = bears_out(&output, &native) {
                        problems.push(format!("{at}: Native ({platform}): {e}"));
                    }
                }
            }
        }
        let expected = platforms(interpreter.is_none(), built);
        if row.platforms != expected {
            problems.push(format!(
                "{at}: Platforms says {:?}; from the backends it is {expected:?}",
                row.platforms
            ));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}

#[test]
fn every_probe_is_a_row_of_the_table() {
    let rows = table_rows();
    for (feature, _) in PROBES {
        assert!(
            rows.iter().any(|row| row.feature == *feature),
            "{WHAT_WORKS} has no row for {feature:?}, which front_door.rs probes"
        );
    }
}

#[test]
fn the_table_says_which_sinks_and_capabilities_check_but_do_not_run() {
    let rows = table_rows();
    for feature in ["`std.sql`", "`std.proc` and `Spawn`"] {
        let row = rows
            .iter()
            .find(|row| row.feature == feature)
            .unwrap_or_else(|| panic!("{WHAT_WORKS} has no row for {feature}"));
        assert_eq!(row.checks, "Yes", "{feature}");
        assert!(row.interpreter.starts_with("No"), "{feature}");
    }
}

// ---- The home page, the README, the install guide and the guide index -----------

#[test]
fn the_home_page_and_the_readme_link_what_works_today_and_claim_no_logging_api() {
    for (page, link) in [
        ("docs/index.md", "guide/what-works-today.md"),
        ("README.md", "docs/guide/what-works-today.md"),
    ] {
        let text = read(page);
        assert!(
            text.contains(&format!("]({link})")),
            "{page} should link {link}"
        );
        // There is no logging module (docs/checker.md lists the std
        // modules), so no front page may promise one.
        assert!(
            !text.to_lowercase().contains("logging"),
            "{page} mentions a logging API, which does not exist"
        );
    }
}

#[test]
fn the_home_page_itself_says_which_features_check_but_do_not_run() {
    let home = read("docs/index.md");
    let sentence = home
        .split(". ")
        .find(|s| s.contains("check but do not run"))
        .unwrap_or_else(|| panic!("docs/index.md should say what checks but does not run"));
    // Every feature the table says checks but does not run is named,
    // by the first code span of its row, so the home page cannot fall
    // behind the table.
    let mut named = 0;
    for row in table_rows()
        .iter()
        .filter(|row| row.checks == "Yes" && row.interpreter.starts_with("No"))
    {
        let name = row
            .feature
            .split('`')
            .nth(1)
            .unwrap_or_else(|| panic!("{:?} has no code span", row.feature));
        assert!(
            sentence.contains(&format!("`{name}`")),
            "docs/index.md does not say that `{name}` checks but does not run: {sentence}"
        );
        named += 1;
    }
    assert!(
        named >= 2,
        "the table lists {named} features that do not run"
    );
    assert!(
        home.contains("](guide/what-works-today.md)"),
        "docs/index.md should link the table"
    );
}

/// The `## ` headings of a page, in order.
fn sections(page: &str) -> Vec<String> {
    let mut fenced = false;
    let mut out = Vec::new();
    for line in read(page).lines() {
        if line.starts_with("```") {
            fenced = !fenced;
        } else if !fenced {
            if let Some(title) = line.strip_prefix("## ") {
                out.push(title.trim().to_string());
            }
        }
    }
    out
}

#[test]
fn the_install_guide_puts_the_installers_first_and_building_from_source_last() {
    let headings = sections("docs/guide/install.md");
    assert_eq!(
        headings.first().map(String::as_str),
        Some("One-line installers"),
        "{headings:?}"
    );
    assert_eq!(
        headings.last().map(String::as_str),
        Some("Build from source"),
        "{headings:?}"
    );
}

/// The error codes a script stops with: the word after each `marker`.
fn error_codes(script: &str, marker: &str) -> std::collections::BTreeSet<String> {
    read(script)
        .split(marker)
        .skip(1)
        .map(|rest| {
            rest.trim_start_matches('\'')
                .chars()
                .take_while(|c| c.is_ascii_lowercase() || *c == '_')
                .collect::<String>()
        })
        .filter(|code| !code.is_empty())
        .collect()
}

#[test]
fn the_install_guide_tables_every_error_code_the_installers_have() {
    let guide = read("docs/guide/install.md");
    let mut codes = error_codes("scripts/install/install.sh", "fail ");
    codes.extend(error_codes("scripts/install/install.ps1", "Stop-Install '"));
    assert!(codes.len() >= 17, "{codes:?}");
    for code in codes {
        assert!(
            guide.contains(&format!("\n| `{code}` |")),
            "the install guide's table has no row for {code}"
        );
    }
}

/// The release a page names as the latest: `vX.Y.Z` after `marker`.
fn release_after(page: &str, marker: &str) -> String {
    let text = read(page);
    let at = text
        .find(marker)
        .unwrap_or_else(|| panic!("{page} has no {marker:?}"));
    let rest = &text[at + marker.len()..];
    let start = rest.find('v').expect("a version");
    rest[start..]
        .chars()
        .take_while(|c| *c == 'v' || *c == '.' || c.is_ascii_digit())
        .collect::<String>()
        .trim_end_matches('.')
        .to_string()
}

#[test]
fn the_guide_index_names_the_release_the_install_guide_names() {
    let marker = "The latest release is ";
    let install = release_after("docs/guide/install.md", marker);
    let index = release_after("docs/guide/index.md", marker);
    assert!(install.starts_with("v0."), "{install}");
    assert_eq!(index, install, "docs/guide/index.md names another release");
    assert!(!read("docs/guide/index.md").contains("or later"));
}
