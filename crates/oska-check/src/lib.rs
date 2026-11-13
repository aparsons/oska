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
//! [`check_package`] checks a [`Package`], every module of a program
//! with its manifest, and returns one report in which every diagnostic,
//! audit site and attenuation names its file (`package.rs`).
//! [`check_file`], what `oskac check <file.osk>` runs, is a package of
//! one module. [`check_module`] checks a source string alone, naming no
//! file.
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

use oska_syntax::ast::Module;
use oska_syntax::{json_escape, Diagnostic, Location, Span};

mod authority;
mod body;
pub mod builtins;
mod coherence;
mod collect;
mod decl_rules;
mod infer;
mod instantiation;
mod items;
mod link;
mod manifest;
mod package;
mod prelude;
pub mod program;
mod recursion;
mod release;
mod resolve;
mod suggest;
mod toolchain;
mod traits;
mod ty;
mod usefulness;

pub use manifest::{decode_manifest, parse_manifest, Manifest};
pub use package::{
    check_file, check_package, not_a_package, run_of_library, symlink_refused, Locator,
    ModuleSource, Package, ENTRY_MODULE, MANIFEST_PATH,
};
pub use toolchain::{
    debug_escapes, declared_intrinsics as std_rt_intrinsics, is_intrinsic,
    RT_SOURCE as STD_RT_SOURCE,
};

