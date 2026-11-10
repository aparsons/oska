//! Conformance of the task guides in `docs/guide` (spec #70, ticket #73).
//!
//! Every fenced block in a guide is checked against the real toolchain, so
//! a guide cannot drift from `oskac`:
//!
//! - ```` ```oska ```` is a complete program: `oskac check` reports no
//!   diagnostic.
//! - ```` ```oska fragment ```` is explicitly a fragment, such as a module
//!   that uses types it never declares: it only has to parse.
//! - ```` ```oska error=E0300 ```` is a complete program that `oskac check`
//!   refuses with exactly that code.
//! - ```` ```oska tests ```` is a complete program whose `test` blocks all
//!   pass under `oskac test`.
//! - ```` ```console cmd=run ```` directly after an `oska` block is the exact
//!   stdout of `oskac run` on it. `cmd=check` and `cmd=test` run those
//!   commands instead. Extra words change the run: `stream=stderr` shows
//!   stderr, `json` adds `--format=json`, `release` adds `--release`,
//!   `integration` adds `--integration`, and `-- a b` passes arguments to the program. The
//!   exit code is not asserted.
//! - ```` ```help ```` is the exact stdout of `oskac --help`.
//! - ```` ```sh ```` blocks may use only `oskac` commands and flags that
//!   `oskac --help` lists. A block that documents something not built yet
//!   is ```` ```sh planned ````, which also needs a line starting
//!   `> **Available from v0.1.0.**` on the same page.
//!
//! An `oska` block may add `file=name.osk`; the program is saved under that
//! name (default `main.osk`), because it appears in the toolchain's output.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The guides the ticket asks for, relative to `docs/guide`.
const REQUIRED_PAGES: [&str; 11] = [
    "index.md",
    "install.md",
    "quickstart.md",
    "language-tour.md",
    "capabilities.md",
    "untrusted-data.md",
    "testing.md",
    "debugging.md",
    "troubleshooting.md",
    "cli-reference.md",
    "agent-guide.md",
];

/// The marker a page needs before it documents a `planned` block.
const PLANNED_MARKER: &str = "> **Available from v0.1.0.**";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// Every `.md` file under `dir`, sorted.
fn markdown_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("cannot list {dir:?}: {e}")) {
        let path = entry.expect("should read a directory entry").path();
        if path.is_dir() {
            out.extend(markdown_files(&path));
        } else if path.extension().is_some_and(|e| e == "md") {
            out.push(path);
        }
    }
    out.sort();
    out
}

fn guide_pages() -> Vec<PathBuf> {
    markdown_files(&repo_root().join("docs/guide"))
}

/// A fenced code block.
struct Block {
    page: PathBuf,
    /// 1-based line of the opening fence.
    line: usize,
    /// The info string after the fence, split on spaces.
    info: Vec<String>,
    body: String,
}

impl Block {
    fn at(&self) -> String {
        format!("{}:{}", self.page.display(), self.line)
    }

    fn language(&self) -> &str {
        self.info.first().map_or("", String::as_str)
    }

    fn has(&self, word: &str) -> bool {
        self.info.iter().skip(1).any(|w| w == word)
    }

    fn option(&self, key: &str) -> Option<&str> {
        let prefix = format!("{key}=");
        self.info
            .iter()
            .skip(1)
            .find_map(|w| w.strip_prefix(&prefix))
    }

    /// The words after `--` in the info string: program arguments.
    fn program_args(&self) -> Vec<String> {
        match self.info.iter().position(|w| w == "--") {
            Some(at) => self.info[at + 1..].to_vec(),
            None => Vec::new(),
        }
    }
}

