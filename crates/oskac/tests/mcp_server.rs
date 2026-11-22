//! `std.mcp`'s `serve` (#250), through a fake MCP peer: each transcript in
//! `fixtures/mcp/` is the lines a client writes (`> `) and the exact lines
//! the server must answer (`< `), in order, with `# ` comments. The peer
//! runs an example server as a process on both backends, feeds it the
//! client lines on standard input and compares standard output byte for
//! byte, with the files left behind and the exit code; standard error is
//! the differential harness's to compare too, and is empty here.
//!
//! The server of most transcripts is `examples/notes_server.osk`. The
//! tools of `fixtures/mcp_hints.osk` reach one kind of capability each,
//! for the hints `serve` derives, and `fixtures/mcp_unsplit.osk` is a
//! toolset the checker cannot split per tool, and one generic over its
//! source. `fixtures/mcp_invalid.osk` is a server whose tools are broken.

#![cfg(any(
    all(
        any(target_os = "linux", target_os = "macos"),
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(windows, target_arch = "x86_64")
))]

mod common;
mod differential;

use differential::{assert_same, Case, Outcome};

const NOTES: &str = include_str!("../../../examples/notes_server.osk");
const HINTS: &str = include_str!("fixtures/mcp_hints.osk");
const UNSPLIT: &str = include_str!("fixtures/mcp_unsplit.osk");
const SHIFTING: &str = include_str!("fixtures/mcp_shifting.osk");
const INVALID: &str = include_str!("fixtures/mcp_invalid.osk");

/// The metadata every modern request carries.
const META: &str = r#""_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}"#;

/// The client lines of a transcript, and the server lines it expects, each
/// with its line break.
fn parse(transcript: &str) -> (String, String) {
    let (mut client, mut server) = (String::new(), String::new());
    for line in transcript.lines() {
        if let Some(line) = line.strip_prefix("> ") {
            client.push_str(line);
            client.push('\n');
        } else if let Some(line) = line.strip_prefix("< ") {
            server.push_str(line);
            server.push('\n');
        } else {
            assert!(
                line.starts_with('#'),
                "a transcript line is `> `, `< ` or `# `: {line}"
            );
        }
    }
    (client, server)
}

/// The notes server in a directory that holds a note, a secret outside
/// the notes and an empty place for drafts.
fn notes(name: &str) -> Case {
    Case::file_at(name, "notes_server.osk", NOTES)
        .with_file("notes/a.txt", "hello note\n")
        .with_file("secret.txt", "secret\n")
        .with_file("drafts/.keep", "")
}

/// Run `case` on both backends with the client lines of `transcript`, and
/// require the server lines it expects, and an exit with status 0 at the
/// end of input.
fn play(case: Case, transcript: &str) -> Outcome {
    let (client, server) = parse(transcript);
    let outcome = assert_same(&case.with_stdin(&client));
    assert_eq!(outcome.stdout, server);
    assert_eq!(outcome.exit_code, Some(0));
    assert_eq!(outcome.stderr, "");
    outcome
}

#[test]
fn a_modern_client_discovers_lists_and_calls_and_every_error_is_a_json_rpc_error() {
    let outcome = play(
        notes("mcp-modern"),
        include_str!("fixtures/mcp/modern.transcript"),
    );
    assert_eq!(
        outcome.tree.get("drafts/d.txt"),
        Some(&Some("line one\nline two".to_string()))
    );
}

#[test]
fn a_legacy_client_initializes_and_pings_and_lists_and_calls() {
    play(
        notes("mcp-legacy"),
        include_str!("fixtures/mcp/legacy.transcript"),
    );
}

#[test]
fn a_legacy_client_asking_for_another_version_is_offered_the_legacy_one() {
    play(
        notes("mcp-legacy-other"),
        include_str!("fixtures/mcp/legacy_other.transcript"),
    );
}

#[test]
fn a_path_out_of_the_notes_reads_and_writes_nothing_outside() {
    let outcome = play(
        notes("mcp-confine"),
        include_str!("fixtures/mcp/confine.transcript"),
    );
    assert!(
        !outcome.tree.keys().any(|path| path.contains("escape")),
        "{:?}",
        outcome.tree.keys()
    );
    let drafts: Vec<_> = outcome
        .tree
        .keys()
        .filter(|p| p.starts_with("drafts/"))
        .collect();
    assert_eq!(drafts, ["drafts/", "drafts/.keep"]);
    assert!(!outcome.stdout.contains("secret\\n"), "{}", outcome.stdout);
}

#[test]
fn each_tools_hints_come_from_what_it_reaches() {
    let case = Case::file_at("mcp-hints", "mcp_hints.osk", HINTS)
        .with_file("notes/a.txt", "hello note\n")
        .with_file("drafts/.keep", "");
    play(case, include_str!("fixtures/mcp/hints.transcript"));
}

#[test]
fn a_toolset_not_split_per_tool_gives_every_tool_the_whole_toolsets_hints() {
    let case = Case::file_at("mcp-unsplit", "mcp_unsplit.osk", UNSPLIT)
        .with_file("notes/a.txt", "hello note\n")
        .with_file("drafts/.keep", "");
    play(case, include_str!("fixtures/mcp/unsplit.transcript"));
}

#[test]
fn a_tool_that_reaches_a_capability_through_a_type_parameter_is_not_read_only() {
    let case = Case::file_at("mcp-generic", "mcp_unsplit.osk", UNSPLIT).with_args(&["generic"]);
    play(case, include_str!("fixtures/mcp/generic.transcript"));
}

