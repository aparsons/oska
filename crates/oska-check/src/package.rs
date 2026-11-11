//! The package-level entry point: [`check_package`] checks every module
//! of a [`Package`] and returns one [`CheckReport`], with every
//! diagnostic, audit site and attenuation naming the file it is in.
//! [`check_file`], a single file as `oskac check` gives it, is a package
//! of one module with no manifest.
//!
//! The modules of a package are checked as one program: `link.rs` joins
//! them into one module, resolving `use pkg.…` imports, and the checker
//! checks that module, so every rule holds across the whole package.

use oska_syntax::{Location, Span};

use crate::{check_module_with, check_parsed, link, CheckOptions, CheckReport};

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
    match package.modules.as_slice() {
        [] => CheckReport::default(),
        [module] if package.manifest.is_none() => {
            let report = check_module_with(&module.source, options);
            locate(report, &Joined::new(std::slice::from_ref(module)))
        }
        modules => check_joined(modules, options),
    }
}

/// The modules' texts end to end, each after a line break, and where
/// each starts. The line break makes the end of one module's text an
/// offset no other module has.
struct Joined<'p> {
    text: String,
    modules: Vec<(&'p ModuleSource, usize)>,
}

impl<'p> Joined<'p> {
    fn new(modules: &'p [ModuleSource]) -> Joined<'p> {
        let mut text = String::new();
        let mut starts = Vec::new();
        for module in modules {
            if !starts.is_empty() {
                text.push('\n');
            }
            starts.push((module, text.len()));
            text.push_str(&module.source);
        }
        Joined {
            text,
            modules: starts,
        }
    }

    /// `span` within the file of its module, and that module.
    fn local(&self, span: Span) -> (&'p ModuleSource, Span) {
        let index = self
            .modules
            .partition_point(|(_, start)| *start <= span.start)
            .saturating_sub(1);
        let (module, start) = self.modules[index];
        let local = Span {
            start: span.start - start,
            end: (span.end - start).min(module.source.len()),
        };
        (module, local)
    }
}

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
    report.diagnostics.sort_by_key(|d| d.span.start);
    if !report.diagnostics.is_empty() {
        report.program = None;
    }
    locate(report, &joined)
}

/// Name the file of every diagnostic, audit site and attenuation of
/// `report`, whose spans are offsets into `joined`'s text, and make each
/// span an offset into its file. A fix whose edits are in another file
/// than its diagnostic names that file.
fn locate(mut report: CheckReport, joined: &Joined) -> CheckReport {
    report.diagnostics = report
        .diagnostics
        .into_iter()
        .map(|mut d| {
            let (module, span) = joined.local(d.span);
            d.span = span;
            if let Some(fix) = &mut d.fix {
                for edit in &mut fix.edits {
                    let (edited, span) = joined.local(edit.span);
                    edit.span = span;
                    if edited.path != module.path {
                        fix.path = Some(edited.path.clone());
                    }
                }
            }
            d.locate(&module.path, &module.source)
        })
        .collect();
    for site in &mut report.audit {
        let (module, span) = joined.local(site.span);
        site.span = span;
        site.location = Some(Location::new(&module.path, &module.source, span.start));
    }
    for attenuation in &mut report.attenuations {
        let (module, span) = joined.local(attenuation.span);
        attenuation.span = span;
        attenuation.location = Some(Location::new(&module.path, &module.source, span.start));
    }
    report
}
