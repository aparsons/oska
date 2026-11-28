// SPDX-License-Identifier: Apache-2.0
//! The agent-setup prompt (spec #70, ticket #80), continued from
//! `agent_setup.rs`: the opt-in Claude Code hook, the Claude Code plugin
//! (spec #206), its `oska` skill (#211) and its hooks and install steps (#212).

mod common;

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use common::agent_setup::*;
use common::hook::{scratch, write_all};
use common::Json;

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

/// `text` with Windows line ends made Unix ones, for a clone that checks
/// files out with CRLF.
fn lf(text: &str) -> String {
    text.replace("\r\n", "\n")
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
                    Json::Str(s)
                        if ["source", "matcher", "type", "command"].contains(&key.as_str()) =>
                    {
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
            // A skill's front matter keys, such as `paths`.
            out.push_str(&format!("{file}\n"));
            let text = lf(&read(&repo_root().join(&file)));
            if let Some(front) = text
                .strip_prefix("---\n")
                .and_then(|rest| rest.split_once("\n---\n"))
            {
                for line in front.0.lines() {
                    let key = line.split(':').next().unwrap_or(line);
                    out.push_str(&format!("{file}: {key}\n"));
                }
            }
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
        lf(&read(&fixture)),
        actual,
        "the plugin's files or keys changed; review the change, then rerun with \
         UPDATE_GOLDEN=1 cargo test -p oskac --test agent_setup and commit the fixture"
    );
}

#[test]
fn the_plugin_grants_no_claude_code_permission_and_holds_no_program() {
    let allowed: BTreeSet<&str> = [
        // The marketplace.
        "name",
        "owner",
        "metadata",
        "description",
        "plugins",
        "source",
        // plugin.json.
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
        lf(&setup_file("AGENTS.template.md"))
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
        lf(&read(&path)) == expected,
        "{SKILL} differs from site/agent-setup/AGENTS.template.md with its front matter; \
         edit the template, not the skill, then regenerate it with \
         UPDATE_GOLDEN=1 cargo test -p oskac --test agent_setup and commit both"
    );
}

#[test]
fn the_skill_follows_the_agent_skills_format() {
    let skill = lf(&read(&repo_root().join(SKILL)));
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

// ---- The plugin's hooks and how to install it (#212) ----------------------------------------

#[test]
fn the_plugin_hooks_are_the_published_hook_settings() {
    // Both files have the same `{"hooks": {...}}` shape, so the plugin and
    // the settings a project copies run the same events, matchers and
    // commands: `oskac hook claude --event=post-edit` and `--event=stop`.
    let plugin = repo_json("plugins/oska/hooks/hooks.json");
    let published = Json::parse(&setup_file("claude-settings.json"));
    assert!(
        plugin == published,
        "plugins/oska/hooks/hooks.json differs from site/agent-setup/claude-settings.json; \
         change both together"
    );
    assert_eq!(
        string(only_hook(&plugin, "PostToolUse").1.get("command")),
        "oskac hook claude --event=post-edit"
    );
    assert_eq!(
        string(only_hook(&plugin, "Stop").1.get("command")),
        "oskac hook claude --event=stop"
    );
}

/// The two commands a Claude Code user types to install the plugin.
const PLUGIN_INSTALL: [&str; 2] = [
    "/plugin marketplace add aparsons/oska",
    "/plugin install oska@oska",
];

#[test]
fn the_guide_the_prompt_the_site_and_llms_txt_say_how_to_install_the_plugin() {
    // The names in the commands are the names in the files.
    assert_eq!(string(repo_json(MARKETPLACE).get("name")), "oska");
    let site = built_site();
    let texts = [
        ("prompt.md", setup_file("prompt.md")),
        (
            "docs/guide/agent-guide.md",
            read(&repo_root().join("docs/guide/agent-guide.md")),
        ),
        ("agent-setup.html", site["agent-setup.html"].clone()),
        ("llms.txt", site["llms.txt"].clone()),
        ("CHANGELOG.md", read(&repo_root().join("CHANGELOG.md"))),
    ];
    for (name, text) in &texts {
        for command in PLUGIN_INSTALL {
            assert!(text.contains(command), "{name} should say `{command}`");
        }
    }
    // The guide's section, which the site page and llms.txt link.
    assert!(site["guide/agent-guide.html"].contains("id=\"the-claude-code-plugin\""));
    // The prompt leaves the install to the person: an agent does not
    // change their Claude Code setup on its own.
    assert!(setup_file("prompt.md").contains("Do not install it for me."));
}
