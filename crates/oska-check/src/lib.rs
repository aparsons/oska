//! Type checker for the Oska programming language.
//!
//! [`check_module`] checks one source file against the embedded
//! **prelude** (`src/prelude.osk`) and returns a [`CheckReport`]: every
//! diagnostic, the **authority** of every function, every **audit
//! site** (a call a reviewer must see, such as `.assume_trusted()`:
//! `body/audit.rs`), and every **attenuation** (a call that narrows a
//! capability, such as `.restrict_to("docs")`: `body/attenuation.rs`).
//! The report is
//! produced even when there are errors. [`CheckReport::to_json`] is what
//! `oskac check --format=json` prints; `docs/checker.md` describes it.
//! A report with no diagnostics also carries the **checked program**
//! ([`program`]) that backends such as `oska-run` execute (ADR 0007);
//! [`builtins`] tells them what the prelude's built-in types are.
//! A `use std.*` adds that module's declarations (`src/std/*.osk`) to
//! the prelude the file sees.
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
//! Checker codes are `E02xx` and `E03xx`, defined in the `codes` module
//! below. The canonical table of what each means, what it spans, and
//! which carry a machine-applicable fix is in `docs/checker.md`; a
//! conformance test keeps it in step with `codes`.

use oska_syntax::{json_escape, Diagnostic, Span};

mod authority;
mod body;
pub mod builtins;
mod coherence;
mod collect;
mod decl_rules;
mod infer;
mod items;
mod prelude;
pub mod program;
mod recursion;
mod release;
mod resolve;
mod suggest;
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
    pub const SCOPE_ESCAPES: &str = "E0292";
    pub const TODO_IN_RELEASE: &str = "E0293";
    pub const USE_OUTSIDE_STD: &str = "E0294";
    pub const UNKNOWN_MODULE: &str = "E0295";
    pub const UNTRUSTED_TEXT: &str = "E0300";
    pub const SINK_NOT_IMPORTED: &str = "E0310";
    pub const UNKNOWN_SINK_TAG: &str = "E0311";
    pub const NOT_EXPORTED: &str = "E0312";
    pub const UNTRUSTED_NOT_TEXT: &str = "E0320";
    pub const QUALIFIED_ONLY: &str = "E0340";
    pub const TEXT_AS_SINK_LITERAL: &str = "E0341";
    pub const SECRET_INTERPOLATED: &str = "E0350";
    pub const SECRET_COMPARED: &str = "E0351";
    pub const SECRET_DISPLAYED: &str = "E0352";
    pub const PRIVATE_TO_MODULE: &str = "E0360";
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
    /// Every audit site, in source order.
    pub audit: Vec<AuditSite>,
    /// Every attenuation of a capability, in source order.
    pub attenuations: Vec<Attenuation>,
    /// The **checked program** a backend runs (`program.rs`): present
    /// only when there are no diagnostics.
    pub program: Option<program::Program>,
}

/// A call a reviewer must see: one that vouches for untrusted text, or
/// exposes a secret. `body/audit.rs` decides which calls these are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditSite {
    /// What the call does.
    pub kind: AuditKind,
    /// The whole call expression.
    pub span: Span,
}

/// What an audit site does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditKind {
    /// Vouches for untrusted text: `.assume_trusted()`.
    TrustAssertion,
    /// Reads a secret's value: `.expose()`.
    SecretExposure,
}

impl AuditKind {
    /// The kind as the report names it: the method's name, such as
    /// `assume_trusted`.
    pub fn as_str(self) -> &'static str {
        match self {
            AuditKind::TrustAssertion => "assume_trusted",
            AuditKind::SecretExposure => "expose",
        }
    }
}

/// Renders as `audit: assume_trusted (bytes 52..73)`, as the
/// human-readable report lists it.
impl std::fmt::Display for AuditSite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "audit: {} (bytes {}..{})",
            self.kind.as_str(),
            self.span.start,
            self.span.end
        )
    }
}

