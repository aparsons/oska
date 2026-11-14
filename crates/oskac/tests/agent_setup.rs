//! The agent-setup prompt (spec #70, ticket #80): the prompt a person pastes
//! into a coding agent, the `AGENTS.md` and `CLAUDE.md` templates, and the
//! opt-in Claude Code hook, all in `site/agent-setup/`.
//!
//! They tell an agent to run real commands, so these tests hold them to the
//! real tools. Every installer flag, `oskac` command and flag, and installer
//! JSON field they mention must exist; every `oskalang.dev` page they link
//! must be one the site publishes; the example program must check and run;
//! the hook's settings must be valid JSON that asks for no permission; and
//! the hook script must do what it says. The rules the prompt makes for
//! the agent (no pipe to a shell but the installer, no secrets, no unseen
//! changes) are held in place too.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use common::Json;
use oska_run::FakeHost;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

fn setup_file(name: &str) -> String {
    read(&repo_root().join("site/agent-setup").join(name))
}

/// The files an agent reads and is told to run or copy, by name.
const AGENT_FILES: [&str; 4] = [
    "prompt.md",
    "AGENTS.template.md",
    "CLAUDE.template.md",
    "oska-check.sh",
];

fn oskac(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .output()
        .expect("oskac should run")
}

fn stdout_of(args: &[&str]) -> String {
    let output = oskac(args);
    assert_eq!(output.status.code(), Some(0), "oskac {args:?}");
    String::from_utf8(output.stdout).expect("UTF-8")
}

// ---- Flags, commands and fields exist --------------------------------------------------

/// The `--flag` names in `text`, without any `=value`.
fn flags_in(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 2 < bytes.len() {
        let starts = i == 0 || !bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'-';
        if starts && bytes[i] == b'-' && bytes[i + 1] == b'-' && bytes[i + 2].is_ascii_lowercase() {
            let end = text[i + 2..]
                .find(|c: char| !(c.is_ascii_lowercase() || c == '-'))
                .map_or(text.len(), |n| i + 2 + n);
            found.insert(text[i..end].to_string());
            i = end;
        } else {
            i += 1;
        }
    }
    found
}

/// The flags `oskac --help` lists.
fn oskac_flags() -> BTreeSet<String> {
    flags_in(&stdout_of(&["--help"]))
}

/// What `install.sh --help` prints.
fn installer_help() -> String {
    let output = Command::new("sh")
        .arg(repo_root().join("scripts/install/install.sh"))
        .arg("--help")
        .output()
        .expect("sh should run");
    assert_eq!(output.status.code(), Some(0));
    String::from_utf8(output.stdout).expect("UTF-8")
}

#[test]
fn every_flag_the_agent_files_mention_exists_in_the_tool_they_are_for() {
    let oskac_flags = oskac_flags();
    let installer_flags = flags_in(&installer_help());
    let ps1 = read(&repo_root().join("scripts/install/install.ps1"));
    assert!(oskac_flags.contains("--format") && installer_flags.contains("--json"));
    let mut checked = BTreeSet::new();
    for name in AGENT_FILES {
        for line in setup_file(name).lines() {
            for flag in flags_in(line) {
                // A line that runs the installer uses its flags, one that
                // runs `oskac` uses `oskac`'s; any other line, either's.
                let ok = if line.contains("install.") {
                    installer_flags.contains(&flag) && ps1.contains(&flag)
                } else if line.contains("oskac") {
                    oskac_flags.contains(&flag)
                } else {
                    oskac_flags.contains(&flag) || installer_flags.contains(&flag)
                };
                assert!(ok, "{name} mentions {flag}, which the tool lacks:\n{line}");
                checked.insert(flag);
            }
        }
    }
    for flag in ["--json", "--version", "--format"] {
        assert!(checked.contains(flag), "the files should use {flag}");
    }
}

/// The text inside the code spans and code fences of `markdown`, line by
/// line.
fn code_lines(markdown: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut fenced = false;
    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
        } else if fenced {
            out.push(line.to_string());
        } else {
            out.extend(line.split('`').skip(1).step_by(2).map(str::to_string));
        }
    }
    out
}

/// The commands `oskac --help` lists.
fn oskac_commands() -> BTreeSet<String> {
    stdout_of(&["--help"])
        .lines()
        .filter_map(|l| l.strip_prefix("    oskac "))
        .filter_map(|l| l.split_whitespace().next())
        .filter(|w| !w.starts_with('-'))
        .map(str::to_string)
        .collect()
}

#[test]
fn every_oskac_command_the_agent_files_mention_exists() {
    let commands = oskac_commands();
    assert!(commands.contains("check") && commands.contains("explain"));
    let mut seen = BTreeSet::new();
    for name in AGENT_FILES {
        for code in code_lines(&setup_file(name)) {
            let words: Vec<&str> = code.split_whitespace().collect();
            for (at, word) in words.iter().enumerate() {
                if *word != "oskac" && !word.ends_with("/oskac") {
                    continue;
                }
                let Some(next) = words.get(at + 1) else {
                    continue;
                };
                if next.starts_with('-') {
                    continue;
                }
                assert!(
                    commands.contains(*next),
                    "{name}: `oskac {next}` is not a command: {commands:?}\n{code}"
                );
                seen.insert(next.to_string());
            }
        }
    }
    for command in ["check", "run", "test", "explain"] {
        assert!(
            seen.contains(command),
            "the files should use oskac {command}"
        );
    }
}

#[test]
fn the_installer_fields_the_prompt_reads_are_fields_the_installers_print() {
    let prompt = setup_file("prompt.md");
    let sh = read(&repo_root().join("scripts/install/install.sh"));
    let ps1 = read(&repo_root().join("scripts/install/install.ps1"));
    for field in [
        "ok",
        "error",
        "message",
        "checksum_verified",
        "signature_verified",
        "version",
        "path",
        "bin_dir",
        "on_path",
    ] {
        assert!(
            prompt.contains(&format!("`{field}`")),
            "the prompt reads `{field}`"
        );
        assert!(sh.contains(field), "install.sh prints {field}");
        assert!(ps1.contains(field), "install.ps1 prints {field}");
    }
}

// ---- What the prompt tells the agent -----------------------------------------------------