/// The fenced blocks of a page. Fences must start in column 0, so that no
/// block is silently skipped.
fn blocks_of(page: &Path) -> Vec<Block> {
    let text = read(page);
    let mut blocks = Vec::new();
    let mut open: Option<Block> = None;
    for (index, line) in text.lines().enumerate() {
        let number = index + 1;
        if line.trim_start().starts_with("```") && !line.starts_with("```") {
            panic!(
                "{}:{number}: a code fence must start in column 0",
                page.display()
            );
        }
        match (&mut open, line.strip_prefix("```")) {
            (None, Some(info)) => {
                open = Some(Block {
                    page: page.to_path_buf(),
                    line: number,
                    info: info.split_whitespace().map(str::to_string).collect(),
                    body: String::new(),
                });
            }
            (Some(_), Some(rest)) if rest.trim().is_empty() => {
                blocks.extend(open.take());
            }
            (Some(block), _) => {
                block.body.push_str(line);
                block.body.push('\n');
            }
            (None, None) => {}
        }
    }
    assert!(
        open.is_none(),
        "{}: a code block is never closed",
        page.display()
    );
    blocks
}

fn all_blocks() -> Vec<Block> {
    guide_pages().iter().flat_map(|p| blocks_of(p)).collect()
}

// ---- Running the toolchain ---------------------------------------------------

/// Run `oskac args...` in a scratch directory that holds `source` as
/// `file`, so that paths in its output are the same on every machine.
fn oskac_on(block: &Block, source: &str, args: &[&str], tail: &[String]) -> Output {
    let file = block.option("file").unwrap_or("main.osk");
    // One directory per test thread, block, and page, so that tests running
    // in parallel never write the same file.
    let page: String = block
        .page
        .strip_prefix(repo_root().join("docs/guide"))
        .unwrap_or(&block.page)
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let thread = format!("{:?}", std::thread::current().id()).replace(['(', ')'], "");
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("guide-{thread}-{page}-{}", block.line));
    std::fs::create_dir_all(&dir).expect("should create the scratch directory");
    std::fs::write(dir.join(file), source).expect("should write the program");
    let mut command = Command::new(env!("CARGO_BIN_EXE_oskac"));
    command.args(args).arg(file);
    if !tail.is_empty() {
        command.arg("--").args(tail);
    }
    command
        .current_dir(&dir)
        .output()
        .expect("oskac should run")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("output should be UTF-8")
}

/// The diagnostic codes in a `--format=json` report.
fn codes_in(json: &str) -> BTreeSet<String> {
    json.split("\"code\":\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .map(str::to_string)
        .collect()
}

fn oskac_help() -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .arg("--help")
        .output()
        .expect("oskac should run");
    assert!(output.status.success());
    text(&output.stdout)
}

// ---- Tests -------------------------------------------------------------------

#[test]
fn every_required_guide_page_exists() {
    let dir = repo_root().join("docs/guide");
    for page in REQUIRED_PAGES {
        assert!(dir.join(page).is_file(), "docs/guide/{page} is missing");
    }
}

/// Why `block` does not meet its promise, if it does not.
fn oska_block_problem(block: &Block) -> Option<String> {
    if block.has("fragment") {
        let output = oskac_on(block, &block.body, &["parse"], &[]);
        return (!output.status.success()).then(|| {
            format!(
                "a fragment must still parse:\n{}{}",
                text(&output.stdout),
                text(&output.stderr)
            )
        });
    }
    if let Some(code) = block.option("error") {
        let output = oskac_on(block, &block.body, &["check", "--format=json"], &[]);
        let found = codes_in(&text(&output.stdout));
        return (found != BTreeSet::from([code.to_string()]))
            .then(|| format!("expected exactly {code}, found {found:?}"));
    }
    if block.has("tests") {
        let output = oskac_on(block, &block.body, &["test"], &[]);
        return (!output.status.success()).then(|| {
            format!(
                "its tests should pass:\n{}{}",
                text(&output.stdout),
                text(&output.stderr)
            )
        });
    }
    let output = oskac_on(block, &block.body, &["check"], &[]);
    (!output.status.success()).then(|| {
        format!(
            "a complete program should check cleanly (mark it `fragment` otherwise):\n{}",
            text(&output.stderr)
        )
    })
}

#[test]
fn every_oska_block_parses_and_checks_or_is_marked_a_fragment() {
    let blocks = all_blocks();
    let programs: Vec<&Block> = blocks.iter().filter(|b| b.language() == "oska").collect();
    assert!(
        programs.len() >= 10,
        "expected many `oska` blocks, found {}",
        programs.len()
    );
    let problems: Vec<String> = programs
        .iter()
        .filter_map(|b| oska_block_problem(b).map(|p| format!("{}: {p}", b.at())))
        .collect();
    assert!(problems.is_empty(), "\n{}", problems.join("\n\n"));
}

