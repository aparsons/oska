// SPDX-License-Identifier: Apache-2.0
//! `std.mcp`'s `Toolset`, `ToolSpec` and `ToolResult` (#249), run on both
//! backends through the differential harness: a toolset lists its specs,
//! with a title and an input schema, and its `call` gives text, JSON and
//! error results. The output is the same, byte for byte, on both.

#![cfg(any(
    all(
        any(target_os = "linux", target_os = "macos"),
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(windows, target_arch = "x86_64")
))]

mod differential;

use differential::{assert_same, Case};

const TOOLS: &str = r#"use std.json
use std.mcp

cap struct Echo { out: Stdout }

impl mcp.Toolset for Echo {
    fn specs(self) -> List<mcp.ToolSpec> {
        let input = json.Schema.object([json.Field.required("text", json.Schema.string())])
        [
            mcp.ToolSpec.new("echo", "Echo the arguments.", input).with_title("Echo"),
            mcp.ToolSpec.new("fail", "Always fail.", json.Schema.object([])),
        ]
    }

    fn call(self, name: String, args: json.Value) -> mcp.ToolResult {
        match name {
            "echo" => mcp.ToolResult.json(args)
            "fail" => mcp.ToolResult.error("{name} failed")
            _ => mcp.ToolResult.text("unknown")
        }
    }
}

fn show(io: Stdout, result: mcp.ToolResult) -> Result<(), IoError> {
    let content = result.content()
    let state = if result.is_error() { "error" } else { "ok" }
    io.println("{state}: {content}")?
    match result.structured() {
        Some(value) => {
            let text = json.encode(value)
            io.println("structured: {text}")
        }
        None => io.println("no structured content")
    }
}

fn main(io: Stdout) -> Result<(), IoError> {
    let tools = Echo(out: io)
    for spec in tools.specs() {
        let name = spec.name()
        let title = spec.title().unwrap_or("(none)")
        let description = spec.description()
        let input = json.encode(spec.input().to_value())
        io.println("{name} {title}: {description} {input}")?
    }
    let args = json.Value.Object([json.Member(key: "text", value: json.Value.Text("a\nb"))])
    show(io, tools.call("echo", args))?
    show(io, tools.call("fail", json.Value.Null))?
    show(io, mcp.ToolResult.text("plain"))
}
"#;

#[test]
fn a_toolset_lists_its_specs_and_runs_its_tools() {
    let outcome = assert_same(&Case::file("mcp_toolset", TOOLS));
    assert_eq!(
        outcome.stdout,
        "echo Echo: Echo the arguments. {\"type\":\"object\",\"properties\":{\"text\":{\"type\":\"string\"}},\"required\":[\"text\"],\"additionalProperties\":false}\n\
         fail (none): Always fail. {\"type\":\"object\",\"properties\":{},\"required\":[],\"additionalProperties\":false}\n\
         ok: {\"text\":\"a\\nb\"}\n\
         structured: {\"text\":\"a\\nb\"}\n\
         error: fail failed\n\
         no structured content\n\
         ok: plain\n\
         no structured content\n"
    );
    assert_eq!(outcome.exit_code, Some(0));
}
