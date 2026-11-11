//! The package-level entry point: [`check_package`] checks every module
//! of a [`Package`] and returns one [`CheckReport`], with every
//! diagnostic, audit site and attenuation naming the file it is in.
//! [`check_file`], a single file as `oskac check` gives it, is a package
//! of one module with no manifest.
//!
//! The modules of a package are checked as one program: `link.rs` joins
//! them into one module, resolving `use pkg.…` imports, and the checker
//! checks that module, so every rule holds across the whole package.

use oska_syntax::ast::ItemKind;
use oska_syntax::{Diagnostic, Location, Span};

use crate::manifest::is_snake_case;
use crate::{
    check_module_with, check_parsed, codes, link, parse_manifest, Attenuation, AuditSite,
    CheckOptions, CheckReport,
};

/// The manifest's path in a package, and in the reports.
pub const MANIFEST_PATH: &str = "oska.toml";

/// The module path of a single file, and of a package's entry module,
/// `src/main.osk`.
pub const ENTRY_MODULE: &str = "main";

/// A package to check: its manifest's text, if it has one, and every
/// module.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Package {
    /// The text of `oska.toml`, read by [`parse_manifest`]: its errors
    /// are reported first, with the path `oska.toml`.
    pub manifest: Option<String>,
    /// Every module, in the order the report lists them.
    pub modules: Vec<ModuleSource>,
}

/// One module of a package: one source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleSource {
    /// The module path, as `use pkg.<module>` names it, such as
    /// `util.text`. A single file is the module [`ENTRY_MODULE`].
    pub module: String,
    /// The file's path, as report entries name it: package-relative, such
    /// as `src/util/text.osk`, or as given on the command line for a single
    /// file.
    pub path: String,
    /// The file's text.
    pub source: String,
}

impl ModuleSource {
    /// The module at `module` in a package: its file is `src/` followed by
    /// the module path with `.` written as `/`, and `.osk`, so `util.text`
    /// is `src/util/text.osk`.
    pub fn in_package(module: &str, source: &str) -> ModuleSource {
        ModuleSource {
            module: module.to_string(),
            path: format!("src/{}.osk", module.replace('.', "/")),
            source: source.to_string(),
        }
    }
}

impl Package {
    /// A library: a package with no `src/main.osk`. It can be checked and
    /// tested, not run.
    pub fn is_library(&self) -> bool {
        !self.modules.iter().any(|m| m.module == ENTRY_MODULE)
    }

    /// A single file, the program at `path` with the text `source`: a
    /// package of one module, [`ENTRY_MODULE`], with no manifest.
    pub fn single_file(path: &str, source: &str) -> Package {
        Package {
            manifest: None,
            modules: vec![ModuleSource {
                module: ENTRY_MODULE.to_string(),
                path: path.to_string(),
                source: source.to_string(),
            }],
        }
    }
}

/// Check the single file at `path`, whose text is `source`, as a package
/// of one module: what `oskac check <file.osk>` reports.
pub fn check_file(path: &str, source: &str, options: CheckOptions) -> CheckReport {
    check_package(&Package::single_file(path, source), options)
}

/// Check every module of `package` as one program and return one report.
/// Diagnostics, audit sites and attenuations are listed module by module,
/// in the order of `package.modules`, and in source order within a
/// module; each names its module's file, and its spans are offsets into
/// that file. The checked program, present only when there are no
/// diagnostics, spans every module (`link.rs`).
///
/// A package of one module with no manifest is a single file: checked
/// exactly as [`check_module_with`] checks it, where `use pkg.…` is an
/// error.
pub fn check_package(package: &Package, options: CheckOptions) -> CheckReport {
    let mut report = match package.modules.as_slice() {
        [] => CheckReport::default(),
        [module] if package.manifest.is_none() => {
            let mut report = check_module_with(&module.source, options);
            report.diagnostics.extend(layout_diagnostics(module));
            report.diagnostics.sort_by_key(|d| d.span.start);
            if !report.diagnostics.is_empty() {
                report.program = None;
            }
            locate(report, &Joined::new(std::slice::from_ref(module)))
        }
        modules => check_joined(modules, options),
    };
    if let Some(text) = &package.manifest {
        if let Err(diagnostics) = parse_manifest(MANIFEST_PATH, text) {
            report.diagnostics.splice(0..0, diagnostics);
            report.program = None;
        }
    }
    report
}

