//! `std.mcp`'s `Toolset` (#249): the authority report gives each tool of
//! an implementation its own entry, computed from the literal arms of
//! `call`, and the checked program carries each tool's capabilities. The
//! human and JSON reports are golden-tested in
//! `oskac/tests/toolset_golden.rs`; these cases pin down how a tool's
//! reach is computed.

mod common;

use common::{check, check_modules};
use oska_check::{CheckReport, ToolsetAuthority};

/// A program with `fields` in its toolset, `helpers` in an inherent impl
/// of it, `specs` as the body of `specs`, and `call` as the body of
/// `call`.
fn toolset(fields: &str, helpers: &str, specs: &str, call: &str) -> String {
    format!(
        "use std.json\nuse std.mcp\n\n\
         cap struct Tools {{ {fields} }}\n\n\
         impl mcp.Toolset for Tools {{\n    \
         fn specs(self) -> List<mcp.ToolSpec> {{\n{specs}\n    }}\n\n    \
         fn call(self, name: String, args: json.Value) -> mcp.ToolResult {{\n{call}\n    }}\n}}\n\n\
         impl Tools {{\n{helpers}\n}}\n\n\
         fn shown(io: Stdout, text: String) {{\n    let _ = io.println(text)\n}}\n\n\
         fn listed(dir: DirRead) -> mcp.ToolResult {{\n    mcp.ToolResult.text(\"listed\")\n}}\n"
    )
}

const FIELDS: &str = "notes: DirRead, drafts: DirWrite, out: Stdout, limit: Int";

const NO_HELPERS: &str = "";

/// Specs for `read` and `write`, with literal names.
const SPECS: &str = "        let input = json.Schema.object([])\n        \
     [mcp.ToolSpec.new(\"read\", \"Read.\", input), mcp.ToolSpec.new(\"write\", \"Write.\", input)]";

/// The checked toolset of `src`, which must check cleanly.
fn toolset_of(src: &str) -> (ToolsetAuthority, CheckReport) {
    let report = check(src);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let call = report
        .functions
        .iter()
        .find(|f| f.name == "<Tools as std.mcp.Toolset>.call")
        .unwrap_or_else(|| panic!("no entry for call: {:#?}", report.functions));
    let toolset = call.toolset.clone().expect("call should list its tools");
    (toolset, report)
}

/// Each tool's name and authority.
fn tools(toolset: &ToolsetAuthority) -> Vec<(Option<&str>, Vec<&str>)> {
    toolset
        .tools
        .iter()
        .map(|t| {
            (
                t.name.as_deref(),
                t.authority.iter().map(String::as_str).collect(),
            )
        })
        .collect()
}

#[test]
fn each_literal_arm_reaches_only_the_fields_it_uses() {
    let call = "        match name {\n            \
                \"read\" => listed(self.notes)\n            \
                \"write\" => {\n                let _ = self.drafts\n                \
                mcp.ToolResult.text(\"ok\")\n            }\n            \
                \"count\" => mcp.ToolResult.text(\"{self.limit}\")\n            \
                _ => mcp.ToolResult.error(\"unknown\")\n        }";
    let (toolset, _) = toolset_of(&toolset(FIELDS, NO_HELPERS, SPECS, call));
    assert!(toolset.split);
    assert_eq!(
        tools(&toolset),
        [
            (Some("read"), vec!["DirRead"]),
            (Some("write"), vec!["DirWrite"]),
            (Some("count"), vec![]),
        ]
    );
    assert!(toolset.tools[2].pure);
    assert_eq!(toolset.tools[0].via[0].param, "self.notes");
    assert_eq!(toolset.arms_without_spec, ["count"]);
}

