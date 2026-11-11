//! The package-level entry point: [`check_package`] checks every module
//! of a [`Package`] and returns one [`CheckReport`], with every
//! diagnostic, audit site and attenuation naming the file it is in.
//! [`check_file`], a single file as `oskac check` gives it, is a package
//! of one module with no manifest.
//!
//! Until cross-module `use` exists (#118), the modules of a package are
//! independent: each is checked by [`check_module_with`] as it would be
//! alone, and the reports are merged in the order the modules are given.

use oska_syntax::ast::ItemKind;
use oska_syntax::{Diagnostic, Location, Span};

use crate::manifest::is_snake_case;
use crate::{check_module_with, codes, parse_manifest, CheckOptions, CheckReport};

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

/// Check every module of `package` and return one report. Diagnostics,
/// audit sites and attenuations are listed module by module, in the order
/// of `package.modules`, and in source order within a module; each names
/// its module's file. The checked program is present only when no module
/// has a diagnostic: the program of a one-module package, or of a
/// package's [`ENTRY_MODULE`]. A package of several modules without one,
/// a library, has none until modules join into one program (#118).
pub fn check_package(package: &Package, options: CheckOptions) -> CheckReport {
    let mut merged = CheckReport::default();
    if let Some(text) = &package.manifest {
        if let Err(diagnostics) = parse_manifest(MANIFEST_PATH, text) {
            merged.diagnostics.extend(diagnostics);
        }
    }
    let mut programs = Vec::new();
    for module in &package.modules {
        let mut report = check_module_with(&module.source, options);
        report.diagnostics.extend(layout_diagnostics(module));
        report.diagnostics.sort_by_key(|d| d.span.start);
        let locate =
            |span: oska_syntax::Span| Some(Location::new(&module.path, &module.source, span.start));
        merged.diagnostics.extend(
            report
                .diagnostics
                .into_iter()
                .map(|d| d.locate(&module.path, &module.source)),
        );
        merged
            .audit
            .extend(report.audit.into_iter().map(|mut site| {
                site.location = locate(site.span);
                site
            }));
        merged
            .attenuations
            .extend(report.attenuations.into_iter().map(|mut attenuation| {
                attenuation.location = locate(attenuation.span);
                attenuation
            }));
        merged.functions.extend(report.functions);
        programs.push((module.module.as_str(), report.program));
    }
    if merged.diagnostics.is_empty() {
        merged.program = if programs.len() == 1 {
            programs.pop().and_then(|(_, program)| program)
        } else {
            programs
                .into_iter()
                .find(|(module, _)| *module == ENTRY_MODULE)
                .and_then(|(_, program)| program)
        };
    }
    merged
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