impl AuditSite {
    /// Render as a single-line JSON object.
    pub fn to_json(&self) -> String {
        format!(
            "{{\"kind\":\"{}\",\"span\":{{\"start\":{},\"end\":{}}}}}",
            self.kind.as_str(),
            self.span.start,
            self.span.end
        )
    }
}

/// One attenuation: a call of a prelude method that narrows a
/// capability, such as `read.restrict_to("docs")` or `dir.read_only()`.
/// `body/attenuation.rs` decides which calls these are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attenuation {
    /// The function the call is in, named as in `functions`. A closure's
    /// attenuations belong to the function around it.
    pub function: String,
    /// The method: `restrict_to`, `restrict_to_file`, `read_only` or
    /// `write_only`.
    pub method: String,
    /// The whole call expression.
    pub span: Span,
    /// The capability kind the method is called on, such as `FsRead`.
    pub source: String,
    /// The capability kind it returns, such as `DirRead`.
    pub result: String,
    /// The root it narrows to.
    pub root: AttenuationRoot,
}

/// The root an attenuation narrows to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttenuationRoot {
    /// The method takes no root: `read_only` and `write_only`.
    Absent,
    /// A string literal or a module constant, with its text. Narrowing a
    /// directory capability gives the path relative to its own root, as
    /// written.
    Literal(String),
    /// Anything else: only known when the program runs.
    Computed,
}

impl Attenuation {
    /// The root as the JSON report names it: its text, or `None` when the
    /// method takes none or the root is computed at runtime.
    pub fn root_text(&self) -> Option<&str> {
        match &self.root {
            AttenuationRoot::Literal(text) => Some(text),
            AttenuationRoot::Absent | AttenuationRoot::Computed => None,
        }
    }

    /// One line of the human-readable report, such as
    /// `FsRead -> DirRead: restrict_to "docs" (bytes 52..73)`.
    pub fn to_human(&self) -> String {
        let root = match &self.root {
            AttenuationRoot::Absent => String::new(),
            AttenuationRoot::Literal(text) => format!(" {text:?}"),
            AttenuationRoot::Computed => " computed at runtime".to_string(),
        };
        format!(
            "{} -> {}: {}{root} (bytes {}..{})",
            self.source, self.result, self.method, self.span.start, self.span.end
        )
    }

    /// Render as a single-line JSON object.
    pub fn to_json(&self) -> String {
        let root = self.root_text().map_or_else(
            || "null".to_string(),
            |text| format!("\"{}\"", json_escape(text)),
        );
        format!(
            "{{\"function\":\"{}\",\"method\":\"{}\",\"span\":{{\"start\":{},\"end\":{}}},\"source\":\"{}\",\"result\":\"{}\",\"root\":{root}}}",
            json_escape(&self.function),
            json_escape(&self.method),
            self.span.start,
            self.span.end,
            json_escape(&self.source),
            json_escape(&self.result)
        )
    }
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
    /// Render as `{"diagnostics": [...], "functions": [...], "audit":
    /// [...], "attenuations": [...]}`, with one diagnostic, function,
    /// audit site, or attenuation per line. Every array is present, even
    /// when empty.
    pub fn to_json(&self) -> String {
        let diagnostics: Vec<String> = self.diagnostics.iter().map(Diagnostic::to_json).collect();
        let functions: Vec<String> = self
            .functions
            .iter()
            .map(FunctionAuthority::to_json)
            .collect();
        let audit: Vec<String> = self.audit.iter().map(AuditSite::to_json).collect();
        let attenuations: Vec<String> =
            self.attenuations.iter().map(Attenuation::to_json).collect();
        format!(
            "{{\n  \"diagnostics\": {},\n  \"functions\": {},\n  \"audit\": {},\n  \"attenuations\": {}\n}}\n",
            json_lines(&diagnostics),
            json_lines(&functions),
            json_lines(&audit),
            json_lines(&attenuations)
        )
    }