#[test]
fn a_tool_reaches_what_the_methods_it_calls_on_self_reach() {
    let helpers = "    fn read_it(self) -> mcp.ToolResult {\n        self.log()\n        \
                   let _ = self.notes\n        mcp.ToolResult.text(\"ok\")\n    }\n\n    \
                   fn log(self) {\n        shown(self.out, \"read\")\n    }\n\n    \
                   fn again(self) -> mcp.ToolResult {\n        self.again()\n    }";
    let call = "        match name {\n            \
                \"read\" => self.read_it()\n            \
                \"write\" => self.again()\n            \
                _ => mcp.ToolResult.error(\"unknown\")\n        }";
    let (toolset, _) = toolset_of(&toolset(FIELDS, helpers, SPECS, call));
    assert_eq!(
        tools(&toolset),
        [
            (Some("read"), vec!["Stdout", "DirRead"]),
            (Some("write"), vec![]),
        ]
    );
    assert!(toolset.tools[0].writes_protocol_stream());
    assert!(!toolset.tools[1].writes_protocol_stream());
}

#[test]
fn a_tool_that_passes_self_on_whole_reaches_all_of_it() {
    let call = "        match name {\n            \
                \"read\" => everything(self)\n            \
                \"write\" => mcp.ToolResult.text(\"ok\")\n            \
                _ => mcp.ToolResult.error(\"unknown\")\n        }";
    let src = format!(
        "{}\nfn everything(tools: Tools) -> mcp.ToolResult {{\n    mcp.ToolResult.text(\"ok\")\n}}\n",
        toolset(FIELDS, NO_HELPERS, SPECS, call)
    );
    let (toolset, _) = toolset_of(&src);
    assert_eq!(
        tools(&toolset),
        [
            (Some("read"), vec!["DirRead", "DirWrite", "Stdout"]),
            (Some("write"), vec![]),
        ]
    );
    assert_eq!(toolset.tools[0].via[0].param, "self");
}

#[test]
fn statements_before_the_match_count_for_every_tool() {
    let call = "        shown(self.out, \"called\")\n        match name {\n            \
                \"read\" => listed(self.notes)\n            \
                \"write\" => mcp.ToolResult.text(\"ok\")\n            \
                _ => mcp.ToolResult.error(\"unknown\")\n        }";
    let (toolset, _) = toolset_of(&toolset(FIELDS, NO_HELPERS, SPECS, call));
    assert_eq!(
        tools(&toolset),
        [
            (Some("read"), vec!["Stdout", "DirRead"]),
            (Some("write"), vec!["Stdout"]),
        ]
    );
}

#[test]
fn a_spec_with_no_arm_runs_the_default_arm() {
    let call = "        match name {\n            \
                \"read\" => listed(self.notes)\n            \
                other => {\n                let _ = self.drafts\n                \
                mcp.ToolResult.error(\"unknown {other}\")\n            }\n        }";
    let (toolset, _) = toolset_of(&toolset(FIELDS, NO_HELPERS, SPECS, call));
    assert_eq!(
        tools(&toolset),
        [
            (Some("read"), vec!["DirRead"]),
            (Some("write"), vec!["DirWrite"]),
        ]
    );
    assert!(toolset.tools[0].arm);
    assert!(!toolset.tools[1].arm);
    assert!(toolset.arms_without_spec.is_empty());
    assert!(toolset.specs_checked);
}

/// The authority of the default arm of `toolset`, which is split.
fn default_arm(toolset: &ToolsetAuthority) -> Vec<&str> {
    let default = toolset
        .default
        .as_ref()
        .expect("a split toolset has a default arm");
    assert_eq!(default.name, None);
    assert!(!default.arm);
    default.authority.iter().map(String::as_str).collect()
}

#[test]
fn the_default_arm_is_reported_when_every_spec_has_an_arm() {
    let call = "        match name {\n            \
                \"read\" => listed(self.notes)\n            \
                \"write\" => mcp.ToolResult.text(\"ok\")\n            \
                _ => {\n                let _ = self.drafts\n                \
                mcp.ToolResult.error(\"unknown\")\n            }\n        }";
    let (toolset, _) = toolset_of(&toolset(FIELDS, NO_HELPERS, SPECS, call));
    assert_eq!(
        tools(&toolset),
        [(Some("read"), vec!["DirRead"]), (Some("write"), vec![])]
    );
    assert_eq!(default_arm(&toolset), ["DirWrite"]);
}