#[test]
fn the_prompt_has_the_agent_install_verify_configure_test_and_report() {
    let prompt = setup_file("prompt.md");
    for step in [
        "curl -fsSL https://oskalang.dev/install.sh | sh -s -- --json",
        "install.ps1))) --json",
        "`checksum_verified` and `signature_verified` are not both `true`, stop",
        "oskac --version --format=json",
        "`oskac check hello.osk`",
        "`oskac run hello.osk`",
        "AGENTS.md",
        "CLAUDE.md",
        "`oskac test <file.osk>`",
        "run the installer again",
        "https://oskalang.dev/llms.txt",
    ] {
        assert!(prompt.contains(step), "the prompt should say: {step}");
    }
    // Each step is a heading, in order.
    let headings: Vec<&str> = prompt.lines().filter(|l| l.starts_with("## ")).collect();
    assert_eq!(
        headings,
        [
            "## Rules for this task",
            "## 1. Install",
            "## 2. Check the toolchain",
            "## 3. Configure the project",
            "## 4. Run the tests in CI",
            "## 5. Report",
        ]
    );
}

#[test]
fn the_prompt_makes_the_agent_safe_to_follow() {
    let prompt = setup_file("prompt.md");
    for rule in [
        "Say what you are about to run before you run it.",
        "Pipe nothing to a shell except the official installer from oskalang.dev",
        "Never ask for, read, print, or handle secrets",
        "Do not change an existing file until you have shown me the diff and I have agreed.",
        "Do not change any permission or approval setting, and do not touch user-level or global settings.",
        "Never skip a verification, and never work around a failed one.",
        "Ask me whether I want it, and do nothing if I do not say yes.",
        "Never edit `~/.claude/settings.json`.",
    ] {
        assert!(prompt.contains(rule), "the prompt should say: {rule}");
    }
}

#[test]
fn nothing_is_piped_to_a_shell_but_the_official_installer() {
    for name in AGENT_FILES {
        for line in setup_file(name).lines() {
            let mut rest = line;
            while let Some(at) = rest.find('|') {
                let after = rest[at + 1..].trim_start();
                let piped = [
                    "sh", "bash", "zsh", "iex", "python", "node", "source", "eval",
                ]
                .iter()
                .any(|shell| {
                    after.starts_with(shell)
                        && !after[shell.len()..].starts_with(|c: char| c.is_alphanumeric())
                });
                if piped {
                    assert!(
                        line.contains("https://oskalang.dev/install."),
                        "{name} pipes to a shell something that is not the installer:\n{line}"
                    );
                }
                rest = &rest[at + 1..];
            }
        }
    }
}

#[test]
fn the_agent_files_name_no_host_but_oskalang_dev_and_no_privileged_command() {
    for name in AGENT_FILES {
        let text = setup_file(name);
        let mut rest = text.as_str();
        while let Some(at) = rest.find("http") {
            let url = &rest[at..];
            let end = url
                .find(|c: char| c.is_whitespace() || "`)>\"'".contains(c))
                .unwrap_or(url.len());
            let url = url[..end].trim_end_matches(['.', ',', ':', ';']);
            if url.starts_with("https://") || url.starts_with("http://") {
                assert!(
                    url.starts_with("https://oskalang.dev/") || url == "https://oskalang.dev",
                    "{name} links {url}, which is not on oskalang.dev"
                );
            }
            rest = &rest[at + 4..];
        }
        for word in [
            "sudo",
            "chmod",
            "--dangerously",
            "bypassPermissions",
            "acceptEdits",
            "--yes",
        ] {
            assert!(!text.contains(word), "{name} contains {word}");
        }
    }
}

#[test]
fn the_example_program_in_the_prompt_checks_and_runs() {
    let prompt = setup_file("prompt.md");
    let mut programs = Vec::new();
    let mut current: Option<String> = None;
    for line in prompt.lines() {
        match (&mut current, line) {
            (None, "```oska") => current = Some(String::new()),
            (Some(_), "```") => programs.extend(current.take()),
            (Some(text), _) => {
                text.push_str(line);
                text.push('\n');
            }
            _ => {}
        }
    }
    assert_eq!(programs.len(), 1, "the prompt has one example program");
    let dir = scratch("hello");
    let file = dir.join("hello.osk");
    std::fs::write(&file, &programs[0]).expect("should write the example");
    let file = file.to_string_lossy().to_string();
    let check = oskac(&["check", &file]);
    assert_eq!(check.status.code(), Some(0), "{check:?}");
    let run = oskac(&["run", &file]);
    assert_eq!(run.status.code(), Some(0), "{run:?}");
    assert_eq!(String::from_utf8_lossy(&run.stdout), "Hello from Oska!\n");
    // The prompt says what it prints.
    assert!(prompt.contains("`Hello from Oska!`"));
}