#[test]
fn every_console_and_help_block_shows_exactly_what_the_toolchain_prints() {
    let help = oskac_help();
    let mut checked = 0;
    for page in guide_pages() {
        let blocks = blocks_of(&page);
        let mut last_program: Option<&Block> = None;
        for block in &blocks {
            let at = block.at();
            let actual = match block.language() {
                "oska" => {
                    last_program = Some(block);
                    continue;
                }
                "help" => help.clone(),
                "console" => {
                    let program = last_program.unwrap_or_else(|| {
                        panic!("{at}: a `console` block must follow an `oska` block")
                    });
                    let command = block.option("cmd").unwrap_or_else(|| {
                        panic!("{at}: a `console` block needs `cmd=run|check|test`")
                    });
                    assert!(
                        ["run", "check", "test"].contains(&command),
                        "{at}: unknown cmd={command}"
                    );
                    let mut args = vec![command];
                    if block.has("json") {
                        args.push("--format=json");
                    }
                    if block.has("release") {
                        args.push("--release");
                    }
                    if block.has("integration") {
                        args.push("--integration");
                    }
                    let output = oskac_on(program, &program.body, &args, &block.program_args());
                    match block.option("stream") {
                        None | Some("stdout") => text(&output.stdout),
                        Some("stderr") => text(&output.stderr),
                        Some(other) => panic!("{at}: unknown stream={other}"),
                    }
                }
                _ => continue,
            };
            checked += 1;
            assert_eq!(
                block.body.trim_end(),
                actual.trim_end(),
                "{at}: the block should show exactly what the toolchain prints"
            );
        }
    }
    assert!(checked >= 5, "expected output blocks, found {checked}");
}

#[test]
fn shell_blocks_use_only_oskac_commands_and_flags_that_exist() {
    let help = oskac_help();
    let commands: BTreeSet<&str> = ["lex", "parse", "check", "run", "test", "explain"].into();
    for block in all_blocks().iter().filter(|b| b.language() == "sh") {
        if block.has("planned") {
            continue;
        }
        for line in block.body.lines() {
            let line = line.trim().trim_start_matches("$ ");
            let Some(rest) = line.strip_prefix("oskac ") else {
                continue;
            };
            let rest = rest.split('#').next().unwrap_or("");
            let mut words = rest.split_whitespace();
            let first = words.next().unwrap_or("");
            assert!(
                commands.contains(first) || ["--version", "--help"].contains(&first),
                "{}: `oskac {first}` is not an oskac command (mark the block `planned` if it is not built yet)",
                block.at()
            );
            // Arguments after `--` belong to the program being run.
            for word in words.take_while(|w| *w != "--") {
                if word.starts_with("--") {
                    let flag = word.split('=').next().unwrap_or(word);
                    assert!(
                        help.contains(flag),
                        "{}: `{flag}` is not a flag in `oskac --help`",
                        block.at()
                    );
                }
            }
        }
    }
}

#[test]
fn planned_blocks_sit_on_a_page_with_the_version_marker() {
    for page in guide_pages() {
        let planned = blocks_of(&page).iter().any(|b| b.has("planned"));
        if planned {
            assert!(
                read(&page).lines().any(|l| l.starts_with(PLANNED_MARKER)),
                "{}: a `planned` block needs a line starting `{PLANNED_MARKER}`",
                page.display()
            );
        }
    }
}