#[test]
fn specs_with_names_that_are_not_literals_give_no_notes() {
    let specs = "        let input = json.Schema.object([])\n        \
                 let name = \"read\"\n        \
                 [mcp.ToolSpec.new(name, \"Read.\", input), mcp.ToolSpec.new(\"write\", \"Write.\", input)]";
    let call = "        match name {\n            \
                \"read\" => listed(self.notes)\n            \
                \"other\" => mcp.ToolResult.text(\"ok\")\n            \
                _ => mcp.ToolResult.error(\"unknown\")\n        }";
    let (toolset, _) = toolset_of(&toolset(FIELDS, NO_HELPERS, specs, call));
    assert_eq!(
        tools(&toolset),
        [(Some("read"), vec!["DirRead"]), (Some("other"), vec![])]
    );
    assert!(toolset.arms_without_spec.is_empty());
    assert!(!toolset.specs_checked);
}

#[test]
fn the_default_arm_is_reported_when_specs_are_not_literal() {
    let specs = "        let input = json.Schema.object([])\n        \
                 let name = \"read\"\n        \
                 [mcp.ToolSpec.new(name, \"Read.\", input)]";
    let call = "        match name {\n            \
                \"read\" => listed(self.notes)\n            \
                _ => {\n                shown(self.out, \"unknown\")\n                \
                mcp.ToolResult.error(\"unknown\")\n            }\n        }";
    let (toolset, _) = toolset_of(&toolset(FIELDS, NO_HELPERS, specs, call));
    assert_eq!(tools(&toolset), [(Some("read"), vec!["DirRead"])]);
    assert_eq!(default_arm(&toolset), ["Stdout"]);
    assert!(toolset
        .default
        .as_ref()
        .is_some_and(|d| d.writes_protocol_stream()));
}

#[test]
fn a_call_that_is_not_a_match_of_literal_arms_is_not_split() {
    let shapes = [
        // Not a match.
        "        if name == \"read\" {\n            listed(self.notes)\n        \
         } else {\n            mcp.ToolResult.error(\"unknown\")\n        }",
        // A guarded arm may fall through to the next.
        "        match name {\n            \
         \"read\" if self.limit > 0 => listed(self.notes)\n            \
         _ => mcp.ToolResult.error(\"unknown\")\n        }",
        // The name parameter shadowed before the match.
        "        let name = \"read\"\n        match name {\n            \
         \"read\" => listed(self.notes)\n            \
         _ => mcp.ToolResult.error(\"unknown\")\n        }",
        // A match on something other than the name.
        "        let other = \"read\"\n        match other {\n            \
         \"read\" => listed(self.notes)\n            \
         _ => mcp.ToolResult.error(\"unknown\")\n        }",
    ];
    for call in shapes {
        let (toolset, _) = toolset_of(&toolset(FIELDS, NO_HELPERS, SPECS, call));
        assert!(!toolset.split, "{call}");
        assert_eq!(
            tools(&toolset),
            [(None, vec!["DirRead", "DirWrite", "Stdout"])],
            "{call}"
        );
        assert!(toolset.tools[0].writes_protocol_stream());
        assert!(toolset.default.is_none(), "{call}");
    }
}

#[test]
fn the_checked_program_carries_each_tools_capabilities() {
    let call = "        match name {\n            \
                \"read\" => listed(self.notes)\n            \
                _ => mcp.ToolResult.error(\"unknown\")\n        }";
    let (_, report) = toolset_of(&toolset(FIELDS, NO_HELPERS, SPECS, call));
    let program = report.program.expect("a clean program is checked");
    let [toolset] = program.toolsets.as_slice() else {
        panic!("one toolset: {:#?}", program.toolsets);
    };
    assert_eq!(toolset.implementation, "Tools");
    assert_eq!(toolset.capabilities("read"), ["DirRead"]);
    // `write` has no arm: it runs the default arm, which reaches nothing.
    assert!(toolset.capabilities("write").is_empty());
    // A name the report does not split out gets the whole toolset's.
    assert_eq!(
        toolset.capabilities("unknown"),
        ["DirRead", "DirWrite", "Stdout"]
    );
}