    /// The authority report as `oskac check <path>` prints it on standard
    /// output: one line per function and method, each followed by its
    /// attenuations (indented), then one line per audit site, each ending
    /// in a newline. Diagnostics are not part of it.
    pub fn authority_to_human(&self, path: &str) -> String {
        let mut out = String::new();
        let mut listed = vec![false; self.attenuations.len()];
        for function in &self.functions {
            out.push_str(&function.to_human());
            out.push('\n');
            self.list_attenuations(&function.name, &mut listed, &mut out);
        }
        // A function the report has no line for, such as a trait's
        // default method, still shows what it attenuates.
        for i in 0..listed.len() {
            if !listed[i] {
                let name = &self.attenuations[i].function;
                out.push_str(&format!("fn {name}:\n"));
                self.list_attenuations(name, &mut listed, &mut out);
            }
        }
        for site in &self.audit {
            out.push_str(&format!("{path}: {site}\n"));
        }
        out
    }
}

impl CheckReport {
    /// Append the attenuations of the function `name` that are not yet
    /// `listed`, one indented line each, and mark them listed.
    fn list_attenuations(&self, name: &str, listed: &mut [bool], out: &mut String) {
        for (i, attenuation) in self.attenuations.iter().enumerate() {
            if !listed[i] && attenuation.function == name {
                listed[i] = true;
                out.push_str(&format!("  {}\n", attenuation.to_human()));
            }
        }
    }
}

impl FunctionAuthority {
    /// One line of the human-readable authority report, such as
    /// `fn main: Stdout (io), FsRead (fs)` or `fn add: pure`.
    pub fn to_human(&self) -> String {
        if self.pure {
            return format!("fn {}: pure", self.name);
        }
        let sources: Vec<String> = self
            .via
            .iter()
            .map(|via| {
                if via.capabilities.is_empty() {
                    format!("cap fn ({})", via.param)
                } else {
                    format!("{} ({})", via.capabilities.join(", "), via.param)
                }
            })
            .collect();
        format!("fn {}: {}", self.name, sources.join(", "))
    }

    /// Render as a single-line JSON object.
    pub fn to_json(&self) -> String {
        let via: Vec<String> = self
            .via
            .iter()
            .map(|v| {
                format!(
                    "{{\"param\":\"{}\",\"capabilities\":{}}}",
                    json_escape(&v.param),
                    json_strings(&v.capabilities)
                )
            })
            .collect();
        format!(
            "{{\"name\":\"{}\",\"authority\":{},\"via\":[{}],\"pure\":{}}}",
            json_escape(&self.name),
            json_strings(&self.authority),
            via.join(","),
            self.pure
        )
    }
}

fn json_strings(items: &[String]) -> String {
    let quoted: Vec<String> = items
        .iter()
        .map(|s| format!("\"{}\"", json_escape(s)))
        .collect();
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
                ..CheckReport::default()
            }
        }
    };
    let mut diagnostics = Vec::new();
    let decls = collect::Decls::collect(&module, &mut diagnostics);
    resolve::module(&module, &decls, &mut diagnostics);
    decl_rules::module(src, &module, &decls, &mut diagnostics);
    coherence::module(&module, &decls, &mut diagnostics);
    recursion::module(&module, &decls, &mut diagnostics);
    if options.release {
        release::module(&module, &mut diagnostics);
    }
    let items = items::Items::collect(&module, &decls);
    traits::module(src, &module, &decls, &items, &mut diagnostics);
    let mut findings = body::Findings {
        diagnostics,
        ..body::Findings::default()
    };
    body::module(src, &module, &decls, &items, &mut findings);
    let body::Findings {
        mut diagnostics,
        mut audit,
        mut attenuations,
        resolutions,
        ..
    } = findings;
    let program = diagnostics
        .is_empty()
        .then(|| program::lower::module(&module, &items, &resolutions));
    let functions = authority::report(&module, &decls);
    diagnostics.sort_by_key(|d| d.span.start);
    audit.sort_by_key(|site| site.span.start);
    // A chain shares its start: the inner call, which ends first, is first.
    attenuations.sort_by_key(|a| (a.span.start, a.span.end));
    CheckReport {
        diagnostics,
        functions,
        audit,
        attenuations,
        program,
    }
}