/// A new, empty directory for a test, under the system's temporary one,
/// removed when the test ends, even when it fails: one test copies `oskac`
/// in, and the copies filled `/tmp` when they were left behind.
fn scratch(name: &str) -> Scratch {
    let dir = std::env::temp_dir().join(format!("oska-agent-setup-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("should create a scratch directory");
    Scratch(dir)
}

/// A scratch directory, removed on drop.
struct Scratch(PathBuf);

impl std::ops::Deref for Scratch {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for Scratch {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ---- The templates -------------------------------------------------------------------------

#[test]
fn the_agents_template_has_the_loop_the_flags_and_the_pointers() {
    let template = setup_file("AGENTS.template.md");
    assert!(template.starts_with("## Working with Oska\n"));
    for line in [
        "oskac check <file.osk> --format=json",
        "apply its `edits`",
        "oskac explain <code>",
        "oskac explain <code> --format=json",
        "oskac run <file.osk> -- args",
        "oskac test <file.osk>",
        "authority report",
        "oskac check <file.osk> --release",
        "https://oskalang.dev/llms.txt",
        "https://oskalang.dev/codes.json",
        "https://oskalang.dev/codes/E0300",
    ] {
        assert!(
            template.contains(line),
            "AGENTS.template.md should say: {line}"
        );
    }
    // The order of the loop: check, fix, run, test, authority.
    let order: Vec<usize> = [
        "1. Check",
        "2. Apply machine fixes",
        "4. Run",
        "5. Test",
        "6. Read the authority",
    ]
    .iter()
    .map(|step| {
        template
            .find(step)
            .unwrap_or_else(|| panic!("no step {step}"))
    })
    .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");
}

#[test]
fn the_claude_template_points_at_agents_md() {
    let template = setup_file("CLAUDE.template.md");
    assert_eq!(template.lines().next(), Some("@AGENTS.md"));
}

// ---- Every page they link is published ------------------------------------------------------

/// The real generator, run on the real docs and setup files in a fake
/// filesystem: what it writes under `_site/`.
fn built_site() -> BTreeMap<String, String> {
    fn add(host: FakeHost, root: &Path, relative: &str) -> FakeHost {
        let path = root.join(relative);
        if path.is_dir() {
            let mut host = host.with_dir(relative);
            let mut names: Vec<_> = std::fs::read_dir(&path)
                .expect("a readable directory")
                .map(|e| e.expect("a directory entry").file_name())
                .collect();
            names.sort();
            for name in names {
                host = add(
                    host,
                    root,
                    &format!("{relative}/{}", name.to_string_lossy()),
                );
            }
            host
        } else {
            host.with_file(relative, &read(&path))
        }
    }
    let root = repo_root();
    let mut host = FakeHost::new();
    for relative in [
        "docs",
        "README.md",
        "GLOSSARY.md",
        "site/style.css",
        "site/search.js",
        "site/agent-setup",
        "Cargo.toml",
    ] {
        host = add(host, &root, relative);
    }
    let build = read(&root.join("site/build.osk"));
    let outcome = oska_run::run(&build, &mut host, &["site/build.osk".to_string()]);
    assert!(
        outcome.exit_code == 0 && outcome.diagnostics.is_empty() && outcome.panic.is_none(),
        "the generator should succeed: {outcome:#?}"
    );
    host.files_under("_site")
}

#[test]
fn every_oskalang_dev_page_the_agent_files_link_is_published() {
    let site = built_site();
    // The installers are published from the release key, not by the
    // generator (see `scripts/release/stage-site.sh`).
    let staged = ["install.sh", "install.ps1"];
    let mut checked = 0;
    for name in AGENT_FILES {
        let text = setup_file(name);
        let mut rest = text.as_str();
        while let Some(at) = rest.find("https://oskalang.dev/") {
            let url = &rest[at + "https://oskalang.dev/".len()..];
            let end = url
                .find(|c: char| c.is_whitespace() || "`)>\"'".contains(c))
                .unwrap_or(url.len());
            let path = url[..end].trim_end_matches(['.', ',', ':', ';']);
            rest = &rest[at + 1..];
            if staged.contains(&path) {
                continue;
            }
            let found = site.contains_key(path)
                || site.contains_key(&format!("{path}.html"))
                || site.contains_key(&format!("{path}index.html"));
            assert!(
                found,
                "{name} links /{path}, which the site does not publish"
            );
            checked += 1;
        }
    }
    assert!(checked >= 8, "only {checked} links checked");
}

#[test]
fn the_site_publishes_the_prompt_and_every_file_and_llms_txt_links_them() {
    let site = built_site();
    let prompt = setup_file("prompt.md");
    assert_eq!(site["agent-setup.md"], prompt);
    assert_eq!(site["agent-setup.txt"], prompt);
    for name in [
        "AGENTS.template.md",
        "CLAUDE.template.md",
        "claude-settings.json",
        "oska-check.sh",
    ] {
        assert_eq!(
            site[&format!("agent-setup/{name}")],
            setup_file(name),
            "{name}"
        );
        assert!(
            site["llms.txt"].contains(&format!("](https://oskalang.dev/agent-setup/{name})")),
            "llms.txt should link {name}"
        );
    }
    for url in ["agent-setup.md", "agent-setup.txt"] {
        assert!(
            site["llms.txt"].contains(&format!("](https://oskalang.dev/{url})")),
            "llms.txt should link {url}"
        );
    }
    // Near the top: before the docs.
    let llms = &site["llms.txt"];
    assert!(
        llms.find("## Set up an agent").expect("a group") < llms.find("## Docs").expect("docs")
    );
}

#[test]
fn the_home_page_and_the_install_guide_show_the_prompt_with_a_copy_button() {
    let site = built_site();
    for page in ["index.html", "guide/install.html", "agent-setup.html"] {
        let html = &site[page];
        assert!(html.contains("<pre id=\"agent-setup-prompt\">"), "{page}");
        assert!(
            html.contains("data-copy=\"agent-setup-prompt\" hidden=\"hidden\">Copy</button>"),
            "{page}"
        );
        // The first line of the prompt is selectable text on the page.
        assert!(
            html.contains("# Set up Oska for AI-driven development\n"),
            "{page}"
        );
    }
}

// ---- The opt-in hook ---------------------------------------------------------------------------

/// Every key of every object in `json`, at any depth.
fn keys(json: &Json, out: &mut Vec<String>) {
    match json {
        Json::Obj(fields) => {
            for (key, value) in fields {
                out.push(key.clone());
                keys(value, out);
            }
        }
        Json::Arr(items) => items.iter().for_each(|item| keys(item, out)),
        _ => {}
    }
}

fn string(json: Option<&Json>) -> &str {
    match json {
        Some(Json::Str(s)) => s,
        other => panic!("{other:?} is not a string"),
    }
}

#[test]
fn the_hook_settings_are_valid_json_that_calls_oskac_after_an_edit_and_when_the_agent_stops() {
    let settings = Json::parse(&setup_file("claude-settings.json"));
    let Json::Obj(top) = &settings else {
        panic!("the settings are an object");
    };
    assert_eq!(
        top.len(),
        1,
        "only `hooks`: it is merged into a project's settings"
    );
    let Some(Json::Obj(events)) = settings.get("hooks") else {
        panic!("a `hooks` object");
    };
    assert_eq!(events.len(), 2, "PostToolUse and Stop");
    let post = only_hook(&settings, "PostToolUse");
    // After an edit, never before: a hook that runs before cannot see the file.
    assert_eq!(
        string(post.0.get("matcher")),
        "Edit|Write|MultiEdit",
        "the edit tools"
    );
    assert_eq!(string(post.1.get("type")), "command");
    assert_eq!(
        string(post.1.get("command")),
        "oskac hook claude --event=post-edit",
        "it calls `oskac` from PATH, with no script and no shell syntax"
    );
    assert!(post.1.get("timeout").is_none(), "the fast hook needs none");
    let stop = only_hook(&settings, "Stop");
    // A Stop entry has no matcher: it is for every attempt to finish.
    assert!(stop.0.get("matcher").is_none());
    assert_eq!(string(stop.1.get("type")), "command");
    assert_eq!(
        string(stop.1.get("command")),
        "oskac hook claude --event=stop"
    );
    // A slow suite must not stall every turn indefinitely.
    let Some(Json::Num(timeout)) = stop.1.get("timeout") else {
        panic!("a numeric `timeout`, in seconds");
    };
    assert!(
        (1..=600).contains(timeout),
        "a timeout of {timeout} seconds"
    );
    // Arguments only, so the same line works in whichever shell Claude
    // Code uses, on every platform.
    for command in [post.1, stop.1].map(|hook| string(hook.get("command"))) {
        assert!(
            command
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '-' || c == '='),
            "shell syntax in `{command}`"
        );
    }
}

/// The one entry of `event`, and the one hook in it.
fn only_hook<'j>(settings: &'j Json, event: &str) -> (&'j Json, &'j Json) {
    let Some(Json::Arr(entries)) = settings.get("hooks").and_then(|h| h.get(event)) else {
        panic!("a {event} list");
    };
    assert_eq!(entries.len(), 1, "{event}");
    let Some(Json::Arr(hooks)) = entries[0].get("hooks") else {
        panic!("a hooks list");
    };
    assert_eq!(hooks.len(), 1, "{event}");
    (&entries[0], &hooks[0])
}

#[test]
fn the_hook_settings_grant_no_permission_and_change_nothing_but_hooks() {
    let settings = Json::parse(&setup_file("claude-settings.json"));
    let mut all = Vec::new();
    keys(&settings, &mut all);
    let allowed: BTreeSet<&str> = [
        "hooks",
        "PostToolUse",
        "Stop",
        "matcher",
        "type",
        "command",
        "timeout",
    ]
    .into();
    for key in &all {
        assert!(allowed.contains(key.as_str()), "unexpected key `{key}`");
    }
    let text = setup_file("claude-settings.json");
    for word in [
        "permissions",
        "allow",
        "deny",
        "env",
        "bypass",
        "enableAll",
        "apiKey",
        "token",
        "http",
        "curl",
        "wget",
    ] {
        assert!(!text.contains(word), "the settings mention `{word}`");
    }
}

/// What a hook run left behind: its exit code, standard output and
/// standard error.
struct Hooked {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl Hooked {
    /// The lines of standard error.
    fn lines(&self) -> Vec<&str> {
        self.stderr.lines().collect()
    }
}

/// `oskac hook claude --event=post-edit`: Claude Code's tool-call JSON on
/// standard input, run in `cwd`, with no shell between: this is what the
/// published settings run, on every platform.
fn post_edit(input: &str, cwd: &Path) -> Hooked {
    hook(&["hook", "claude", "--event=post-edit"], input, cwd)
}

fn hook(args: &[&str], input: &str, cwd: &Path) -> Hooked {
    let mut child = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("oskac should run");
    {
        use std::io::Write;
        let mut stdin = child.stdin.take().expect("a pipe");
        // A hook that has already refused the input need not read it all.
        let _ = stdin.write_all(input.as_bytes());
    }
    let output = child.wait_with_output().expect("the hook should end");
    Hooked {
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    }
}

/// `text` as a JSON string literal.
fn json_string(text: &str) -> String {
    format!("\"{}\"", oska_syntax::json_escape(text))
}

/// The JSON Claude Code gives a PostToolUse hook after an `Edit` of
/// `file`, in the project at `cwd`.
fn tool_call(file: &str, cwd: &Path) -> String {
    format!(
        "{{\"session_id\":\"s\",\"cwd\":{},\"hook_event_name\":\"PostToolUse\",\
         \"tool_name\":\"Edit\",\"tool_input\":{{\"file_path\":{},\
         \"old_string\":\"a\",\"new_string\":\"b\"}},\"tool_response\":{{\"success\":true}}}}",
        json_string(&cwd.to_string_lossy()),
        json_string(file)
    )
}

/// The call after an edit of `file`, a file under `cwd`.
fn edit_of(file: &Path, cwd: &Path) -> String {
    tool_call(&file.to_string_lossy(), cwd)
}

/// `files`, each a path under `dir` and its text, written there.
fn write_all(dir: &Path, files: &[(&str, &str)]) {
    for (path, text) in files {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().expect("a parent")).expect("should create");
        std::fs::write(file, text).expect("should write");
    }
}

const HELLO: &str = "fn main(io: Stdout) -> Result<(), IoError> {\n    io.println(\"hi\")\n}\n";

/// Three diagnostics, one of each applicability: a `machine` fix on line
/// 1, a `maybe` fix on line 10, and a `placeholders` fix on line 11.
const THREE_FIXES: &str = "\
struct Holder {
    out: Stdout,
}

enum Light {
    Red,
    Green,
}

fn name(light: Light, other: Lightt) -> Int {
    match light {
        Light.Red => 1
    }
}

fn main() {}
";

const MANIFEST: &str = "[package]\nname = \"app\"\nversion = \"0.1.0\"\n";

#[test]
fn the_hook_is_quiet_for_an_osk_file_that_checks() {
    let dir = scratch("hook-good");
    write_all(&dir, &[("ok.osk", HELLO)]);
    let hooked = post_edit(&edit_of(&dir.join("ok.osk"), &dir), &dir);
    assert_eq!(
        (hooked.code, hooked.stdout.as_str(), hooked.stderr.as_str()),
        (Some(0), "", "")
    );
}

#[test]
fn the_hook_hands_back_only_the_diagnostics_with_line_and_column_and_exit_status_2() {
    let dir = scratch("hook-bad");
    write_all(&dir, &[("app.osk", THREE_FIXES)]);
    let hooked = post_edit(&edit_of(&dir.join("app.osk"), &dir), &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    assert_eq!(hooked.stdout, "", "the hook prints nothing on stdout");
    let lines = hooked.lines();
    assert_eq!(lines.len(), 4, "{}", hooked.stderr);
    // One line per diagnostic, in the order the checker reports them, with
    // the path written from the project's directory.
    assert!(
        lines[0].starts_with("app.osk:1:8: error[E0210]: "),
        "{lines:?}"
    );
    assert!(
        lines[1].starts_with("app.osk:10:30: error[E0200]: "),
        "{lines:?}"
    );
    assert!(
        lines[2].starts_with("app.osk:11:11: error[E0253]: "),
        "{lines:?}"
    );
    // Then how many have a `machine` fix, and the command that applies them.
    assert_eq!(
        lines[3],
        "1 of 3 diagnostics have a `machine` fix: run `oskac fix app.osk`"
    );
    // Only the diagnostics: no authority report, no JSON, no fix lines.
    assert!(!hooked.stderr.contains("    fix"), "{}", hooked.stderr);
    assert!(!hooked.stderr.contains("bytes "), "{}", hooked.stderr);
    assert!(!hooked.stderr.contains('{'), "{}", hooked.stderr);
}

#[test]
fn the_hook_does_not_mention_oskac_fix_when_no_diagnostic_has_a_machine_fix() {
    let dir = scratch("hook-no-fix");
    write_all(&dir, &[("bad.osk", "fn main() { let x: Int = \"a\" }\n")]);
    let hooked = post_edit(&edit_of(&dir.join("bad.osk"), &dir), &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    let lines = hooked.lines();
    assert_eq!(lines.len(), 1, "{}", hooked.stderr);
    assert!(lines[0].starts_with("bad.osk:1:"), "{lines:?}");
    assert!(lines[0].contains("error[E0220]: "), "{lines:?}");
}

#[test]
fn the_hook_resolves_a_relative_path_against_the_projects_directory() {
    let dir = scratch("hook-relative");
    write_all(
        &dir,
        &[
            ("ok.osk", HELLO),
            ("sub/bad.osk", "fn main() { let x: Int = 1 + \"a\" }\n"),
        ],
    );
    let hooked = post_edit(&tool_call("ok.osk", &dir), &dir);
    assert_eq!((hooked.code, hooked.stderr.as_str()), (Some(0), ""));
    // The hook runs where Claude Code runs it, but `cwd` is what it trusts.
    let elsewhere = scratch("hook-relative-elsewhere");
    let hooked = post_edit(&tool_call("sub/bad.osk", &dir), &elsewhere);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    assert!(hooked.stderr.contains("bad.osk:1:"), "{}", hooked.stderr);
}

#[test]
fn the_hook_checks_a_package_module_with_its_package() {
    // Checked alone, `main.osk` of a package reports `E0203` and `E0221`
    // for the modules it uses; with its package it has nothing to report.
    let main = repo_root().join("examples/shopping/src/main.osk");
    let dir = scratch("hook-package");
    let hooked = post_edit(&edit_of(&main, &dir), &dir);
    assert_eq!((hooked.code, hooked.stderr.as_str()), (Some(0), ""));
}

/// A package `app` under `dir` whose `util` module has an error and whose
/// `main` module is fine.
fn broken_package(dir: &Path) {
    write_all(
        dir,
        &[
            ("app/oska.toml", MANIFEST),
            (
                "app/src/main.osk",
                "use pkg.util\n\nfn main(io: Stdout) -> Result<(), IoError> {\n    io.println(util.name())\n}\n",
            ),
            (
                "app/src/util.osk",
                "pub(pkg) fn name() -> String {\n    let x: Int = \"a\"\n    \"n\"\n}\n",
            ),
        ],
    );
}

#[test]
fn the_hook_reports_an_error_in_another_module_of_the_package() {
    let dir = scratch("hook-package-bad");
    broken_package(&dir);
    let hooked = post_edit(&edit_of(&dir.join("app/src/main.osk"), &dir), &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    let lines = hooked.lines();
    assert_eq!(lines.len(), 1, "{}", hooked.stderr);
    assert!(
        lines[0].starts_with("app/src/util.osk:2:"),
        "the path is from the project's directory: {lines:?}"
    );
    assert!(lines[0].contains("error[E0220]: "), "{lines:?}");
    assert!(!hooked.stderr.contains("E0203"), "{}", hooked.stderr);
}

#[test]
fn the_hook_checks_the_package_of_an_oska_toml_edit() {
    let dir = scratch("hook-manifest");
    broken_package(&dir);
    let hooked = post_edit(&edit_of(&dir.join("app/oska.toml"), &dir), &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    assert!(
        hooked.stderr.contains("app/src/util.osk:2:"),
        "{}",
        hooked.stderr
    );
    // A manifest of a package that checks has nothing to report.
    let good = scratch("hook-manifest-good");
    write_all(&good, &[("oska.toml", MANIFEST), ("src/main.osk", HELLO)]);
    let hooked = post_edit(&edit_of(&good.join("oska.toml"), &good), &good);
    assert_eq!((hooked.code, hooked.stderr.as_str()), (Some(0), ""));
}

#[test]
fn the_hook_ignores_a_file_that_is_not_osk_or_oska_toml_without_a_check() {
    let dir = scratch("hook-other");
    broken_package(&dir);
    write_all(
        &dir,
        &[
            ("app/notes.md", "text"),
            ("app/src/notes.md", "text"),
            ("app/src/oska.toml.bak", "text"),
        ],
    );
    // Each of these is in a package that does not check: a check would
    // end in status 2.
    for file in ["app/notes.md", "app/src/notes.md", "app/src/oska.toml.bak"] {
        let hooked = post_edit(&edit_of(&dir.join(file), &dir), &dir);
        assert_eq!(
            (hooked.code, hooked.stderr.as_str()),
            (Some(0), ""),
            "{file}"
        );
    }
}

#[test]
fn the_hook_shows_at_most_twenty_diagnostics_and_counts_the_rest() {
    let dir = scratch("hook-cap");
    let mut source = String::new();
    for n in 0..25 {
        source.push_str(&format!("struct Holder{n} {{\n    out: Stdout,\n}}\n\n"));
    }
    source.push_str("fn main() {}\n");
    write_all(&dir, &[("many.osk", &source)]);
    let hooked = post_edit(&edit_of(&dir.join("many.osk"), &dir), &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    let lines = hooked.lines();
    assert_eq!(lines.len(), 22, "{}", hooked.stderr);
    for (n, line) in lines[..20].iter().enumerate() {
        let expected = format!("many.osk:{}:8: error[E0210]: ", n * 4 + 1);
        assert!(line.starts_with(&expected), "{line}");
    }
    assert_eq!(lines[20], "and 5 more");
    assert_eq!(
        lines[21],
        "25 of 25 diagnostics have a `machine` fix: run `oskac fix many.osk`"
    );
}

#[test]
fn the_hook_names_the_package_to_fix_when_the_target_is_a_package() {
    let dir = scratch("hook-fix-package");
    write_all(
        &dir,
        &[
            ("app/oska.toml", MANIFEST),
            (
                "app/src/main.osk",
                "struct Holder {\n    out: Stdout,\n}\n\nfn main() {}\n",
            ),
        ],
    );
    let hooked = post_edit(&edit_of(&dir.join("app/src/main.osk"), &dir), &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    let lines = hooked.lines();
    assert!(
        lines[0].starts_with("app/src/main.osk:1:8: error[E0210]: "),
        "{lines:?}"
    );
    assert_eq!(
        lines[1],
        "1 of 1 diagnostics have a `machine` fix: run `oskac fix app`"
    );
}

#[test]
fn malformed_input_or_an_unreadable_file_is_one_line_and_exit_status_1() {
    let dir = scratch("hook-malformed");
    write_all(&dir, &[("ok.osk", HELLO)]);
    let missing = edit_of(&dir.join("missing.osk"), &dir);
    let inputs = [
        // Not JSON, and not even text.
        "this is not json",
        "",
        "{\"tool_input\":",
        "{} {}",
        // JSON of an unexpected shape: no file, or a file that is not a name.
        "{\"tool_input\":{}}",
        "{\"cwd\":\"/\"}",
        "{\"tool_input\":{\"file_path\":7}}",
        "[]",
        "null",
        // A file that is not there.
        missing.as_str(),
    ];
    for input in inputs {
        let hooked = post_edit(input, &dir);
        assert_eq!(hooked.code, Some(1), "{input}: {}", hooked.stderr);
        assert_eq!(hooked.stdout, "", "{input}");
        assert!(
            hooked.stderr.starts_with("error: "),
            "{input}: {}",
            hooked.stderr
        );
        assert_eq!(hooked.lines().len(), 1, "{input}: {}", hooked.stderr);
    }
}

#[test]
fn an_unknown_or_missing_event_is_one_line_and_exit_status_1() {
    let dir = scratch("hook-event");
    for args in [
        &["hook", "claude"][..],
        &["hook", "claude", "--event=sleep"],
        &["hook", "claude", "--event="],
    ] {
        let hooked = hook(args, "{}", &dir);
        assert_eq!(hooked.code, Some(1), "{args:?}: {}", hooked.stderr);
        assert!(hooked.stderr.starts_with("error: "), "{args:?}");
        assert_eq!(hooked.lines().len(), 1, "{args:?}: {}", hooked.stderr);
    }
}

// ---- The Stop gate -------------------------------------------------------------------------------

/// `oskac hook claude --event=stop [targets...]`: Claude Code's Stop-hook
/// JSON on standard input, run in `cwd`.
fn stop(targets: &[&str], cwd: &Path) -> Hooked {
    let mut args = vec!["hook", "claude", "--event=stop"];
    args.extend_from_slice(targets);
    hook(&args, &stop_input(cwd, false), cwd)
}

/// The JSON Claude Code gives a Stop hook in the project at `cwd`;
/// `active` is whether the agent is already continuing after a block.
fn stop_input(cwd: &Path, active: bool) -> String {
    format!(
        "{{\"session_id\":\"s\",\"cwd\":{},\"hook_event_name\":\"Stop\",\
         \"stop_hook_active\":{active}}}",
        json_string(&cwd.to_string_lossy())
    )
}

/// A program that checks for release, with one test that passes.
const PASSING: &str = "\
fn double(n: Int) -> Int {
    n * 2
}

test \"double doubles\" {
    t.assert_eq(double(2), 4)
}
";

/// A program that checks for release, with one test that passes and one
/// that fails.
const FAILING: &str = "\
fn double(n: Int) -> Int {
    n * 2
}

test \"double doubles\" {
    t.assert_eq(double(2), 4)
}

test \"double is wrong here\" {
    t.assert_eq(double(3), 7)
}
";

/// A program that passes `oskac check` and its tests, and fails
/// `oskac check --release`: it leaves a `todo()`.
const UNFINISHED: &str = "\
fn later() -> Int {
    todo()
}

test \"nothing\" {
    t.assert_eq(1, 1)
}
";