#[test]
fn an_unsplit_toolset_gives_every_tool_the_whole_toolsets_capabilities() {
    let call = "        listed(self.notes)";
    let (_, report) = toolset_of(&toolset(FIELDS, NO_HELPERS, SPECS, call));
    let program = report.program.expect("a clean program is checked");
    let toolset = &program.toolsets[0];
    assert_eq!(
        toolset.capabilities("read"),
        ["DirRead", "DirWrite", "Stdout"]
    );
}

#[test]
fn a_tool_that_reaches_a_type_parameter_carries_the_unknown_kind() {
    let src = "use std.json\nuse std.mcp\n\n\
               trait Source {\n    fn read(self) -> String\n}\n\n\
               cap struct Wrap<S: Source> { source: S }\n\n\
               impl<S: Source> mcp.Toolset for Wrap<S> {\n    \
               fn specs(self) -> List<mcp.ToolSpec> {\n        []\n    }\n\n    \
               fn call(self, name: String, args: json.Value) -> mcp.ToolResult {\n        \
               match name {\n            \"read\" => mcp.ToolResult.text(self.source.read())\n            \
               _ => mcp.ToolResult.error(\"unknown\")\n        }\n    }\n}\n";
    let program = check(src).program.expect("a clean program is checked");
    let toolset = &program.toolsets[0];
    assert_eq!(toolset.capabilities("read"), ["?"]);
    // A name the report does not list gets the whole toolset's.
    assert_eq!(toolset.capabilities("other"), ["?"]);
    assert_eq!(toolset.whole, ["?"]);
}

#[test]
fn a_generic_toolset_names_its_bounded_parameter() {
    let src = "use std.json\nuse std.mcp\n\n\
               trait Source {\n    fn read(self) -> String\n}\n\n\
               cap struct Wrap<S: Source> { source: S, out: Stdout }\n\n\
               impl<S: Source> mcp.Toolset for Wrap<S> {\n    \
               fn specs(self) -> List<mcp.ToolSpec> {\n        []\n    }\n\n    \
               fn call(self, name: String, args: json.Value) -> mcp.ToolResult {\n        \
               match name {\n            \"read\" => mcp.ToolResult.text(self.source.read())\n            \
               _ => mcp.ToolResult.error(\"unknown\")\n        }\n    }\n}\n";
    let report = check(src);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let call = report
        .functions
        .iter()
        .find_map(|f| f.toolset.as_ref())
        .expect("call should list its tools");
    let read = &call.tools[0];
    assert!(read.authority.is_empty());
    assert!(!read.pure);
    assert_eq!(read.via[0].param, "self.source");
    assert_eq!(read.via[0].generics, ["S"]);
}

#[test]
fn a_package_toolset_is_split_too() {
    let report = check_modules(&[
        (
            "main",
            "use std.json\nuse std.mcp\nuse pkg.tools\n\n\
             fn main(io: Stdout) {\n    \
             let t = tools.Tools(out: io)\n    \
             let _ = t.call(\"say\", json.Value.Null)\n}\n",
        ),
        (
            "tools",
            "use std.json\nuse std.mcp\n\n\
             pub cap struct Tools { pub out: Stdout }\n\n\
             impl mcp.Toolset for Tools {\n    \
             fn specs(self) -> List<mcp.ToolSpec> {\n        \
             [mcp.ToolSpec.new(\"say\", \"Say.\", json.Schema.object([]))]\n    }\n\n    \
             fn call(self, name: String, args: json.Value) -> mcp.ToolResult {\n        \
             match name {\n            \"say\" => {\n                \
             let _ = self.out.println(\"hi\")\n                \
             mcp.ToolResult.text(\"said\")\n            }\n            \
             _ => mcp.ToolResult.error(\"unknown\")\n        }\n    }\n}\n",
        ),
    ]);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let call = report
        .functions
        .iter()
        .find(|f| f.toolset.is_some())
        .expect("call should list its tools");
    assert_eq!(call.name, "<pkg.tools.Tools as std.mcp.Toolset>.call");
    let toolset = call.toolset.as_ref().expect("checked above");
    assert_eq!(tools(toolset), [(Some("say"), vec!["Stdout"])]);
    let program = report.program.expect("a clean program is checked");
    assert_eq!(program.toolsets[0].capabilities("say"), ["Stdout"]);
}

