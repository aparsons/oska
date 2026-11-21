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
use std::process::{Command, Output};

use common::hook::{scratch, write_all, HELLO};
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
    for flag in [
        "--json",
        "--version",
        "--format",
        "--baseline",
        "--update-baseline",
        "--event",
    ] {
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
    for command in ["check", "run", "test", "explain", "fix", "hook"] {
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
        "action",
        "previous_version",
        "index_verified",
        "update_available",
        "removed",
        "kept",
        "path_hint",
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
        "`oskac fix <file.osk>`",
        "run the installer again",
        "--check --json",
        "--uninstall --json",
        "`oskac self uninstall --format=json`",
        "`oskac self info --format=json`",
        "`downgrade_refused`",
        "https://oskalang.dev/llms.txt",
        // The update step stops as the install step does, and reports the
        // action, a downgrade included.
        "If `ok` is not `true`, stop",
        "`index_verified` is not `true`, stop",
        "`action`",
        "`previous_version`",
        "`downgraded`",
        // The project's minimum toolchain, with the diff first.
        "oska = \"<version>\"",
        // The report gives the update and uninstall commands.
        "To update",
        "To uninstall",
    ] {
        assert!(prompt.contains(step), "the prompt should say: {step}");
    }
    for stale in ["there is no self-update command", "not_managed"] {
        assert!(
            !prompt.contains(stale),
            "the prompt should not say: {stale}"
        );
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
            "## 4. Run the tests and the authority gate in CI",
            "## 5. Update, check or uninstall later",
            "## 6. Report",
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

// ---- The templates -------------------------------------------------------------------------

#[test]
fn the_agents_template_has_the_loop_the_flags_and_the_pointers() {
    let template = setup_file("AGENTS.template.md");
    assert!(template.starts_with("## Working with Oska\n"));
    for line in [
        "oskac check <file.osk> --format=json",
        "oskac fix <file.osk>",
        "oskac fix <file.osk> --dry-run",
        "oskac fix <file.osk> --placeholders",
        "oskac explain <code>",
        "oskac explain <code> --format=json",
        "oskac run <file.osk> -- args",
        "oskac test <file.osk>",
        "authority report",
        "oskac check <file.osk> --baseline=authority.json",
        "oskac check <file.osk> --release",
        "https://oskalang.dev/llms.txt",
        "https://oskalang.dev/codes.json",
        "https://oskalang.dev/codes/E0300",
        // How this oskac was installed, and whether an update exists.
        "oskac self info --format=json",
        "--check --json",
    ] {
        assert!(
            template.contains(line),
            "AGENTS.template.md should say: {line}"
        );
    }
    // The loop uses the tool: no hand-written fix loop, no hand-made diff.
    for stale in ["apply its `edits`", "span.start", "compare this against"] {
        assert!(
            !template.contains(stale),
            "AGENTS.template.md still says: {stale}"
        );
    }
    // The order of the loop: check, fix, run, test, authority.
    let order: Vec<usize> = [
        "1. Check",
        "2. Apply machine fixes",
        "4. Run",
        "5. Test",
        "6. Compare the authority",
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

/// The first code span or fenced line of `text` that contains `needle`.
fn command_with(text: &str, needle: &str) -> String {
    code_lines(text)
        .into_iter()
        .find(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("no code span with `{needle}`"))
}

/// The arguments of `command`, a line such as `oskac check <file.osk>
/// --baseline=authority.json`, with the placeholder replaced by `file`.
fn args_of(command: &str, file: &str) -> Vec<String> {
    command
        .split_whitespace()
        .skip_while(|word| *word != "oskac")
        .skip(1)
        .map(|word| word.replace("<file.osk>", file).replace("<path>", file))
        .collect()
}

fn run_in(dir: &Path, args: &[String]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("oskac should run")
}

#[test]
fn the_agents_template_says_not_to_refresh_a_baseline_to_pass_the_gate() {
    let template = setup_file("AGENTS.template.md");
    assert!(
        template.contains(
            "Do not refresh a committed baseline to make the gate pass without telling the user why"
        ),
        "the rule that approving a widening is the user's decision"
    );
    assert!(template.contains("oskac check <file.osk> --update-baseline=authority.json"));
    assert!(template.contains("exit code 3"));
}

#[test]
fn the_template_names_the_baseline_commands_that_work_as_it_says() {
    let template = setup_file("AGENTS.template.md");
    let dir = scratch("template-baseline");
    write_all(&dir, &[("app.osk", HELLO)]);
    let write = args_of(&command_with(&template, "--update-baseline="), "app.osk");
    let check = args_of(&command_with(&template, "--baseline="), "app.osk");
    assert_eq!(run_in(&dir, &write).status.code(), Some(0));
    assert!(dir.join("authority.json").exists());
    assert_eq!(run_in(&dir, &check).status.code(), Some(0));
    // A widening is exit code 3, as the template says.
    write_all(
        &dir,
        &[(
            "app.osk",
            "fn main(io: Stdout, net: Net) -> Result<(), IoError> {\n    io.println(\"hi\")\n}\n",
        )],
    );
    let widened = run_in(&dir, &check);
    assert_eq!(widened.status.code(), Some(3), "{widened:?}");
}

#[test]
fn the_fix_command_the_template_names_applies_the_certain_fixes_and_dry_run_writes_nothing() {
    let template = setup_file("AGENTS.template.md");
    let source = "struct Logger { out: Stdout }\n\nfn main() {}\n";
    let dir = scratch("template-fix");
    write_all(&dir, &[("app.osk", source)]);
    let fix = args_of(&command_with(&template, "oskac fix <file.osk>"), "app.osk");
    let mut dry = fix.clone();
    dry.push("--dry-run".to_string());
    assert_eq!(run_in(&dir, &dry).status.code(), Some(0));
    assert_eq!(
        read(&dir.join("app.osk")),
        source,
        "--dry-run writes nothing"
    );
    assert_eq!(run_in(&dir, &fix).status.code(), Some(0));
    assert!(read(&dir.join("app.osk")).starts_with("cap struct Logger"));
}

#[test]
fn the_prompt_offers_both_hooks_on_every_platform_and_installs_no_script() {
    let prompt = setup_file("prompt.md");
    for line in [
        "**3c. The check-after-edit hook",
        "**3d. The Stop gate",
        "`oskac hook claude --event=post-edit`",
        "`oskac hook claude --event=stop`",
        "Ask me whether I want it, and do nothing if I do not say yes.",
        "deliberately broken",
        "`oskac` by name",
    ] {
        assert!(prompt.contains(line), "the prompt should say: {line}");
    }
    // The hook is no longer a shell script, so it is not limited to Unix,
    // and the agent merges the settings itself on every platform: the
    // installer's `--hook` sets up the post-edit hook alone, not the gate.
    for gone in [
        "Linux and macOS only",
        "POSIX",
        "agent-setup/oska-check.sh",
        "skip this step",
        "--hook",
        "settings_merge_needed",
    ] {
        assert!(!prompt.contains(gone), "the prompt still says: {gone}");
    }
    // The two hooks are separate choices, in this order.
    let post = prompt.find("**3c.").expect("3c");
    let stop = prompt.find("**3d.").expect("3d");
    assert!(post < stop);
}

/// A project set up with the old shell hook has a `PostToolUse` entry that
/// runs `.claude/hooks/oska-check.sh`. The prompt still merges each entry
/// the user agrees to: the post-edit entry in place of the old one, and the
/// Stop entry whether or not the old one is there.
#[test]
fn the_prompt_merges_each_agreed_entry_into_a_project_with_the_old_shell_hook() {
    let prompt = setup_file("prompt.md");
    for line in [
        "Merge each entry I agreed to that the settings do not already run, whatever other hooks they have.",
        "a `PostToolUse` entry that runs `.claude/hooks/oska-check.sh`, the old shell hook",
        "For 3c, replace that entry with the published one",
        "It is not the Stop gate: for 3d, add the `Stop` entry all the same.",
        "If I declined 3c, leave the old entry as it is.",
    ] {
        assert!(prompt.contains(line), "the prompt should say: {line}");
    }
    // The old script is mentioned only as an entry to replace.
    for line in prompt.lines().filter(|l| l.contains("oska-check.sh")) {
        assert!(line.starts_with("For either, show me the diff"), "{line}");
    }
}

#[test]
fn the_prompt_names_each_hook_command_of_the_published_settings() {
    let prompt = setup_file("prompt.md");
    let settings = Json::parse(&setup_file("claude-settings.json"));
    for event in ["PostToolUse", "Stop"] {
        let (_, hook) = only_hook(&settings, event);
        let command = string(hook.get("command"));
        assert!(
            prompt.contains(&format!("`{command}`")),
            "the prompt should name `{command}`"
        );
    }
    // Every `oskac hook` the agent files run is one the settings run, with
    // targets added only for the Stop gate.
    for name in AGENT_FILES {
        for code in code_lines(&setup_file(name)) {
            if let Some(at) = code.find("oskac hook claude --event=") {
                let command = &code[at..];
                assert!(
                    command.starts_with("oskac hook claude --event=post-edit")
                        || command.starts_with("oskac hook claude --event=stop"),
                    "{name}: {command}"
                );
            }
        }
    }
}

#[test]
fn the_prompt_only_enables_a_hook_when_oskac_is_on_the_path() {
    let prompt = setup_file("prompt.md");
    assert!(
        prompt.contains("`on_path` is `true`"),
        "a hook is enabled only then"
    );
    assert!(
        prompt.contains("`on_path` is `false`") && prompt.contains("`bin_dir`"),
        "otherwise it says to add `bin_dir` to `PATH` first"
    );
}

#[test]
fn the_prompt_offers_an_authority_baseline_and_a_ci_step_that_runs_it() {
    let prompt = setup_file("prompt.md");
    for line in [
        "--update-baseline=authority.json",
        "--baseline=authority.json",
        "exit code 3",
        "Do not refresh `authority.json` to make the gate pass",
    ] {
        assert!(prompt.contains(line), "the prompt should say: {line}");
    }
    // The commands are real: write the baseline, then the CI step passes.
    let dir = scratch("prompt-baseline");
    write_all(&dir, &[("app.osk", HELLO)]);
    let write = args_of(&command_with(&prompt, "--update-baseline="), "app.osk");
    let gate = args_of(&command_with(&prompt, "--baseline="), "app.osk");
    assert_eq!(run_in(&dir, &write).status.code(), Some(0));
    assert_eq!(run_in(&dir, &gate).status.code(), Some(0));
}

#[test]
fn the_claude_template_points_at_agents_md() {
    let template = setup_file("CLAUDE.template.md");
    assert_eq!(template.lines().next(), Some("@AGENTS.md"));
}

// ---- Every page they link is published ------------------------------------------------------

/// The real generator, run on the real docs and setup files in a fake
/// filesystem: what it writes under `_site/`.
/// Built once per test program: a build takes seconds.
fn built_site() -> &'static BTreeMap<String, String> {
    static SITE: std::sync::OnceLock<BTreeMap<String, String>> = std::sync::OnceLock::new();
    SITE.get_or_init(build_site)
}

/// [`built_site`], built anew.
fn build_site() -> BTreeMap<String, String> {
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
        "site/favicon.svg",
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
        "claude-settings-post-edit.json",
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

/// What `install.sh --hook` writes: the post-edit entry of the published
/// settings alone, never the Stop gate, which is a separate choice.
#[test]
fn the_post_edit_settings_are_the_post_edit_entry_of_the_published_settings_alone() {
    let full = Json::parse(&setup_file("claude-settings.json"));
    let post_edit = Json::parse(&setup_file("claude-settings-post-edit.json"));
    let entry = full
        .get("hooks")
        .and_then(|hooks| hooks.get("PostToolUse"))
        .expect("a PostToolUse entry")
        .clone();
    assert_eq!(
        post_edit,
        Json::Obj(vec![(
            "hooks".to_string(),
            Json::Obj(vec![("PostToolUse".to_string(), entry)])
        )])
    );
}

#[test]
fn the_hook_settings_grant_no_permission_and_change_nothing_but_hooks() {
    for name in ["claude-settings.json", "claude-settings-post-edit.json"] {
        settings_grant_no_permission(name);
    }
}

fn settings_grant_no_permission(name: &str) {
    let settings = Json::parse(&setup_file(name));
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
        assert!(
            allowed.contains(key.as_str()),
            "{name}: unexpected key `{key}`"
        );
    }
    let text = setup_file(name);
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
        assert!(!text.contains(word), "{name} mentions `{word}`");
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
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("sh should run");
    {
        use std::io::Write;
        let mut stdin = child.stdin.take().expect("a pipe");
        stdin
            .write_all(common::hook::edit_of(&dir.join("bad.osk"), &dir).as_bytes())
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

// ---- The Claude Code plugin (spec #206) -------------------------------------------------------

/// The marketplace file at the repository root, from which
/// `/plugin marketplace add aparsons/oska` reads the plugin list.
const MARKETPLACE: &str = ".claude-plugin/marketplace.json";

/// The plugin's directory, from the repository root.
const PLUGIN_DIR: &str = "plugins/oska";

/// Every file of the plugin, as `/`-separated paths from the repository
/// root, sorted, with the marketplace file first.
fn plugin_files() -> Vec<String> {
    fn walk(root: &Path, relative: &str, out: &mut Vec<String>) {
        let mut names: Vec<_> = std::fs::read_dir(root.join(relative))
            .unwrap_or_else(|e| panic!("cannot list {relative}: {e}"))
            .map(|e| e.expect("a directory entry").file_name())
            .collect();
        names.sort();
        for name in names {
            let path = format!("{relative}/{}", name.to_string_lossy());
            if root.join(&path).is_dir() {
                walk(root, &path, out);
            } else {
                out.push(path);
            }
        }
    }
    let mut files = vec![MARKETPLACE.to_string()];
    walk(&repo_root(), PLUGIN_DIR, &mut files);
    files
}

fn repo_json(relative: &str) -> Json {
    Json::parse(&read(&repo_root().join(relative)))
}

/// The workspace version in `Cargo.toml`, such as `0.6.0`.
fn workspace_version() -> String {
    read(&repo_root().join("Cargo.toml"))
        .split("[workspace.package]")
        .nth(1)
        .expect("Cargo.toml has a [workspace.package] table")
        .lines()
        .find_map(|line| line.trim().strip_prefix("version = "))
        .expect("[workspace.package] names a version")
        .trim_matches('"')
        .to_string()
}

#[test]
fn the_marketplace_lists_exactly_the_oska_plugin_from_this_repository() {
    let marketplace = repo_json(MARKETPLACE);
    // `/plugin install oska@oska`: the plugin `oska` from the marketplace `oska`.
    assert_eq!(string(marketplace.get("name")), "oska");
    assert!(!string(marketplace.get("owner").and_then(|o| o.get("name"))).is_empty());
    let Some(Json::Arr(plugins)) = marketplace.get("plugins") else {
        panic!("a `plugins` list");
    };
    assert_eq!(plugins.len(), 1, "exactly one plugin");
    let entry = &plugins[0];
    assert_eq!(string(entry.get("name")), "oska");
    assert!(!string(entry.get("description")).is_empty());
    // A path in this repository, never another repository or a URL: the
    // plugin comes from the same signed repository as the release.
    assert_eq!(string(entry.get("source")), format!("./{PLUGIN_DIR}"));
    assert!(repo_root()
        .join(PLUGIN_DIR)
        .join(".claude-plugin/plugin.json")
        .is_file());
}

#[test]
fn the_plugin_manifest_names_oska_at_the_workspace_version() {
    let manifest = repo_json(&format!("{PLUGIN_DIR}/.claude-plugin/plugin.json"));
    assert_eq!(string(manifest.get("name")), "oska");
    // The release process bumps both (docs/agents/release.md), so a user
    // can tell which `oskac` the plugin's guidance describes.
    assert_eq!(
        string(manifest.get("version")),
        workspace_version(),
        "set `version` in {PLUGIN_DIR}/.claude-plugin/plugin.json to the workspace version"
    );
    assert!(!string(manifest.get("description")).is_empty());
    assert!(!string(manifest.get("author").and_then(|a| a.get("name"))).is_empty());
    let marketplace = repo_json(MARKETPLACE);
    let Some(Json::Arr(plugins)) = marketplace.get("plugins") else {
        panic!("a `plugins` list");
    };
    assert_eq!(
        plugins[0].get("description"),
        manifest.get("description"),
        "one description in both files"
    );
}

/// The key paths of `json` (`a.b[].c`), each with its value when it is a
/// string that decides what runs or where the plugin comes from.
fn key_paths(json: &Json, prefix: &str, out: &mut Vec<String>) {
    match json {
        Json::Obj(fields) => {
            for (key, value) in fields {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                match value {
                    Json::Str(s) if ["source", "matcher", "command"].contains(&key.as_str()) => {
                        out.push(format!("{path} = {s}"));
                    }
                    Json::Obj(_) | Json::Arr(_) => key_paths(value, &path, out),
                    _ => out.push(path),
                }
            }
        }
        Json::Arr(items) => items
            .iter()
            .for_each(|item| key_paths(item, &format!("{prefix}[]"), out)),
        _ => out.push(prefix.to_string()),
    }
}

/// What the plugin is allowed to do, one line per key of each JSON file,
/// with the values that decide what runs. A change to it, such as a new
/// hook or a new kind of key, shows in the reviewed diff of the fixture.
fn plugin_authority() -> String {
    let mut out = String::new();
    for file in plugin_files() {
        if file.ends_with(".json") {
            let mut paths = Vec::new();
            key_paths(&repo_json(&file), "", &mut paths);
            for path in paths {
                out.push_str(&format!("{file}: {path}\n"));
            }
        } else {
            out.push_str(&format!("{file}\n"));
        }
    }
    out
}

#[test]
fn the_plugin_authority_matches_its_reviewed_fixture() {
    let fixture = repo_root().join("crates/oskac/tests/fixtures/plugin-authority.txt");
    let actual = plugin_authority();
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&fixture, &actual).expect("should write the fixture");
    }
    assert_eq!(
        read(&fixture),
        actual,
        "the plugin's files or keys changed; review the change, then rerun with \
         UPDATE_GOLDEN=1 cargo test -p oskac --test agent_setup and commit the fixture"
    );
}

#[test]
fn the_plugin_requests_no_permission_and_holds_no_program() {
    let allowed: BTreeSet<&str> = [
        // The marketplace.
        "name",
        "owner",
        "metadata",
        "description",
        "plugins",
        "source",
        // The manifest.
        "version",
        "author",
        // The hooks.
        "hooks",
        "PostToolUse",
        "Stop",
        "matcher",
        "type",
        "command",
        "timeout",
    ]
    .into();
    for file in plugin_files() {
        // Text and JSON only: nothing a reviewer cannot read, nothing to run.
        assert!(
            file.ends_with(".json") || file.ends_with(".md"),
            "{file} is not JSON or Markdown"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(repo_root().join(&file))
                .expect("metadata")
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0, "{file} is executable");
        }
        let text = read(&repo_root().join(&file));
        assert!(!text.starts_with("#!"), "{file} is a script");
        if file.ends_with(".json") {
            let mut all = Vec::new();
            keys(&Json::parse(&text), &mut all);
            for key in &all {
                assert!(
                    allowed.contains(key.as_str()),
                    "{file}: key `{key}` is not allowed in the plugin"
                );
            }
        }
        // Nothing fetches or sends data, and no file reaches into the
        // plugin's own directory for a program to run.
        for word in ["curl", "wget", "CLAUDE_PLUGIN_ROOT", "allowed-tools"] {
            assert!(!text.contains(word), "{file} mentions `{word}`");
        }
        if file.ends_with(".json") {
            for word in [
                "permissions",
                "allowedTools",
                "\"allow\"",
                "\"deny\"",
                "\"env\"",
                "mcpServers",
                "lspServers",
                "agents",
                "bypass",
                "apiKey",
            ] {
                assert!(!text.contains(word), "{file} mentions `{word}`");
            }
        }
    }
}

// ---- The plugin's `oska` skill (#211) ---------------------------------------------------------

/// The skill, in the plugin's default skills directory.
const SKILL: &str = "plugins/oska/skills/oska/SKILL.md";

/// When the skill applies, in the words an agent matches a task against.
/// Every enabled plugin's skill descriptions sit in the agent's context on
/// every turn, so it stays short.
const SKILL_DESCRIPTION: &str = "Check, fix, run and test Oska code with oskac. \
Use when writing or changing Oska code, in .osk files or oska.toml.";

/// The skill as the generator writes it: front matter in the Agent Skills
/// format, then the `AGENTS.md` template unchanged, which stays the one
/// hand-written copy of the guidance. `paths` loads the skill on its own
/// only for Oska files; an agent that does not know the key ignores it.
fn generated_skill() -> String {
    format!(
        "---\nname: oska\ndescription: {SKILL_DESCRIPTION}\npaths: [\"**/*.osk\", \"**/oska.toml\"]\n---\n\n{}",
        setup_file("AGENTS.template.md")
    )
}

#[test]
fn the_skill_is_the_agents_template_with_front_matter() {
    let path = repo_root().join(SKILL);
    let expected = generated_skill();
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().expect("a directory")).expect("should create it");
        std::fs::write(&path, &expected).expect("should write the skill");
    }
    assert!(
        read(&path) == expected,
        "{SKILL} differs from site/agent-setup/AGENTS.template.md with its front matter; \
         edit the template, not the skill, then regenerate it with \
         UPDATE_GOLDEN=1 cargo test -p oskac --test agent_setup and commit both"
    );
}