/// The diagnostic codes this crate reports.
pub(crate) mod codes {
    pub const UNKNOWN_NAME: &str = "E0200";
    pub const WRONG_NAMESPACE: &str = "E0201";
    pub const DUPLICATE_NAME: &str = "E0202";
    pub const PKG_OUTSIDE_PACKAGE: &str = "E0203";
    pub const UNKNOWN_PKG_MODULE: &str = "E0204";
    pub const NOT_IN_MODULE: &str = "E0205";
    pub const PRIVATE_ITEM: &str = "E0206";
    pub const IMPORT_CYCLE: &str = "E0207";
    pub const NAME_BOUND_TWICE: &str = "E0208";
    pub const TOOLCHAIN_ONLY: &str = "E0209";
    pub const CAP_REQUIRED: &str = "E0210";
    pub const MAIN_PARAM: &str = "E0211";
    pub const MAIN_RETURN: &str = "E0212";
    pub const NOT_UPPERCASE: &str = "E0213";
    pub const NOT_LOWERCASE: &str = "E0214";
    pub const NO_BASE_CASE: &str = "E0215";
    pub const DBG_IN_RELEASE: &str = "E0216";
    pub const MAIN_CONFINED: &str = "E0217";
    pub const IMPL_CONTRACT: &str = "E0218";
    pub const MAIN_CONTRACT: &str = "E0219";
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
    pub const PATH_METHOD_ON_FS: &str = "E0273";
    pub const BREAKS_TYPE_PARAMETER: &str = "E0274";
    pub const IMPURE_IN_CONTRACT: &str = "E0280";
    pub const CONTRACT_NOT_BOOL: &str = "E0281";
    pub const LEAVES_CONTRACT: &str = "E0282";
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
    pub const MANIFEST_SYNTAX: &str = "E0400";
    pub const MANIFEST_UNSUPPORTED: &str = "E0401";
    pub const MANIFEST_UNKNOWN: &str = "E0402";
    pub const MANIFEST_DEPENDENCIES: &str = "E0403";
    pub const MANIFEST_MISSING: &str = "E0404";
    pub const MANIFEST_VALUE: &str = "E0405";
    pub const MANIFEST_DUPLICATE: &str = "E0406";
    pub const BAD_MODULE_NAME: &str = "E0410";
    pub const SYMLINK_IN_SOURCE: &str = "E0411";
    pub const MAIN_OUTSIDE_ENTRY: &str = "E0412";
    pub const NOT_A_PACKAGE: &str = "E0413";
    pub const RUN_OF_LIBRARY: &str = "E0414";
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditSite {
    /// What the call does.
    pub kind: AuditKind,
    /// The whole call expression.
    pub span: Span,
    /// The file the call is in, named by a package check.
    pub location: Option<Location>,
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
    /// Render as a single-line JSON object, with a `path` before `span`
    /// when the site names its file.
    pub fn to_json(&self) -> String {
        format!(
            "{{\"kind\":\"{}\",{}\"span\":{{\"start\":{},\"end\":{}}}}}",
            self.kind.as_str(),
            path_json(&self.location),
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
    /// The file the call is in, named by a package check.
    pub location: Option<Location>,
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
    /// `FsRead -> DirRead: restrict_to "docs" (bytes 52..73)`, after
    /// `path:line:col: ` when the attenuation names its file.
    pub fn to_human(&self) -> String {
        let root = match &self.root {
            AttenuationRoot::Absent => String::new(),
            AttenuationRoot::Literal(text) => format!(" {text:?}"),
            AttenuationRoot::Computed => " computed at runtime".to_string(),
        };
        let location = self
            .location
            .as_ref()
            .map_or_else(String::new, |l| format!("{l}: "));
        format!(
            "{location}{} -> {}: {}{root} (bytes {}..{})",
            self.source, self.result, self.method, self.span.start, self.span.end
        )
    }

    /// Render as a single-line JSON object, with a `path` before `span`
    /// when the attenuation names its file.
    pub fn to_json(&self) -> String {
        let root = self.root_text().map_or_else(
            || "null".to_string(),
            |text| format!("\"{}\"", json_escape(text)),
        );
        format!(
            "{{\"function\":\"{}\",\"method\":\"{}\",{}\"span\":{{\"start\":{},\"end\":{}}},\"source\":\"{}\",\"result\":\"{}\",\"root\":{root}}}",
            json_escape(&self.function),
            json_escape(&self.method),
            path_json(&self.location),
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
    /// The function's name, `Type.method` for a method of an inherent
    /// `impl`, or `<Type as Trait>.method` for one of `impl Trait for
    /// Type`. In a package, `pkg.` and the module path come first, as in
    /// `pkg.util.text.shout` (see `link::Modules::function_name` and
    /// `trait_method_name`). No two functions share a name.
    pub name: String,
    /// Declared `pub`: part of its package's API.
    pub public: bool,
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
/// A parameter whose type is or holds a bounded type parameter names the
/// parameter in `generics`: its type argument may be capability-bearing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Via {
    pub param: String,
    pub capabilities: Vec<String>,
    /// The bounded type parameters the parameter's type is or holds, in
    /// order of first appearance.
    pub generics: Vec<String>,
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
    /// in a newline. Diagnostics are not part of it. An audit line starts
    /// with its location, `path:line:col`, when the report names its file
    /// (a package check), and with `path` otherwise.
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
            match &site.location {
                Some(location) => out.push_str(&format!("{location}: {site}\n")),
                None => out.push_str(&format!("{path}: {site}\n")),
            }
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
            return format!("{}fn {}: pure", self.pub_marker(), self.name);
        }
        let sources: Vec<String> = self
            .via
            .iter()
            .map(|via| {
                let mut kinds = via.capabilities.clone();
                kinds.extend(via.generics.iter().map(|g| format!("generic {g}")));
                if kinds.is_empty() {
                    format!("cap fn ({})", via.param)
                } else {
                    format!("{} ({})", kinds.join(", "), via.param)
                }
            })
            .collect();
        format!(
            "{}fn {}: {}",
            self.pub_marker(),
            self.name,
            sources.join(", ")
        )
    }

    fn pub_marker(&self) -> &'static str {
        if self.public {
            "pub "
        } else {
            ""
        }
    }

    /// Render as a single-line JSON object.
    pub fn to_json(&self) -> String {
        let via: Vec<String> = self
            .via
            .iter()
            .map(|v| {
                let generics = if v.generics.is_empty() {
                    String::new()
                } else {
                    format!(",\"generics\":{}", json_strings(&v.generics))
                };
                format!(
                    "{{\"param\":\"{}\",\"capabilities\":{}{generics}}}",
                    json_escape(&v.param),
                    json_strings(&v.capabilities)
                )
            })
            .collect();
        format!(
            "{{\"name\":\"{}\",{}\"authority\":{},\"via\":[{}],\"pure\":{}}}",
            json_escape(&self.name),
            if self.public { "\"pub\":true," } else { "" },
            json_strings(&self.authority),
            via.join(","),
            self.pure
        )
    }
}

/// `"path":"...",` for an entry that names its file, and nothing for one
/// that does not.
fn path_json(location: &Option<Location>) -> String {
    location.as_ref().map_or_else(String::new, |l| {
        format!("\"path\":\"{}\",", json_escape(&l.path))
    })
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
    match oska_syntax::parse_module(src) {
        Ok(module) => check_parsed(src, &module, options, link::Modules::default()),
        Err(err) => CheckReport {
            diagnostics: vec![err],
            ..CheckReport::default()
        },
    }
}

/// Check `module`, parsed from `src`, whose spans are offsets into `src`:
/// one source file, or the module that joins a package's modules
/// (`link.rs`), whose `modules` say which module each part of `src` is.
pub(crate) fn check_parsed(
    src: &str,
    module: &Module,
    options: CheckOptions,
    modules: link::Modules,
) -> CheckReport {
    check_source(src, module, options, modules, false)
}

/// Check the toolchain's own `std.rt` source, the one text that may name
/// the intrinsics it declares. There is no way to ask for this on any
/// other source.
pub fn check_toolchain_rt() -> CheckReport {
    match oska_syntax::parse_prelude(toolchain::RT_SOURCE) {
        Ok(module) => check_source(
            toolchain::RT_SOURCE,
            &module,
            CheckOptions::default(),
            link::Modules::default(),
            true,
        ),
        Err(err) => CheckReport {
            diagnostics: vec![err],
            ..CheckReport::default()
        },
    }
}

fn check_source(
    src: &str,
    module: &Module,
    options: CheckOptions,
    modules: link::Modules,
    toolchain_rt: bool,
) -> CheckReport {
    let mut diagnostics = Vec::new();
    if !toolchain_rt {
        toolchain::module(src, &mut diagnostics);
    }
    let decls = collect::Decls::collect(module, &mut diagnostics);
    resolve::module(module, &decls, &mut diagnostics);
    decl_rules::module(src, module, &decls, &mut diagnostics);
    coherence::module(module, &decls, &mut diagnostics);
    recursion::module(module, &decls, &mut diagnostics);
    if options.release {
        release::module(module, &mut diagnostics);
    }
    let items = items::Items::collect(module, &decls);
    traits::module(src, module, &decls, &items, &mut diagnostics);
    let mut findings = body::Findings {
        diagnostics,
        modules,
        ..body::Findings::default()
    };
    body::module(src, module, &decls, &items, &mut findings);
    let body::Findings {
        mut diagnostics,
        mut audit,
        mut attenuations,
        resolutions,
        typing,
        assumed,
        instances,
        modules,
        ..
    } = findings;
    instantiation::module(module, &decls, &items, assumed, instances, &mut diagnostics);
    let program = diagnostics
        .is_empty()
        .then(|| program::lower::module(src, module, &items, &resolutions, &typing));
    let functions = authority::report(module, &decls, &items, &modules);
    // An intrinsic name is also an unknown name; E0209 says it better.
    let refused: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == codes::TOOLCHAIN_ONLY)
        .map(|d| d.span)
        .collect();
    diagnostics.retain(|d| d.code == codes::TOOLCHAIN_ONLY || !refused.contains(&d.span));
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