#[test]
fn an_impl_of_another_trait_named_toolset_is_not_a_toolset() {
    let src = "trait Toolset {\n    fn call(self, name: String) -> Int\n}\n\n\
               cap struct Tools { out: Stdout }\n\n\
               impl Toolset for Tools {\n    fn call(self, name: String) -> Int {\n        \
               match name {\n            \"a\" => 1\n            _ => 0\n        }\n    }\n}\n";
    let report = check(src);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    assert!(report.functions.iter().all(|f| f.toolset.is_none()));
}

/// `std.mcp`'s own source, checked as a program's module.
const MCP_SOURCE: &str = include_str!("../src/std/mcp.osk");

#[test]
fn the_module_adds_no_capability_and_vouches_for_nothing() {
    // The authority and audit reports leave a `std` module written in
    // Oska out, so this checks it as code of its own. It holds no
    // capability: `serve` is given standard input and output, and the
    // functions that take a toolset have only what its type argument
    // holds (`generic T (tools)`).
    let report = check(MCP_SOURCE);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let reaching: Vec<(&str, &[String])> = report
        .functions
        .iter()
        .filter(|f| !f.authority.is_empty())
        .map(|f| (f.name.as_str(), f.authority.as_slice()))
        .collect();
    assert_eq!(
        reaching,
        [(
            "serve",
            ["Stdin".to_string(), "Stdout".to_string()].as_slice()
        )]
    );
    assert!(report.audit.is_empty(), "{:#?}", report.audit);
}

#[test]
fn serve_and_handle_are_generic_over_the_toolset_and_never_pure() {
    let report = check(MCP_SOURCE);
    let human: Vec<String> = report
        .functions
        .iter()
        .filter(|f| matches!(f.name.as_str(), "serve" | "handle" | "validate"))
        .map(|f| f.to_human())
        .collect();
    assert_eq!(
        human,
        [
            "pub fn validate: generic T (tools)",
            "pub fn serve: Stdin (stdin), Stdout (stdout), generic T (tools)",
            "pub fn handle: generic T (tools)",
        ]
    );
}

#[test]
fn a_tool_result_keeps_its_text_untrusted() {
    let src = "use std.json\nuse std.mcp\n\n\
               fn run(text: String) {}\n\n\
               fn f(r: mcp.ToolResult) {\n    run(r.content())\n}\n";
    let report = check(src);
    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, ["E0300"], "{:#?}", report.diagnostics);
}

#[test]
fn a_tool_spec_and_result_are_built_only_by_their_functions() {
    let report = check(
        "use std.json\nuse std.mcp\n\n\
         fn f() -> mcp.ToolResult {\n    \
         mcp.ToolResult(content: \"x\", structured: None, failed: false)\n}\n",
    );
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.message.contains("private to `std.mcp`")),
        "{:#?}",
        report.diagnostics
    );
}

#[test]
fn a_std_item_in_a_trait_argument_is_named_by_its_std_path() {
    let src = "use std.json\n\n\
               trait Holds<T> {\n    fn held(self) -> T\n}\n\n\
               struct Doc { value: json.Value }\n\n\
               impl Holds<json.Value> for Doc {\n    fn held(self) -> json.Value {\n        self.value\n    }\n}\n";
    let report = check(src);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let names: Vec<&str> = report.functions.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["<Doc as Holds<std.json.Value>>.held"]);
}