/// A program that does not check.
const BROKEN: &str = "fn main() { let x: Int = \"a\" }\n";

#[test]
fn the_stop_gate_is_quiet_and_exit_status_0_when_the_checks_and_the_tests_pass() {
    let dir = scratch("stop-pass");
    write_all(&dir, &[("app.osk", PASSING)]);
    let hooked = stop(&[], &dir);
    assert_eq!(
        (hooked.code, hooked.stdout.as_str(), hooked.stderr.as_str()),
        (Some(0), "", "")
    );
}

#[test]
fn the_stop_gate_blocks_with_exit_status_2_and_names_a_failed_test() {
    let dir = scratch("stop-fail");
    write_all(&dir, &[("app.osk", FAILING)]);
    let hooked = stop(&[], &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    assert_eq!(hooked.stdout, "");
    let failure = hooked
        .stderr
        .lines()
        .find(|l| l.contains("double is wrong here"))
        .unwrap_or_else(|| panic!("the failed test is named: {}", hooked.stderr));
    assert!(
        failure.starts_with("test double is wrong here: app.osk:"),
        "{failure}"
    );
    assert!(
        failure.contains("error[P0011]: ") && failure.contains("actual 6, expected 7"),
        "{failure}"
    );
    assert!(
        !hooked.stderr.contains("double doubles"),
        "a passing test is not mentioned: {}",
        hooked.stderr
    );
    // On every attempt: Claude Code's `stop_hook_active` lets nothing through.
    let again = hook(
        &["hook", "claude", "--event=stop"],
        &stop_input(&dir, true),
        &dir,
    );
    assert_eq!(again.code, Some(2), "{}", again.stderr);
    assert_eq!(again.stderr, hooked.stderr);
}

#[test]
fn the_stop_gate_runs_the_release_check_and_blocks_on_a_todo() {
    let dir = scratch("stop-release");
    write_all(&dir, &[("app.osk", UNFINISHED)]);
    let plain = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(["check", "app.osk"])
        .current_dir(&dir)
        .output()
        .expect("oskac should run");
    assert_eq!(plain.status.code(), Some(0), "the plain check is clean");
    let hooked = stop(&[], &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    assert!(
        hooked
            .stderr
            .lines()
            .any(|l| l.starts_with("app.osk:2:") && l.contains("todo()")),
        "{}",
        hooked.stderr
    );
}

#[test]
fn the_stop_gate_checks_a_package_and_names_its_files_from_the_project() {
    let dir = scratch("stop-package");
    write_all(
        &dir,
        &[
            ("app/oska.toml", MANIFEST),
            ("app/src/main.osk", "fn main() {}\n"),
            ("app/src/util.osk", FAILING),
        ],
    );
    let hooked = stop(&[], &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    assert!(
        hooked
            .stderr
            .contains("test util: double is wrong here: app/src/util.osk:"),
        "{}",
        hooked.stderr
    );
}

#[test]
fn the_stop_gate_with_no_targets_checks_the_package_of_the_project_directory() {
    let dir = scratch("stop-root-package");
    write_all(
        &dir,
        &[
            ("oska.toml", MANIFEST),
            ("src/main.osk", "fn main() {}\n"),
            // Not the package's business: it is not under `src/`.
            ("broken.osk", BROKEN),
        ],
    );
    let hooked = stop(&[], &dir);
    assert_eq!((hooked.code, hooked.stderr.as_str()), (Some(0), ""));
    write_all(&dir, &[("src/util.osk", FAILING)]);
    let hooked = stop(&[], &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    assert!(
        hooked
            .stderr
            .contains("test util: double is wrong here: src/util.osk:"),
        "{}",
        hooked.stderr
    );
}

#[test]
fn the_stop_gate_finds_every_package_and_standalone_file_beneath_the_project() {
    let dir = scratch("stop-discover");
    write_all(
        &dir,
        &[
            ("one/oska.toml", MANIFEST),
            ("one/src/main.osk", "fn main() {}\n"),
            ("tools/two/oska.toml", MANIFEST),
            ("tools/two/src/main.osk", "fn main() {}\n"),
            ("tools/script.osk", PASSING),
            ("top.osk", PASSING),
        ],
    );
    assert_eq!(stop(&[], &dir).code, Some(0));
    // Each one is found: break each in turn and the gate blocks, naming it.
    for (file, broken) in [
        ("one/src/main.osk", BROKEN),
        ("tools/two/src/main.osk", BROKEN),
        ("tools/script.osk", FAILING),
        ("top.osk", FAILING),
    ] {
        let original = std::fs::read(dir.join(file)).expect("readable");
        write_all(&dir, &[(file, broken)]);
        let hooked = stop(&[], &dir);
        assert_eq!(hooked.code, Some(2), "{file}: {}", hooked.stderr);
        assert!(hooked.stderr.contains(file), "{file}: {}", hooked.stderr);
        std::fs::write(dir.join(file), original).expect("should restore");
    }
}

#[test]
fn the_stop_gate_skips_hidden_directories_and_does_not_follow_symlinks() {
    let dir = scratch("stop-skip");
    write_all(
        &dir,
        &[
            ("ok.osk", PASSING),
            (".hidden/bad.osk", FAILING),
            ("sub/.git/bad.osk", FAILING),
        ],
    );
    let hooked = stop(&[], &dir);
    assert_eq!((hooked.code, hooked.stderr.as_str()), (Some(0), ""));
    #[cfg(unix)]
    {
        let outside = scratch("stop-skip-outside");
        write_all(&outside, &[("bad.osk", FAILING)]);
        std::os::unix::fs::symlink(&*outside, dir.join("link")).expect("should link");
        std::os::unix::fs::symlink(outside.join("bad.osk"), dir.join("bad.osk"))
            .expect("should link");
        let hooked = stop(&[], &dir);
        assert_eq!((hooked.code, hooked.stderr.as_str()), (Some(0), ""));
    }
}

#[test]
fn the_stop_gate_checks_only_the_named_targets_resolved_from_the_project() {
    let dir = scratch("stop-named");
    write_all(
        &dir,
        &[
            ("good.osk", PASSING),
            ("fixtures/broken.osk", BROKEN),
            ("pkg/oska.toml", MANIFEST),
            ("pkg/src/main.osk", "fn main() {}\n"),
            ("pkg/src/util.osk", FAILING),
        ],
    );
    // The broken fixture is not named, so it is not checked.
    let hooked = stop(&["good.osk"], &dir);
    assert_eq!((hooked.code, hooked.stderr.as_str()), (Some(0), ""));
    // Run from elsewhere: a target is a path from the project's directory.
    let elsewhere = scratch("stop-named-elsewhere");
    let hooked = hook(
        &["hook", "claude", "--event=stop", "good.osk"],
        &stop_input(&dir, false),
        &elsewhere,
    );
    assert_eq!((hooked.code, hooked.stderr.as_str()), (Some(0), ""));
    // A named package, and a named file, are each checked.
    let hooked = stop(&["good.osk", "pkg"], &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    assert!(
        hooked.stderr.contains("pkg/src/util.osk:"),
        "{}",
        hooked.stderr
    );
    let hooked = stop(&["fixtures/broken.osk"], &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    assert!(
        hooked.stderr.contains("fixtures/broken.osk:1:"),
        "{}",
        hooked.stderr
    );
    // A name that is not there is the hook's failure, not the program's.
    let hooked = stop(&["missing.osk"], &dir);
    assert_eq!(hooked.code, Some(1), "{}", hooked.stderr);
    assert_eq!(hooked.lines().len(), 1, "{}", hooked.stderr);
}

#[test]
fn the_stop_gate_does_not_run_integration_tests() {
    let dir = scratch("stop-integration");
    write_all(
        &dir,
        &[(
            "app.osk",
            "#[integration]\ntest \"fails on the real os\" {\n    t.assert(false)\n}\n\n\
             test \"passes\" {\n    t.assert(true)\n}\n",
        )],
    );
    let hooked = stop(&[], &dir);
    assert_eq!((hooked.code, hooked.stderr.as_str()), (Some(0), ""));
}

/// A program with a test that uses something the interpreter cannot run
/// yet (`E0380`): `oskac test` ends with status 2 on it.
const UNRUNNABLE: &str = "\
test \"uses a constructor as a value\" {
    let mk = Some
    let x: Option<Int> = mk(1)
}
";

#[test]
fn the_stop_gate_does_not_block_on_what_the_interpreter_cannot_run_and_tells_the_user() {
    let dir = scratch("stop-unrunnable");
    write_all(&dir, &[("app.osk", UNRUNNABLE)]);
    let plain = Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(["test", "--interpret", "app.osk"])
        .current_dir(&dir)
        .output()
        .expect("oskac should run");
    assert_eq!(plain.status.code(), Some(2), "{plain:?}");
    let hooked = stop(&[], &dir);
    assert_eq!(hooked.code, Some(0), "{}", hooked.stderr);
    assert_eq!(hooked.stderr, "", "the agent is told nothing");
    // The user is told, through Claude Code's user-visible message.
    let output = Json::parse(&hooked.stdout);
    let message = string(output.get("systemMessage"));
    assert!(message.contains("app.osk"), "{message}");
    assert!(message.contains("E0380"), "{message}");
    // Another target that fails still blocks.
    write_all(&dir, &[("other.osk", FAILING)]);
    let hooked = stop(&[], &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
}

#[test]
fn the_stop_gate_writes_nothing_and_a_test_cannot_reach_the_real_filesystem() {
    let dir = scratch("stop-readonly");
    write_all(
        &dir,
        &[
            ("app.osk", FAILING),
            (
                "writes.osk",
                "test \"writes\" {\n    \
                 let _ = t.fs.restrict_to(\"/\").write_only().write(Path.new(\"out.txt\"), \"x\")\n}\n",
            ),
        ],
    );
    let before = snapshot(&dir);
    stop(&[], &dir);
    assert_eq!(snapshot(&dir), before);
    // The write test runs (and passes) on its own.
    let alone = stop(&["writes.osk"], &dir);
    assert_eq!((alone.code, alone.stderr.as_str()), (Some(0), ""));
    assert_eq!(snapshot(&dir), before);
}

#[test]
fn the_stop_gate_with_malformed_input_is_one_line_and_exit_status_1() {
    let dir = scratch("stop-malformed");
    for input in ["", "not json", "[]", "{\"cwd\":7}", "{} {}"] {
        let hooked = hook(&["hook", "claude", "--event=stop"], input, &dir);
        assert_eq!(hooked.code, Some(1), "{input}: {}", hooked.stderr);
        assert!(hooked.stderr.starts_with("error: "), "{input}");
        assert_eq!(hooked.lines().len(), 1, "{input}: {}", hooked.stderr);
    }
}

/// Every file under `dir`, by path, with its bytes.
fn snapshot(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).expect("readable") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                walk(&path, out);
            } else {
                out.insert(path.clone(), std::fs::read(&path).expect("readable"));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, &mut out);
    out
}

#[test]
fn the_hook_writes_nothing() {
    let dir = scratch("hook-readonly");
    broken_package(&dir);
    write_all(&dir, &[("alone.osk", THREE_FIXES), ("ok.osk", HELLO)]);
    let before = snapshot(&dir);
    for file in ["app/src/main.osk", "app/oska.toml", "alone.osk", "ok.osk"] {
        post_edit(&edit_of(&dir.join(file), &dir), &dir);
    }
    assert_eq!(snapshot(&dir), before);
}

/// A path with a quote in it, which a regular expression over JSON cannot
/// read: the JSON writes it `a\"b.osk`. Windows has no such file names.
#[cfg(unix)]
#[test]
fn the_hook_reads_a_path_with_an_escaped_quote() {
    let dir = scratch("hook-quote");
    write_all(&dir, &[("a\"b.osk", "fn main() { let x: Int = \"a\" }\n")]);
    let hooked = post_edit(&edit_of(&dir.join("a\"b.osk"), &dir), &dir);
    assert_eq!(hooked.code, Some(2), "{}", hooked.stderr);
    assert!(
        hooked.stderr.starts_with("a\"b.osk:1:"),
        "{}",
        hooked.stderr
    );
}

/// The hook's source reads its input and the program's files and nothing
/// else: no file written, no program started, no network.
#[test]
fn the_hooks_source_writes_no_file_starts_no_program_and_opens_no_connection() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut sources = vec![src.join("hook.rs")];
    if let Ok(entries) = std::fs::read_dir(src.join("hook")) {
        sources.extend(entries.map(|e| e.expect("entry").path()));
    }
    for file in sources {
        let text = read(&file);
        for word in [
            "std::net",
            "Command::new",
            "std::process::Command",
            "fs::write",
            "fs::create_dir",
            "fs::remove",
            "fs::rename",
            "fs::copy",
            "File::create",
            "OpenOptions",
        ] {
            assert!(!text.contains(word), "{} contains `{word}`", file.display());
        }
    }
}

/// The shell script, kept for a project that saved it earlier: one call
/// to the same command. `sh` runs it on Unix only.
#[cfg(unix)]
#[test]
fn the_published_script_is_a_one_line_call_to_the_hook() {
    let script = setup_file("oska-check.sh");
    assert!(script.starts_with("#!/bin/sh\n# "), "it keeps its header");
    let code: Vec<&str> = script
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .collect();
    assert_eq!(code, ["exec oskac hook claude --event=post-edit"]);
    // And it does what the command does.
    let dir = scratch("hook-script");
    write_all(&dir, &[("bad.osk", "fn main() { let x: Int = \"a\" }\n")]);
    let oskac_dir = Path::new(env!("CARGO_BIN_EXE_oskac"))
        .parent()
        .expect("a directory")
        .to_string_lossy()
        .to_string();
    let mut child = Command::new("sh")
        .arg(repo_root().join("site/agent-setup/oska-check.sh"))
        .env("PATH", format!("{oskac_dir}:/usr/bin:/bin"))
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("sh should run");
    {
        use std::io::Write;
        let mut stdin = child.stdin.take().expect("a pipe");
        stdin
            .write_all(edit_of(&dir.join("bad.osk"), &dir).as_bytes())
            .expect("should write");
    }
    let output = child.wait_with_output().expect("the script should end");
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).starts_with("bad.osk:1:"),
        "{output:?}"
    );
}

#[test]
fn the_hook_script_reads_only_its_input_and_the_file_and_sends_nothing() {
    let script = setup_file("oska-check.sh");
    for word in [
        "curl", "wget", "nc ", "ssh", "scp", "http", ">>", "rm ", "mv ", "tee",
    ] {
        assert!(!script.contains(word), "the hook script contains `{word}`");
    }
    assert!(script.starts_with("#!/bin/sh\n"));
}
