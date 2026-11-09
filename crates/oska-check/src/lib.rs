//! Type checker for the Oska programming language.
//!
//! [`check_module`] checks one source file against the embedded
//! **prelude** (`src/prelude.osk`) and returns a [`CheckReport`]: every
//! diagnostic, plus the **authority** of every function. The report is
//! produced even when there are errors. [`CheckReport::to_json`] is what
//! `oskac check --format=json` prints; `docs/checker.md` describes it.
//!
//! The pipeline runs in order: collect declarations, resolve names, then
//! compute authority. Function bodies are not checked yet.
//!
//! # Diagnostic codes
//!
//! Checker codes are `E02xx`. Each is listed here and in
//! `docs/checker.md`.
//!
//! | Code | Meaning |
//! | --- | --- |
//! | `E0200` | Unknown type or trait name. The fix replaces it with the single closest name, when there is one. |
//! | `E0201` | A trait used where a type is expected, or a type used where a trait is expected. |
//! | `E0202` | A type or trait name declared twice, or one that redeclares a prelude name. |

use oska_syntax::Diagnostic;

mod authority;
mod declarations;
mod decls;
mod prelude;
mod resolve;

/// The diagnostic codes this crate reports.
pub(crate) mod codes {
    pub const UNKNOWN_NAME: &str = "E0200";
    pub const WRONG_NAMESPACE: &str = "E0201";
    pub const DUPLICATE_NAME: &str = "E0202";
    pub const CAP_REQUIRED: &str = "E0210";
    pub const MAIN_PARAM: &str = "E0211";
    pub const MAIN_RETURN: &str = "E0212";
}

/// Options that change what is checked.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CheckOptions {
    /// Checking for a release build: `--release`.
    pub release: bool,
}

/// The result of checking a module.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CheckReport {
    /// Every diagnostic, in source order.
    pub diagnostics: Vec<Diagnostic>,
    /// One entry per function and `impl` method, in source order.
    pub functions: Vec<FunctionAuthority>,
}

/// One function's entry in the authority report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionAuthority {
    /// The function's name, or `Type.method` for a method.
    pub name: String,
    /// The built-in capabilities the function can reach, in order of first
    /// appearance. Attenuated capabilities keep their names, such as
    /// `FsRead`.
    pub authority: Vec<String>,
    /// The capability-bearing parameters, the receiver (`self`) included,
    /// and what each supplies.
    pub via: Vec<Via>,
    /// No parameter is capability-bearing.
    pub pure: bool,
}

/// A capability-bearing parameter and the capabilities it supplies. A
/// `cap fn` parameter is capability-bearing but names no capabilities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Via {
    pub param: String,
    pub capabilities: Vec<String>,
}

impl CheckReport {
    /// Render as `{"diagnostics": [...], "functions": [...]}`, with one
    /// diagnostic or function per line.
    pub fn to_json(&self) -> String {
        let diagnostics: Vec<String> = self.diagnostics.iter().map(Diagnostic::to_json).collect();
        let functions: Vec<String> = self
            .functions
            .iter()
            .map(FunctionAuthority::to_json)
            .collect();
        format!(
            "{{\n  \"diagnostics\": {},\n  \"functions\": {}\n}}\n",
            json_lines(&diagnostics),
            json_lines(&functions)
        )
    }
}

impl FunctionAuthority {
    /// Render as a single-line JSON object. Names are identifiers, so they
    /// need no escaping.
    pub fn to_json(&self) -> String {
        let via: Vec<String> = self
            .via
            .iter()
            .map(|v| {
                format!(
                    "{{\"param\":\"{}\",\"capabilities\":{}}}",
                    v.param,
                    json_strings(&v.capabilities)
                )
            })
            .collect();
        format!(
            "{{\"name\":\"{}\",\"authority\":{},\"via\":[{}],\"pure\":{}}}",
            self.name,
            json_strings(&self.authority),
            via.join(","),
            self.pure
        )
    }
}

fn json_strings(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|s| format!("\"{s}\"")).collect();
    format!("[{}]", quoted.join(","))
}

/// A JSON array with one element per line.
fn json_lines(items: &[String]) -> String {
    if items.is_empty() {
        return "[]".to_string();
    }
    format!("[\n    {}\n  ]", items.join(",\n    "))
}

/// Check `src` as a module, with default options.
pub fn check_module(src: &str) -> CheckReport {
    check_module_with(src, CheckOptions::default())
}

/// Check `src` as a module. A parse error is reported as the only
/// diagnostic, with no functions.
pub fn check_module_with(src: &str, _options: CheckOptions) -> CheckReport {
    let module = match oska_syntax::parse_module(src) {
        Ok(module) => module,
        Err(err) => {
            return CheckReport {
                diagnostics: vec![err],
                functions: Vec::new(),
            }
        }
    };
    let mut diagnostics = Vec::new();
    let decls = decls::Decls::collect(&module, &mut diagnostics);
    resolve::module(&module, &decls, &mut diagnostics);
    declarations::module(src, &module, &decls, &mut diagnostics);
    let functions = authority::report(&module, &decls);
    diagnostics.sort_by_key(|d| d.span.start);
    CheckReport {
        diagnostics,
        functions,
    }
}