#[test]
fn the_skill_follows_the_agent_skills_format() {
    let skill = read(&repo_root().join(SKILL));
    let front = skill
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n"))
        .expect("front matter between `---` lines")
        .0;
    let field = |key: &str| {
        front
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{key}: ")))
            .unwrap_or_else(|| panic!("a `{key}` field"))
    };
    // The name: lowercase letters, digits and hyphens, the directory's name.
    let name = field("name");
    assert_eq!(name, "oska");
    assert!(name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
    assert!(SKILL.ends_with(&format!("/{name}/SKILL.md")));
    // The description: within the format's 1024 characters, and short
    // enough to cost little on every turn.
    let description = field("description");
    assert!(
        description.len() <= 200,
        "a {}-character description",
        description.len()
    );
    // A plain YAML scalar: a `: ` or ` #` inside it would end it early.
    assert!(!description.contains(": ") && !description.contains(" #"));
    for word in ["Oska", ".osk", "oska.toml"] {
        assert!(description.contains(word), "the description names {word}");
    }
    assert!(skill.lines().count() < 500, "a body of reasonable length");
    // The only model-invocable skill: its standing cost is one description.
    let skills_dir = repo_root().join("plugins/oska/skills");
    let skills: Vec<_> = std::fs::read_dir(&skills_dir)
        .expect("a skills directory")
        .collect();
    assert_eq!(skills.len(), 1, "one skill in the plugin");
    // The front matter names no host and no Claude Code feature but `paths`,
    // so the body is the vendor-neutral template and nothing more.
    assert!(!front.contains("http") && !front.to_lowercase().contains("claude"));
}