#[test]
fn the_tools_are_listed_once_and_that_list_is_served_for_the_whole_run() {
    let case =
        Case::file_at("mcp-shifting", "mcp_shifting.osk", SHIFTING).with_file("state/.keep", "");
    let outcome = play(case, include_str!("fixtures/mcp/shifting.transcript"));
    // `specs` did answer differently by the end.
    assert_eq!(
        outcome.tree.get("state/state.txt"),
        Some(&Some("grown".to_string()))
    );
}

#[test]
fn end_of_input_ends_the_server_with_status_0_and_nothing_written() {
    let outcome = assert_same(&notes("mcp-eof"));
    assert_eq!(outcome.stdout, "");
    assert_eq!(outcome.exit_code, Some(0));
}

#[test]
fn a_last_line_without_a_line_break_is_answered() {
    let ping = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{{{META}}}}}"#);
    let outcome = assert_same(&notes("mcp-no-break").with_stdin(&ping));
    assert!(
        outcome
            .stdout
            .starts_with(r#"{"jsonrpc":"2.0","id":1,"result":"#),
        "{}",
        outcome.stdout
    );
    assert_eq!(outcome.exit_code, Some(0));
}

#[test]
fn a_line_over_the_limit_gets_32600_and_the_server_goes_on() {
    // 1 MiB is the limit: this line is one byte over.
    let prefix = r#"{"jsonrpc":"2.0","id":1,"method":"ping","pad":""#;
    let suffix = r#""}"#;
    let pad = "x".repeat(1_048_576 + 1 - prefix.len() - suffix.len());
    let long = format!("{prefix}{pad}{suffix}");
    assert_eq!(long.len(), 1_048_577);
    let at_limit = format!("{}\n", &long[..long.len() - 1]);
    let stdin = format!(
        "{long}\n{at_limit}{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\",\"params\":{{{META}}}}}\n"
    );
    let outcome = assert_same(&notes("mcp-oversized").with_stdin(&stdin));
    let lines: Vec<&str> = outcome.stdout.lines().collect();
    assert_eq!(
        lines[0],
        r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"message longer than the limit"}}"#
    );
    // The line at the limit is read, and is not valid JSON-RPC: an unknown method.
    assert!(
        lines[1].starts_with(r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32700"#),
        "{}",
        lines[1]
    );
    assert!(
        lines[2].starts_with(r#"{"jsonrpc":"2.0","id":2,"result":"#),
        "{}",
        lines[2]
    );
    assert_eq!(lines.len(), 3);
    assert_eq!(outcome.exit_code, Some(0));
}

/// What the invalid server prints for the broken tools `case`, given one
/// request on standard input.
fn invalid(case: &str) -> Outcome {
    let name = format!("mcp-invalid-{case}");
    let request = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
    let outcome = assert_same(
        &Case::file_at(&name, "mcp_invalid.osk", INVALID)
            .with_args(&[case])
            .with_stdin(&format!("{request}\n")),
    );
    assert_eq!(outcome.exit_code, Some(0));
    outcome
}

#[test]
fn serve_refuses_broken_tools_before_it_reads_any_input() {
    let unread = "unread: {\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n";
    for (case, message) in [
        ("duplicate", "two tools are named `alpha`"),
        (
            "space",
            "tool name `not allowed` must be 1 to 128 ASCII letters, digits, `_` and `-`",
        ),
        (
            "dot",
            "tool name `a.b` must be 1 to 128 ASCII letters, digits, `_` and `-`",
        ),
        (
            "accent",
            "tool name `café` must be 1 to 128 ASCII letters, digits, `_` and `-`",
        ),
        (
            "empty",
            "tool name `` must be 1 to 128 ASCII letters, digits, `_` and `-`",
        ),
        (
            "string",
            "the input schema of tool `alpha` must be an object schema",
        ),
        (
            "list",
            "the input schema of tool `alpha` must be an object schema",
        ),
    ] {
        assert_eq!(
            invalid(case).stdout,
            format!("error: {message}\n{unread}"),
            "{case}"
        );
    }
    let long = invalid("long").stdout;
    assert!(
        long.starts_with(&format!(
            "error: tool name `{}` must be 1 to 128",
            "a".repeat(129)
        )),
        "{long}"
    );
    assert!(long.ends_with(unread));
}

#[test]
fn serve_takes_names_of_every_allowed_character_and_128_long() {
    for case in ["longest", "dashes", "none"] {
        let stdout = invalid(case).stdout;
        // The server runs: the ping is answered, as an unknown method in
        // the modern era, which `ping` is not part of.
        assert_eq!(
            stdout,
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"error\":{\"code\":-32601,\"message\":\"method not found: ping\"}}\nserved\nno input left\n",
            "{case}"
        );
    }
}

/// The example's own tests call `mcp.handle` with recorded lines, with no
/// process: they pass on the backend `oskac test` runs.
#[test]
fn the_examples_own_tests_drive_it_with_handle_and_no_process() {
    let root = common::portable::literal_path(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")),
        &["..", ".."],
    );
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_oskac"))
        .args(["test", "examples/notes_server.osk"])
        .current_dir(&root)
        .output()
        .expect("oskac should run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("4 tests: 4 passed, 0 failed"), "{stdout}");
}
