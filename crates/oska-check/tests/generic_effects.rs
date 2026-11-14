//! #149: a function that can perform effects through a type parameter is
//! not reported pure. A parameter whose type is, or holds, a bounded type
//! parameter is authority-carrying: the type argument may be capability-
//! bearing, and the bound gives the body methods to call on it. The report
//! names it `generic T (param)`, the way it names a `cap fn` parameter.

mod common;

use common::{assert_ok, check};

/// The line the human report gives the function `name`.
fn human_line(src: &str, name: &str) -> String {
    let report = check(src);
    report
        .functions
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("{src:?} should report a function `{name}`"))
        .to_human()
}

/// The JSON entry the report gives the function `name`.
fn json_entry(src: &str, name: &str) -> String {
    let report = check(src);
    report
        .functions
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("{src:?} should report a function `{name}`"))
        .to_json()
}

/// The program from #149: a generic agent loop reads files through its
/// type argument.
const TOOLSET: &str = "trait Toolset {
    fn call(self, name: String) -> Result<Untrusted<String>, IoError>
}

cap struct NoteTools {
    notes: DirRead,
}

impl Toolset for NoteTools {
    fn call(self, name: String) -> Result<Untrusted<String>, IoError> {
        self.notes.read_to_string(Path.new(name))
    }
}

fn run<T: Toolset>(tools: T) -> Result<Untrusted<String>, IoError> {
    tools.call(\"a.txt\")
}

fn main(io: Stdout, fs: FsRead) -> Result<(), IoError> {
    let tools = NoteTools(notes: fs.restrict_to(\"notes\"))
    let text = run(tools)?
    io.println(text.assume_trusted())
}
";

#[test]
fn a_function_calling_a_method_on_a_bounded_type_parameter_is_not_pure() {
    assert_ok(TOOLSET);
    assert_eq!(human_line(TOOLSET, "run"), "fn run: generic T (tools)");
}

#[test]
fn json_names_the_type_parameter_a_parameter_carries_authority_through() {
    assert_eq!(
        json_entry(TOOLSET, "run"),
        r#"{"name":"run","authority":[],"via":[{"param":"tools","capabilities":[],"generics":["T"]}],"pure":false}"#
    );
}

#[test]
fn a_function_whose_type_parameters_are_unbounded_stays_pure() {
    let src = "fn first<T>(xs: List<T>, fallback: T) -> T {
    match xs.get(0) {
        Some(x) => x
        None => fallback
    }
}
";
    assert_ok(src);
    assert_eq!(human_line(src, "first"), "fn first: pure");
}

#[test]
fn a_bounded_type_parameter_no_parameter_holds_stays_pure() {
    let src = format!("{TOOLSET}\nfn count<T: Toolset>(n: Int) -> Int {{\n    n\n}}\n");
    assert_ok(&src);
    assert_eq!(human_line(&src, "count"), "fn count: pure");
}

#[test]
fn a_parameter_holding_a_bounded_type_parameter_carries_authority() {
    let src = format!(
        "{TOOLSET}
fn run_first<T: Toolset>(all: List<T>) -> Result<Untrusted<String>, IoError> {{
    match all.get(0) {{
        Some(tools) => tools.call(\"a.txt\")
        None => Ok(\"\")
    }}
}}
"
    );
    assert_ok(&src);
    assert_eq!(
        human_line(&src, "run_first"),
        "fn run_first: generic T (all)"
    );
}

#[test]
fn a_receiver_holding_an_impl_type_parameter_carries_authority() {
    let src = format!(
        "{TOOLSET}
struct Agent<T> {{
    tools: T,
}}

impl<T: Toolset> Agent<T> {{
    fn step(self) -> Result<Untrusted<String>, IoError> {{
        self.tools.call(\"a.txt\")
    }}
}}
"
    );
    assert_ok(&src);
    assert_eq!(
        human_line(&src, "Agent.step"),
        "fn Agent.step: generic T (self)"
    );
}

#[test]
fn capabilities_and_type_parameters_are_listed_together() {
    let src = format!(
        "{TOOLSET}
fn log_run<T: Toolset>(out: Stdout, tools: T) -> Result<(), IoError> {{
    out.println(tools.call(\"a.txt\")?)
}}
"
    );
    assert_ok(&src);
    assert_eq!(
        human_line(&src, "log_run"),
        "fn log_run: Stdout (out), generic T (tools)"
    );
}

#[test]
fn a_trait_default_method_carries_authority_through_self() {
    let src = "trait Toolset {
    fn call(self, name: String) -> Result<Untrusted<String>, IoError>

    fn call_twice(self) -> Result<Untrusted<String>, IoError> {
        let _ = self.call(\"a.txt\")?
        self.call(\"b.txt\")
    }
}
";
    assert_ok(src);
    assert_eq!(
        human_line(src, "Toolset.call_twice"),
        "fn Toolset.call_twice: generic Self (self)"
    );
}
