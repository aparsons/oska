//! The package-level entry point: [`check_package`] checks every module
//! of a [`Package`] and returns one [`CheckReport`], with every
//! diagnostic, audit site and attenuation naming the file it is in.
//! [`check_file`], a single file as `oskac check` gives it, is a package
//! of one module with no manifest.
//!
//! Until cross-module `use` exists (#118), the modules of a package are
//! independent: each is checked by [`check_module_with`] as it would be
//! alone, and the reports are merged in the order the modules are given.

use oska_syntax::Location;

use crate::{check_module_with, CheckOptions, CheckReport};

/// The module path of a single file, and of a package's entry module,
/// `src/main.osk`.
pub const ENTRY_MODULE: &str = "main";

/// A package to check: its manifest's text, if it has one, and every
/// module.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Package {
    /// The text of `oska.toml`. Not read yet: the manifest arrives in #117.
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
    let mut programs = Vec::new();
    for module in &package.modules {
        let report = check_module_with(&module.source, options);
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
