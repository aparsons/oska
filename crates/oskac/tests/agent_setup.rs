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

/// A new, empty directory for a test, under the system's temporary one.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oska-agent-setup-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("should create a scratch directory");
    dir
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
fn the_hook_settings_are_valid_json_that_runs_the_script_after_an_osk_edit() {
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
    assert_eq!(events.len(), 1, "one event");
    let Some(Json::Arr(entries)) = settings.get("hooks").and_then(|h| h.get("PostToolUse")) else {
        panic!("a PostToolUse list");
    };
    assert_eq!(entries.len(), 1);
    // After an edit, never before: a hook that runs before cannot see the file.
    assert_eq!(string(entries[0].get("matcher")), "Edit|Write|MultiEdit");
    let Some(Json::Arr(hooks)) = entries[0].get("hooks") else {
        panic!("a hooks list");
    };
    assert_eq!(hooks.len(), 1);
    assert_eq!(string(hooks[0].get("type")), "command");
    let command = string(hooks[0].get("command"));
    assert_eq!(
        command, "sh \"$CLAUDE_PROJECT_DIR/.claude/hooks/oska-check.sh\"",
        "it runs the project's own copy of the script"
    );
}

#[test]
fn the_hook_settings_grant_no_permission_and_change_nothing_but_hooks() {
    let settings = Json::parse(&setup_file("claude-settings.json"));
    let mut all = Vec::new();
    keys(&settings, &mut all);
    let allowed: BTreeSet<&str> = ["hooks", "PostToolUse", "matcher", "type", "command"].into();
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

/// Run the hook script as Claude Code does: the tool-call JSON on standard
/// input, `PATH` and `OSKA_HOME` as given. Returns the exit code and standard
/// error.
fn run_hook(input: &str, path: &str, oska_home: &Path) -> (Option<i32>, String) {
    let mut child = Command::new("sh")
        .arg(repo_root().join("site/agent-setup/oska-check.sh"))
        .env_clear()
        .env("PATH", path)
        .env("HOME", oska_home)
        .env("OSKA_HOME", oska_home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("sh should run");
    {
        use std::io::Write;
        let mut stdin = child.stdin.take().expect("a pipe");
        stdin.write_all(input.as_bytes()).expect("should write");
    }
    let output = child.wait_with_output().expect("the hook should end");
    assert_eq!(output.stdout, b"", "the hook prints nothing on stdout");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

/// `PATH` for a hook: the system's tools and the directory `oskac` is in.
fn hook_path() -> String {
    let oskac_dir = Path::new(env!("CARGO_BIN_EXE_oskac"))
        .parent()
        .expect("a directory")
        .to_string_lossy()
        .to_string();
    format!("{oskac_dir}:/usr/bin:/bin")
}

fn tool_call(file: &Path) -> String {
    format!(
        "{{\"hook_event_name\":\"PostToolUse\",\"tool_name\":\"Edit\",\"tool_input\":{{\"file_path\":\"{}\",\"old_string\":\"a\",\"new_string\":\"b\"}}}}",
        file.display()
    )
}

#[cfg(unix)]
#[test]
fn the_hook_is_quiet_for_an_osk_file_that_checks() {
    let dir = scratch("hook-good");
    let file = dir.join("ok.osk");
    std::fs::write(
        &file,
        "fn main(io: Stdout) -> Result<(), IoError> {\n    io.println(\"hi\")\n}\n",
    )
    .expect("should write");
    let (code, stderr) = run_hook(&tool_call(&file), &hook_path(), &dir);
    assert_eq!((code, stderr.as_str()), (Some(0), ""));
}

#[cfg(unix)]
#[test]
fn the_hook_hands_a_diagnostic_back_with_exit_status_2() {
    let dir = scratch("hook-bad");
    let file = dir.join("bad.osk");
    std::fs::write(&file, "fn main() { let x: Int = \"a\" }\n").expect("should write");
    let (code, stderr) = run_hook(&tool_call(&file), &hook_path(), &dir);
    assert_eq!(code, Some(2));
    assert!(stderr.contains("\"code\":\"E0220\""), "{stderr}");
}

#[cfg(unix)]
#[test]
fn the_hook_ignores_a_file_that_is_not_osk_without_running_oskac() {
    let dir = scratch("hook-other");
    let file = dir.join("notes.md");
    std::fs::write(&file, "text").expect("should write");
    // No `oskac` on this PATH: running it would end in status 2.
    let (code, stderr) = run_hook(&tool_call(&file), "/usr/bin:/bin", &dir);
    assert_eq!((code, stderr.as_str()), (Some(0), ""));
    let (code, _) = run_hook("{\"tool_input\":{}}", "/usr/bin:/bin", &dir);
    assert_eq!(code, Some(0), "an edit with no file_path");
}

#[cfg(unix)]
#[test]
fn the_hook_prefers_the_installed_oskac_and_does_not_write_anything() {
    let dir = scratch("hook-home");
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).expect("should create");
    std::fs::copy(env!("CARGO_BIN_EXE_oskac"), bin.join("oskac")).expect("should copy");
    let file = dir.join("bad.osk");
    std::fs::write(&file, "fn main() { let x: Int = \"a\" }\n").expect("should write");
    // Not on PATH, but in $OSKA_HOME/bin.
    let (code, stderr) = run_hook(&tool_call(&file), "/usr/bin:/bin", &dir);
    assert_eq!(code, Some(2), "{stderr}");
    assert!(stderr.contains("E0220"), "{stderr}");
    let names: BTreeSet<String> = std::fs::read_dir(&dir)
        .expect("readable")
        .map(|e| e.expect("entry").file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(
        names,
        ["bad.osk", "bin"].iter().map(|s| s.to_string()).collect()
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
