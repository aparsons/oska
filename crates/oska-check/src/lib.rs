//! Type checker for the Oska programming language.
//!
//! [`check_module`] checks one source file against the embedded
//! **prelude** (`src/prelude.osk`) and returns a [`CheckReport`]: every
//! diagnostic, plus the **authority** of every function. The report is
//! produced even when there are errors. [`CheckReport::to_json`] is what
//! `oskac check --format=json` prints; `docs/checker.md` describes it.
//!
//! The pipeline runs in order: collect declarations, resolve names, check
//! declarations (`cap`, `main`, capitalization, recursive types, where
//! `Scope` may appear: `decl_rules.rs`), check that functions, methods,
//! and impls are unique and that inherent impls are for the module's own
//! types (`coherence.rs`), check trait impls, check function, test, and
//! contract bodies (`body.rs`, with local bidirectional inference), then
//! compute authority. A test body sees `t: TestContext` from the prelude.
//! Under `--release`, bodies are also scanned for `dbg(...)` and
//! `todo()`.
//!
//! # Diagnostic codes
//!
//! Checker codes are `E02xx`, defined in the `codes` module below. The
//! canonical table of what each means, what it spans, and which carry a
//! machine-applicable fix is in `docs/checker.md`; a conformance test
//! keeps it in step with `codes`.

use oska_syntax::Diagnostic;

mod authority;
mod body;
mod coherence;
mod collect;
mod decl_rules;
mod infer;
mod items;
mod prelude;
mod recursion;
mod release;
mod resolve;
mod traits;
mod ty;
mod usefulness;

/// The diagnostic codes this crate reports.
pub(crate) mod codes {
    pub const UNKNOWN_NAME: &str = "E0200";
    pub const WRONG_NAMESPACE: &str = "E0201";
    pub const DUPLICATE_NAME: &str = "E0202";
    pub const CAP_REQUIRED: &str = "E0210";
    pub const MAIN_PARAM: &str = "E0211";
    pub const MAIN_RETURN: &str = "E0212";
    pub const NOT_UPPERCASE: &str = "E0213";
    pub const NOT_LOWERCASE: &str = "E0214";
    pub const NO_BASE_CASE: &str = "E0215";
    pub const DBG_IN_RELEASE: &str = "E0216";
    pub const MISMATCH: &str = "E0220";
    pub const UNKNOWN_VALUE: &str = "E0221";
    pub const LITERAL_OUT_OF_RANGE: &str = "E0222";
    pub const BAD_OPERAND: &str = "E0223";
    pub const ARGUMENT_COUNT: &str = "E0224";
    pub const BAD_LABEL: &str = "E0225";
    pub const MISSING_LABEL: &str = "E0226";
    pub const NO_FIELD: &str = "E0227";
    pub const NO_MEMBER: &str = "E0228";
    pub const ASSIGN_TO_LET: &str = "E0229";
    pub const MUTATING_CALL_ON_LET: &str = "E0230";
    pub const NOT_CALLABLE: &str = "E0231";
    pub const OUTSIDE_LOOP: &str = "E0232";
    pub const CANNOT_INFER: &str = "E0233";
    pub const NOT_A_VALUE: &str = "E0234";
    pub const NOT_ITERABLE: &str = "E0235";
    pub const IGNORED_RESULT: &str = "E0240";
    pub const TRY_OPERAND: &str = "E0241";
    pub const TRY_ERROR_TYPE: &str = "E0242";
    pub const TRY_KIND: &str = "E0243";
    pub const UNKNOWN_VARIANT: &str = "E0250";
    pub const PATTERN_FIELD_COUNT: &str = "E0251";
    pub const PATTERN_LABEL: &str = "E0252";
    pub const NOT_EXHAUSTIVE: &str = "E0253";
    pub const UNREACHABLE_ARM: &str = "E0254";
    pub const NOT_IN_BOUNDS: &str = "E0260";
    pub const MISSING_METHOD: &str = "E0261";
    pub const IMPL_SIGNATURE: &str = "E0262";
    pub const NOT_IN_TRAIT: &str = "E0263";
    pub const AMBIGUOUS_METHOD: &str = "E0264";
    pub const NOT_IMPLEMENTED: &str = "E0265";
    pub const CAPTURING_CLOSURE: &str = "E0270";
    pub const FORGED: &str = "E0271";
    pub const PRIVATE_FIELD: &str = "E0272";
    pub const IMPURE_IN_CONTRACT: &str = "E0280";
    pub const CONTRACT_NOT_BOOL: &str = "E0281";
    pub const CONFLICTING_IMPL: &str = "E0290";
    pub const FOREIGN_INHERENT_IMPL: &str = "E0291";
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
pub fn check_module_with(src: &str, options: CheckOptions) -> CheckReport {
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
    let decls = collect::Decls::collect(&module, &mut diagnostics);
    resolve::module(&module, &decls, &mut diagnostics);
    decl_rules::module(src, &module, &decls, &mut diagnostics);
    coherence::module(&module, &decls, &mut diagnostics);
    recursion::module(&module, &mut diagnostics);
    if options.release {
        release::module(&module, &mut diagnostics);
    }
    let items = items::Items::collect(&module, &decls);
    traits::module(&module, &decls, &items, &mut diagnostics);
    body::module(src, &module, &decls, &items, &mut diagnostics);
    let functions = authority::report(&module, &decls);
    diagnostics.sort_by_key(|d| d.span.start);
    CheckReport {
        diagnostics,
        functions,
    }
}