#[test]
fn the_cli_reference_matches_oskac_help() {
    let help = oskac_help();
    let reference = read(&repo_root().join("docs/guide/cli-reference.md"));

    // Every command in the usage text has a heading, and the other way round.
    let in_help: BTreeSet<String> = help
        .lines()
        .filter_map(|l| l.trim().strip_prefix("oskac "))
        .filter(|l| l.starts_with(|c: char| c.is_ascii_alphabetic()))
        .filter_map(|l| l.split_whitespace().next())
        .map(str::to_string)
        .collect();
    let headings: BTreeSet<String> = reference
        .lines()
        .filter_map(|l| l.strip_prefix("## `oskac "))
        .filter_map(|l| l.split(['`', ' ']).next())
        .filter(|command| !command.starts_with("--"))
        .map(str::to_string)
        .collect();
    assert_eq!(
        headings, in_help,
        "the CLI reference needs one `## `oskac <command>`` heading per command in `oskac --help`"
    );

    // Every flag in the usage text has a row in the flag table, and the
    // other way round. A flag row starts `| `--flag`` in the first column.
    let flags_in = |text: &str| -> BTreeSet<String> {
        text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '='))
            .filter(|w| w.starts_with("--") && w.len() > 2)
            .map(|w| w.split('=').next().unwrap_or(w).to_string())
            .collect()
    };
    let help_flags: BTreeSet<String> = flags_in(&help)
        .into_iter()
        .filter(|f| f != "--help" && f != "--version")
        .collect();
    let documented: BTreeSet<String> = reference
        .lines()
        .filter_map(|l| l.strip_prefix("| `--"))
        .filter_map(|l| l.split(['`', '=']).next())
        .map(|f| format!("--{f}"))
        .filter(|f| f != "--help" && f != "--version")
        .collect();
    assert_eq!(
        documented, help_flags,
        "the CLI reference flag table should list exactly the flags of `oskac --help`"
    );
}

#[test]
fn every_example_has_a_page_that_shows_its_exact_source() {
    let root = repo_root();
    let mut examples: Vec<PathBuf> = std::fs::read_dir(root.join("examples"))
        .expect("examples/ should exist")
        .map(|e| e.expect("examples/ entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "osk"))
        .collect();
    examples.sort();
    assert!(!examples.is_empty());
    for example in examples {
        let name = example
            .file_stem()
            .expect("an example has a name")
            .to_string_lossy()
            .into_owned();
        let page = root.join(format!("docs/guide/examples/{name}.md"));
        assert!(
            page.is_file(),
            "examples/{name}.osk needs a page at docs/guide/examples/{name}.md"
        );
        let blocks = blocks_of(&page);
        let source = blocks
            .iter()
            .find(|b| b.language() == "oska")
            .unwrap_or_else(|| panic!("{} should show the example's source", page.display()));
        assert_eq!(
            source.body,
            read(&example),
            "{} should show examples/{name}.osk exactly",
            page.display()
        );
        assert!(
            blocks
                .iter()
                .any(|b| b.language() == "console" && b.option("cmd") == Some("check")),
            "{} should show the `oskac check` authority report",
            page.display()
        );
    }
}

#[test]
fn every_diagnostic_code_a_guide_mentions_is_documented() {
    let root = repo_root();
    let mut known = BTreeSet::new();
    // The parser and lexer list their codes in their own module docs, which
    // `docs/ast.md` points to as the full list.
    for doc in [
        "docs/checker.md",
        "docs/runtime.md",
        "docs/ast.md",
        "crates/oska-syntax/src/parser.rs",
        "crates/oska-syntax/src/lib.rs",
    ] {
        known.extend(codes_in_text(&read(&root.join(doc))));
    }
    for page in guide_pages() {
        for code in codes_in_text(&read(&page)) {
            assert!(
                known.contains(&code),
                "{}: `{code}` is not a documented diagnostic code",
                page.display()
            );
        }
    }
}

/// Words shaped like `E0300` or `P0004`.
fn codes_in_text(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| {
            w.len() == 5 && w.starts_with(['E', 'P']) && w[1..].chars().all(|c| c.is_ascii_digit())
        })
        .map(str::to_string)
        .collect()
}

#[test]
fn relative_links_between_pages_resolve() {
    for page in guide_pages() {
        let text = read(&page);
        let dir = page.parent().expect("a page has a directory");
        for target in text.split("](").skip(1).filter_map(|r| r.split(')').next()) {
            if target.contains("://") || target.starts_with('#') || target.starts_with("mailto:") {
                continue;
            }
            let file = target.split('#').next().unwrap_or(target);
            assert!(
                dir.join(file).exists(),
                "{}: the link `{target}` points nowhere",
                page.display()
            );
        }
    }
}