/// The modules' texts end to end, each after a line break, and where
/// each starts. The line break makes the end of one module's text an
/// offset no other module has. The entry module's text comes first, so
/// that its spans, such as a runtime panic's, are offsets into its own
/// file; `modules` stays in package order.
struct Joined<'p> {
    text: String,
    modules: Vec<(&'p ModuleSource, usize)>,
}

impl<'p> Joined<'p> {
    /// The join of `modules`, which is not empty.
    fn new(modules: &'p [ModuleSource]) -> Joined<'p> {
        let entry_first = modules
            .iter()
            .filter(|m| m.module == ENTRY_MODULE)
            .chain(modules.iter().filter(|m| m.module != ENTRY_MODULE));
        let mut text = String::new();
        let mut starts = Vec::new();
        for module in entry_first {
            if !starts.is_empty() {
                text.push('\n');
            }
            starts.push((module, text.len()));
            text.push_str(&module.source);
        }
        let in_package_order = modules
            .iter()
            .filter_map(|module| starts.iter().find(|(m, _)| std::ptr::eq(*m, module)))
            .copied()
            .collect();
        Joined {
            text,
            modules: in_package_order,
        }
    }

    /// `span` within the file of its module: that module, the span, and
    /// the module's index in package order.
    fn local(&self, span: Span) -> (&'p ModuleSource, Span, usize) {
        let index = self
            .modules
            .iter()
            .position(|(module, start)| {
                *start <= span.start && span.start <= start + module.source.len()
            })
            .unwrap_or(0);
        let Some(&(module, start)) = self.modules.get(index) else {
            return (&NO_MODULE, span, 0);
        };
        let local = Span {
            start: span.start.saturating_sub(start),
            end: span.end.saturating_sub(start).min(module.source.len()),
        };
        (module, local, index)
    }
}

/// What [`Joined::local`] names in a join of no modules, which a package
/// check never makes.
static NO_MODULE: ModuleSource = ModuleSource {
    module: String::new(),
    path: String::new(),
    source: String::new(),
};

/// Parse every module at its offset in the joined text, link them, and
/// check the joined module.
fn check_joined(modules: &[ModuleSource], options: CheckOptions) -> CheckReport {
    let joined = Joined::new(modules);
    let mut parsed = Vec::new();
    let mut parse_errors = Vec::new();
    for (module, start) in &joined.modules {
        let end = start + module.source.len();
        match oska_syntax::parse_module_at(&joined.text[..end], *start) {
            Ok(ast) => parsed.push(link::Parsed {
                module: module.module.clone(),
                ast,
            }),
            Err(err) => parse_errors.push(err),
        }
    }
    if !parse_errors.is_empty() {
        let report = CheckReport {
            diagnostics: parse_errors,
            ..CheckReport::default()
        };
        return locate(report, &joined);
    }
    let linked = link::link(&joined.text, parsed);
    let mut report = check_parsed(&joined.text, &linked.module, options);
    report.diagnostics.retain(|d| {
        !linked
            .reported
            .iter()
            .any(|r| r.start <= d.span.start && d.span.end <= r.end)
    });
    report.diagnostics.extend(linked.diagnostics);
    for (module, start) in &joined.modules {
        report
            .diagnostics
            .extend(layout_diagnostics(module).into_iter().map(|mut d| {
                d.span = Span {
                    start: d.span.start + start,
                    end: d.span.end + start,
                };
                d
            }));
    }
    report.diagnostics.sort_by_key(|d| d.span.start);
    if !report.diagnostics.is_empty() {
        report.program = None;
    }
    locate(report, &joined)
}

/// A diagnostic whose span is an offset into the program of `package`,
/// as a runtime panic's is, in the file and the offsets of its module:
/// the span is local to that file and the diagnostic names it. A
/// package that is one file (one module, no manifest) keeps its offsets.
pub fn locate_in_package(package: &Package, diagnostic: Diagnostic) -> Diagnostic {
    if package.modules.is_empty() {
        return diagnostic;
    }
    let joined = Joined::new(&package.modules);
    let (module, span, _) = joined.local(diagnostic.span);
    let mut located = diagnostic;
    located.span = span;
    located.locate(&module.path, &module.source)
}

/// Name the file of every diagnostic, audit site and attenuation of
/// `report`, whose spans are offsets into `joined`'s text, and make each
/// span an offset into its file. A fix whose edits are in another file
/// than its diagnostic names that file.
fn locate(mut report: CheckReport, joined: &Joined) -> CheckReport {
    let mut diagnostics: Vec<(usize, Diagnostic)> = report
        .diagnostics
        .into_iter()
        .map(|mut d| {
            let (module, span, index) = joined.local(d.span);
            d.span = span;
            if let Some(fix) = &mut d.fix {
                for edit in &mut fix.edits {
                    let (edited, span, _) = joined.local(edit.span);
                    edit.span = span;
                    if edited.path != module.path {
                        fix.path = Some(edited.path.clone());
                    }
                }
            }
            (index, d.locate(&module.path, &module.source))
        })
        .collect();
    // Stable: source order stays within each module.
    diagnostics.sort_by_key(|(index, _)| *index);
    report.diagnostics = diagnostics.into_iter().map(|(_, d)| d).collect();
    let mut audit: Vec<(usize, AuditSite)> = report
        .audit
        .into_iter()
        .map(|mut site| {
            let (module, span, index) = joined.local(site.span);
            site.span = span;
            site.location = Some(Location::new(&module.path, &module.source, span.start));
            (index, site)
        })
        .collect();
    audit.sort_by_key(|(index, _)| *index);
    report.audit = audit.into_iter().map(|(_, site)| site).collect();
    let mut attenuations: Vec<(usize, Attenuation)> = report
        .attenuations
        .into_iter()
        .map(|mut attenuation| {
            let (module, span, index) = joined.local(attenuation.span);
            attenuation.span = span;
            attenuation.location = Some(Location::new(&module.path, &module.source, span.start));
            (index, attenuation)
        })
        .collect();
    attenuations.sort_by_key(|(index, _)| *index);
    report.attenuations = attenuations.into_iter().map(|(_, a)| a).collect();
    report
}

/// The layout rules for one module, as diagnostics of its file (before
/// the file is located): every directory and file name under `src/` is
/// snake_case (`E0410`), and `main` is declared only by [`ENTRY_MODULE`],
/// `src/main.osk` (`E0412`). A single file, which is [`ENTRY_MODULE`], is
/// exempt from both.
fn layout_diagnostics(module: &ModuleSource) -> Vec<Diagnostic> {
    if module.module == ENTRY_MODULE {
        return Vec::new();
    }
    let mut found = Vec::new();
    let named = module
        .path
        .strip_prefix("src/")
        .map(|rest| rest.strip_suffix(".osk").unwrap_or(rest));
    if let Some(named) = named {
        if let Some(bad) = named.split('/').find(|name| !is_snake_case(name)) {
            found.push(Diagnostic::new(
                codes::BAD_MODULE_NAME,
                format!(
                    "`{bad}` in `{}` is not a snake_case name, so `use pkg.{}` cannot name this module",
                    module.path, module.module
                ),
                Span { start: 0, end: 0 },
            ));
        }
    }
    if let Ok(parsed) = oska_syntax::parse_module(&module.source) {
        for item in &parsed.items {
            if let ItemKind::Fn(function) = &item.kind {
                if function.name.name == "main" {
                    found.push(Diagnostic::new(
                        codes::MAIN_OUTSIDE_ENTRY,
                        format!(
                            "`main` is only allowed in `src/main.osk`, not in `{}`",
                            module.path
                        ),
                        function.name.span,
                    ));
                }
            }
        }
    }
    found
}

/// The refusal of a symlinked file or directory at `path`, package-relative,
/// under `src/` (`E0411`): a package's source is what is in its tree.
/// Finding one needs the file system, so `oskac` does it.
pub fn symlink_refused(path: &str) -> Diagnostic {
    Diagnostic::new(
        codes::SYMLINK_IN_SOURCE,
        format!("`{path}` is a symlink: a package's source must be regular files and directories under `src/`"),
        Span { start: 0, end: 0 },
    )
    .locate(path, "")
}

/// The refusal of the directory `dir`, given as a package, when `problem`
/// says what it lacks (`E0413`).
pub fn not_a_package(dir: &str, problem: &str) -> Diagnostic {
    Diagnostic::new(
        codes::NOT_A_PACKAGE,
        format!(
            "`{dir}` is not a package: {problem}; a package has an `oska.toml` with a `[package]` \
             table and its modules under `src/`, and a single file is checked by naming the `.osk` file"
        ),
        Span { start: 0, end: 0 },
    )
}

/// The refusal of `oskac run` on a library (`E0414`): a package without
/// `src/main.osk` has no `main` to run.
pub fn run_of_library(dir: &str) -> Diagnostic {
    Diagnostic::new(
        codes::RUN_OF_LIBRARY,
        format!(
            "`{dir}` is a library: it has no `src/main.osk`, so there is no `main` to run; \
             use `oskac check` or `oskac test`, or add `src/main.osk`"
        ),
        Span { start: 0, end: 0 },
    )
}
