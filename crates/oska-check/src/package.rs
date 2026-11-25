// SPDX-License-Identifier: Apache-2.0
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
use oska_syntax::{Diagnostic, Frame, LineIndex, Location, Span};

use crate::manifest::is_snake_case;
use crate::{
    check_module_with, check_parsed, codes, hoist, link, parse_manifest, Attenuation, AuditSite,
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
    let mut report = check_package_unfixed(package, options);
    hoist::add_fixes(package, &mut report.diagnostics);
    report
}

/// [`check_package`], without the fixes that need the program checked
/// again (`hoist.rs`), which is how those fixes check it.
pub(crate) fn check_package_unfixed(package: &Package, options: CheckOptions) -> CheckReport {
    let manifest_errors = package
        .manifest
        .as_deref()
        .and_then(|text| parse_manifest(MANIFEST_PATH, text).err());
    // A package that needs a newer `oskac` is refused before its modules
    // are read: they may use what this `oskac` predates (`E0407`).
    if let Some(errors) = &manifest_errors {
        if errors.iter().any(|d| d.code == codes::MANIFEST_TOOLCHAIN) {
            return CheckReport {
                diagnostics: errors.clone(),
                ..CheckReport::default()
            };
        }
    }
    let mut report = match package.modules.as_slice() {
        [] => CheckReport::default(),
        [module] if package.manifest.is_none() => {
            let mut report = check_module_with(&module.source, options);
            report.diagnostics.extend(layout_diagnostics(module));
            report.diagnostics.sort_by_key(|d| d.span.start);
            if !report.diagnostics.is_empty() {
                report.program = None;
            }
            locate(report, &Joined::new(vec![module]))
        }
        modules => check_joined(modules.iter().collect(), options),
    };
    if let Some(diagnostics) = manifest_errors {
        report.diagnostics.splice(0..0, diagnostics);
        report.program = None;
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
    /// Each module's lines, in package order, so that locating every
    /// entry of a report is linear in the text.
    lines: Vec<LineIndex<'p>>,
}

impl<'p> Joined<'p> {
    /// The join of `modules`, which is not empty, and of the `std`
    /// modules written in Oska that they import, which come last.
    fn new(modules: Vec<&'p ModuleSource>) -> Joined<'p> {
        Joined::joining(modules, false)
    }

    /// [`Joined::new`], where `toolchain` says the entry is the
    /// toolchain's `std.rt`, which has functions without bodies.
    fn joining(modules: Vec<&'p ModuleSource>, toolchain: bool) -> Joined<'p> {
        let mut modules = modules;
        modules.extend(std_sources(&modules, toolchain));
        let entry_first = modules
            .iter()
            .copied()
            .filter(|m| m.module == ENTRY_MODULE)
            .chain(modules.iter().copied().filter(|m| m.module != ENTRY_MODULE));
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
            .filter_map(|module| starts.iter().find(|(m, _)| std::ptr::eq(*m, *module)))
            .copied()
            .collect();
        let in_package_order: Vec<(&ModuleSource, usize)> = in_package_order;
        Joined {
            text,
            lines: in_package_order
                .iter()
                .map(|(module, _)| LineIndex::new(&module.source))
                .collect(),
            modules: in_package_order,
        }
    }

    /// The location of `local`, an offset into the file of the module at
    /// `index` in package order.
    fn location(&self, index: usize, local: usize) -> Location {
        self.spanning(index, Span::at(local))
    }

    /// The location of `local`, a span in the file of the module at
    /// `index` in package order.
    fn spanning(&self, index: usize, local: Span) -> Location {
        let path = self.modules.get(index).map_or("", |(m, _)| m.path.as_str());
        match self.lines.get(index) {
            Some(lines) => lines.spanning(path, local),
            None => Location::spanning(path, "", local),
        }
    }

    /// `diagnostic`, whose span is an offset into the joined text, in the
    /// file of its module: located, its span an offset into that file,
    /// a fix whose edits are in another file naming that file, and each
    /// related place located in the file it is in.
    fn locate(&self, mut d: Diagnostic) -> (usize, Diagnostic) {
        let (module, span, index) = self.local(d.span);
        d.span = span;
        if let Some(fix) = &mut d.fix {
            for edit in &mut fix.edits {
                let (edited, span, _) = self.local(edit.span);
                edit.span = span;
                if edited.path != module.path {
                    fix.path = Some(edited.path.clone());
                }
            }
        }
        d.location = Some(Box::new(self.spanning(index, span)));
        for related in &mut d.related {
            let (_, span, index) = self.local(related.span);
            related.span = span;
            related.location = Some(self.spanning(index, span));
        }
        if let Some(frames) = &mut d.frames {
            for frame in &mut frames.frames {
                self.locate_frame(frame);
            }
        }
        (index, d)
    }

    /// `frame`, whose span is an offset into the joined text, located in
    /// the file of its module, its span an offset into that file.
    fn locate_frame(&self, frame: &mut Frame) {
        let (_, span, index) = self.local(frame.span);
        frame.span = span;
        frame.location = Some(self.spanning(index, span));
    }

    /// `frame`, not yet located, with its function named as code in its
    /// own module names it.
    fn display_frame_keys(&self, keys: &link::Keys, frame: &mut Frame) {
        let (module, _, _) = self.local(frame.span);
        frame.function = keys.display(&frame.function, &module.module);
    }

    /// `diagnostic`, a message about the joined module, with every key in
    /// its message and its fix's written as code in its module names the
    /// item (`link::Keys::display`).
    fn display_keys(&self, keys: &link::Keys, mut d: Diagnostic) -> Diagnostic {
        let (module, _, _) = self.local(d.span);
        d.message = keys.display(&d.message, &module.module);
        if let Some(frames) = &mut d.frames {
            for frame in &mut frames.frames {
                self.display_frame_keys(keys, frame);
            }
        }
        if let Some(fix) = &mut d.fix {
            fix.message = keys.display(&fix.message, &module.module);
        }
        d
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

/// The `std` modules written in Oska that `modules` import, and that
/// those import in turn, as modules of the program: each once, and none
/// that is among `modules` already. A module that does not parse imports
/// nothing here; checking it reports why.
fn std_sources(modules: &[&ModuleSource], toolchain: bool) -> Vec<&'static ModuleSource> {
    let mut found: Vec<&'static ModuleSource> = Vec::new();
    let mut pending: Vec<&str> = modules.iter().map(|m| m.source.as_str()).collect();
    while let Some(source) = pending.pop() {
        let parsed = if toolchain {
            oska_syntax::parse_prelude(source)
        } else {
            oska_syntax::parse_module(source)
        };
        let Ok(ast) = parsed else {
            continue;
        };
        for std in crate::prelude::oska_std_imports(&ast) {
            let present = modules.iter().any(|m| m.module == std.path)
                || found.iter().any(|m| m.module == std.path);
            if !present {
                let module = std_source(std);
                found.push(module);
                pending.push(&module.source);
            }
        }
    }
    found
}

/// `std`, a `std` module written in Oska, as a module of a program: its
/// module path is its `std` path, which no package module can have, and
/// its file is named `std/<name>.osk`.
fn std_source(std: &'static crate::prelude::StdModule) -> &'static ModuleSource {
    static SOURCES: std::sync::OnceLock<Vec<ModuleSource>> = std::sync::OnceLock::new();
    let sources = SOURCES.get_or_init(|| {
        crate::prelude::STD_MODULES
            .iter()
            .filter(|m| m.written_in_oska)
            .map(|m| ModuleSource {
                module: m.path.to_string(),
                path: format!("std/{}.osk", m.name()),
                source: m.source.to_string(),
            })
            .collect()
    });
    sources
        .iter()
        .find(|m| m.module == std.path)
        .expect("every std module written in Oska has a source")
}

/// Check `source`, a single file that imports a `std` module written in
/// Oska, joined with the modules it imports, as `check_module_with`
/// checks a single file: its own items keep their names, and the report's
/// spans are offsets into `source`, not located in a file.
pub(crate) fn check_single_with_std(source: &str, options: CheckOptions) -> CheckReport {
    let file = ModuleSource {
        module: ENTRY_MODULE.to_string(),
        path: String::new(),
        source: source.to_string(),
    };
    let (report, _) = check_linked(vec![&file], options, true, false);
    report
}

/// [`check_single_with_std`] for the toolchain's own `std.rt`, which may
/// name the intrinsics it declares, has functions without bodies, and
/// imports the `std` modules the runtime calls (`std.dns`).
pub(crate) fn check_toolchain_with_std(source: &str) -> CheckReport {
    let file = ModuleSource {
        module: ENTRY_MODULE.to_string(),
        path: String::new(),
        source: source.to_string(),
    };
    let (report, _) = check_linked(vec![&file], CheckOptions::default(), true, true);
    report
}

/// Parse every module at its offset in the joined text, link them, and
/// check the joined module.
fn check_joined(modules: Vec<&ModuleSource>, options: CheckOptions) -> CheckReport {
    let (report, joined) = check_linked(modules, options, false, false);
    locate(report, &joined)
}

/// [`check_joined`], with the report's spans still offsets into the
/// joined text. With `plain_entry`, `modules` is a single file
/// ([`link::link`]).
fn check_linked(
    modules: Vec<&ModuleSource>,
    options: CheckOptions,
    plain_entry: bool,
    toolchain: bool,
) -> (CheckReport, Joined<'_>) {
    let joined = Joined::joining(modules, toolchain);
    let mut parsed = Vec::new();
    let mut parse_errors = Vec::new();
    for (module, start) in &joined.modules {
        let end = start + module.source.len();
        let attempt = if toolchain {
            oska_syntax::parse_prelude_at(&joined.text[..end], *start)
        } else {
            oska_syntax::parse_module_at(&joined.text[..end], *start)
        };
        match attempt {
            Ok(ast) => parsed.push(link::Parsed {
                module: module.module.clone(),
                ast,
            }),
            Err(err) => parse_errors.push(err),
        }
    }
    let mut layout = Vec::new();
    for (module, start) in joined
        .modules
        .iter()
        .filter(|(m, _)| !link::is_std_path(&m.module))
    {
        layout.extend(layout_diagnostics(module).into_iter().map(|mut d| {
            d.span = Span {
                start: d.span.start + start,
                end: d.span.end + start,
            };
            d
        }));
    }
    if !parse_errors.is_empty() {
        let mut diagnostics = parse_errors;
        diagnostics.extend(layout);
        diagnostics.sort_by_key(|d| d.span.start);
        let report = CheckReport {
            diagnostics,
            ..CheckReport::default()
        };
        return (report, joined);
    }
    let linked = link::link(&joined.text, parsed, plain_entry);
    let ranges = joined
        .modules
        .iter()
        .map(|(module, start)| {
            let span = Span {
                start: *start,
                end: start + module.source.len(),
            };
            (module.module.clone(), span)
        })
        .collect();
    let modules = if plain_entry {
        link::Modules::single_file_with_std(ranges)
    } else {
        link::Modules::new(ranges)
    };
    let mut report = check_parsed(&joined.text, &linked.module, options, modules, toolchain);
    report.diagnostics = report
        .diagnostics
        .into_iter()
        .filter(|d| {
            !linked
                .reported
                .iter()
                .any(|r| r.start <= d.span.start && d.span.end <= r.end)
        })
        .map(|d| joined.display_keys(&linked.keys, d))
        .collect();
    report.diagnostics.extend(linked.diagnostics);
    report.diagnostics.extend(layout);
    report.diagnostics.sort_by_key(|d| d.span.start);
    if !report.diagnostics.is_empty() {
        report.program = None;
    }
    (report, joined)
}

/// What the offsets of a checked package's program mean. Built once for
/// a package, it turns what the program reports, a runtime panic or a
/// failed test, into a report about a file: its module, its file and
/// offsets in that file, and item names as that module's code writes
/// them. A package that is one file (one module, no manifest) keeps its
/// offsets, and has no module.
pub struct Locator<'p> {
    joined: Option<Joined<'p>>,
    keys: link::Keys,
    single_file: bool,
}

impl<'p> Locator<'p> {
    pub fn new(package: &'p Package) -> Locator<'p> {
        let joined =
            (!package.modules.is_empty()).then(|| Joined::new(package.modules.iter().collect()));
        // The keys of every module, the `std` ones written in Oska that
        // the program imports included.
        let parsed: Vec<(&str, oska_syntax::ast::Module)> = joined
            .iter()
            .flat_map(|joined| &joined.modules)
            .filter_map(|(m, _)| {
                Some((
                    m.module.as_str(),
                    oska_syntax::parse_module(&m.source).ok()?,
                ))
            })
            .collect();
        let keys =
            link::Keys::new(parsed.iter().flat_map(|(module, ast)| {
                link::declared_names(ast).map(move |name| (*module, name))
            }));
        Locator {
            joined,
            keys,
            single_file: package.manifest.is_none() && package.modules.len() == 1,
        }
    }

    /// `diagnostic`, whose span is an offset into the program, about the
    /// file it is in.
    pub fn locate(&self, diagnostic: Diagnostic) -> Diagnostic {
        let Some(joined) = &self.joined else {
            return diagnostic;
        };
        let diagnostic = joined.display_keys(&self.keys, diagnostic);
        joined.locate(diagnostic).1
    }

    /// `frame` of a panic's call stack, whose span is an offset into the
    /// program, about the file it is in, as [`Locator::locate`] places a
    /// diagnostic's frames.
    pub fn locate_frame(&self, mut frame: Frame) -> Frame {
        if let Some(joined) = &self.joined {
            joined.display_frame_keys(&self.keys, &mut frame);
            joined.locate_frame(&mut frame);
        }
        frame
    }

    /// The module path of the code at `span` of the program, such as
    /// `util.text`: `None` for a single file.
    pub fn module_of(&self, span: Span) -> Option<&'p str> {
        if self.single_file {
            return None;
        }
        let (module, _, _) = self.joined.as_ref()?.local(span);
        Some(module.module.as_str())
    }
}

/// Name the file of every diagnostic, audit site and attenuation of
/// `report`, whose spans are offsets into `joined`'s text, and make each
/// span an offset into its file. A fix whose edits are in another file
/// than its diagnostic names that file.
fn locate(mut report: CheckReport, joined: &Joined) -> CheckReport {
    let mut diagnostics: Vec<(usize, Diagnostic)> = report
        .diagnostics
        .into_iter()
        .map(|d| joined.locate(d))
        .collect();
    // Stable: source order stays within each module.
    diagnostics.sort_by_key(|(index, _)| *index);
    report.diagnostics = diagnostics.into_iter().map(|(_, d)| d).collect();
    let mut audit: Vec<(usize, AuditSite)> = report
        .audit
        .into_iter()
        .map(|mut site| {
            let (_, span, index) = joined.local(site.span);
            site.span = span;
            site.location = Some(joined.location(index, span.start));
            (index, site)
        })
        .collect();
    audit.sort_by_key(|(index, _)| *index);
    report.audit = audit.into_iter().map(|(_, site)| site).collect();
    let mut attenuations: Vec<(usize, Attenuation)> = report
        .attenuations
        .into_iter()
        .map(|mut attenuation| {
            let (_, span, index) = joined.local(attenuation.span);
            attenuation.span = span;
            attenuation.location = Some(joined.location(index, span.start));
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
        let problem = named
            .split('/')
            .enumerate()
            .find_map(|(at, name)| name_problem(name, at == 0).map(|problem| (name, problem)));
        if let Some((bad, problem)) = problem {
            found.push(Diagnostic::new(
                codes::BAD_MODULE_NAME,
                format!("`{bad}` in `{}` {problem}", module.path),
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

/// What is wrong with `name`, a directory or file name (without `.osk`)
/// of a module's path under `src/`, when something is; `first` when it
/// is the module path's first name. Every module must be one `use pkg.…`
/// can name, and its name must not read as anything else: so a name is
/// snake_case and not a keyword, and a module path does not start with
/// `pkg` or `std`, which begin import paths, or `main`, the entry module.
fn name_problem(name: &str, first: bool) -> Option<&'static str> {
    if name.starts_with('.') {
        Some("is hidden: every `.osk` file under `src/` is a module, so rename it or move it out of `src/`")
    } else if !is_snake_case(name) {
        Some("is not a snake_case name, so `use pkg.…` cannot name this module")
    } else if oska_syntax::is_keyword(name) {
        Some("is a keyword, so `use pkg.…` cannot name this module")
    } else if first && RESERVED_ROOTS.contains(&name) {
        Some(
            "is reserved: a module path cannot start with `pkg` or `std`, which begin \
             import paths, or `main`, the entry module",
        )
    } else {
        None
    }
}

/// The names a module path cannot start with.
const RESERVED_ROOTS: [&str; 3] = [crate::link::PKG, "std", ENTRY_MODULE];

/// The refusal of a symlink at `path`, package-relative (`E0411`): the
/// manifest, or a file or directory under `src/`. A package's source is
/// what is in its tree. Finding one needs the file system, so `oskac`
/// does it.
pub fn symlink_refused(path: &str) -> Diagnostic {
    Diagnostic::new(
        codes::SYMLINK_IN_SOURCE,
        format!(
            "`{path}` is a symlink: a package's `oska.toml` and its source must be regular \
             files and directories in its own tree"
        ),
        Span { start: 0, end: 0 },
    )
    .locate(path, "")
}

/// The refusal of the directory `dir`, given as a package, when `problem`
/// says what it lacks (`E0413`). `inside` is the command and the package
/// directory above `dir`, when `dir` is inside one, which the message
/// suggests instead.
pub fn not_a_package(dir: &str, problem: &str, inside: Option<(&str, &str)>) -> Diagnostic {
    let hint = inside.map_or_else(String::new, |(command, above)| {
        format!("; `{dir}` is inside the package in `{above}`: run `oskac {command} {above}`")
    });
    Diagnostic::new(
        codes::NOT_A_PACKAGE,
        format!(
            "`{dir}` is not a package: {problem}{hint}; a package has an `oska.toml` with a \
             `[package]` table and its modules under `src/`, and a single file is checked by \
             naming the `.osk` file"
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
