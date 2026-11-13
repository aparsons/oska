//! Every program in the repository's corpus lowers to a fully typed
//! checked program (#126): the examples, the code samples of the docs,
//! the CLI fixtures and the site generator. No expression or slot is left
//! without a solved type, and no type names a generic parameter the
//! function it is in does not have.

use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

use oska_check::program::{
    walk, Expr, ExprKind, Part, Pattern, Program, Stmt, Type, TypeDeclKind, Visit,
};
use oska_check::{CheckOptions, ModuleSource, Package};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// The entries of `dir`, sorted.
fn entries(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .map(|entry| entry.expect("should read a directory entry").path())
        .collect();
    found.sort();
    found
}

/// Every `.osk` file under `dir`, as (module path, source).
fn modules(src: &Path, dir: &Path, out: &mut Vec<ModuleSource>) {
    for path in entries(dir) {
        if path.is_symlink() {
            continue;
        }
        if path.is_dir() {
            modules(src, &path, out);
        } else if path.extension().is_some_and(|e| e == "osk") {
            let relative = path.strip_prefix(src).expect("under src");
            let module = relative
                .with_extension("")
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(".");
            out.push(ModuleSource::in_package(&module, &read(&path)));
        }
    }
}

/// The package in the directory `dir`.
fn package(dir: &Path) -> Package {
    let manifest = dir.join("oska.toml");
    let mut found = Vec::new();
    if dir.join("src").is_dir() {
        modules(&dir.join("src"), &dir.join("src"), &mut found);
    }
    Package {
        // A fixture's manifest may not be UTF-8: it is refused anyway.
        manifest: std::fs::read(&manifest)
            .ok()
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned()),
        modules: found,
    }
}

/// The ```` ```oska ```` blocks of a Markdown document that stand alone
/// as a program: not fragments, and not multi-file packages.
fn samples(doc: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current: Option<String> = None;
    for line in doc.lines() {
        match &mut current {
            Some(block) if line.trim_start() == "```" => {
                out.push(std::mem::take(block));
                current = None;
            }
            Some(block) => {
                block.push_str(line);
                block.push('\n');
            }
            None => {
                let Some(info) = line.trim_start().strip_prefix("```oska") else {
                    continue;
                };
                let words: Vec<&str> = info.split_whitespace().collect();
                if !words.iter().any(|w| *w == "fragment" || *w == "package") {
                    current = Some(String::new());
                }
            }
        }
    }
    out
}

/// Every `.md` file under `dir`.
fn docs(dir: &Path, out: &mut Vec<PathBuf>) {
    for path in entries(dir) {
        if path.is_dir() {
            docs(&path, out);
        } else if path.extension().is_some_and(|e| e == "md") {
            out.push(path);
        }
    }
}

/// The corpus, as (name, checked program) for every program that checks.
fn corpus() -> Vec<(String, Program)> {
    let root = repo();
    let mut programs = Vec::new();
    let mut single = |name: String, src: &str| {
        if let Some(program) = oska_check::check_module(src).program {
            programs.push((name, program));
        }
    };
    let mut files: Vec<PathBuf> = entries(&root.join("examples"));
    files.extend(entries(&root.join("crates/oskac/tests/fixtures")));
    files.push(root.join("site/build.osk"));
    for path in files
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "osk"))
    {
        single(path.display().to_string(), &read(path));
    }
    let mut markdown = Vec::new();
    docs(&root.join("docs"), &mut markdown);
    for path in markdown {
        for (i, sample) in samples(&read(&path)).iter().enumerate() {
            single(format!("{} sample {i}", path.display()), sample);
        }
    }
    let mut dirs: Vec<PathBuf> = entries(&root.join("examples"));
    dirs.extend(entries(&root.join("crates/oskac/tests/fixtures/packages")));
    for dir in dirs.iter().filter(|d| d.is_dir() && !d.is_symlink()) {
        let report = oska_check::check_package(&package(dir), CheckOptions::default());
        if let Some(program) = report.program {
            programs.push((dir.display().to_string(), program));
        }
    }
    programs
}

/// The problems with `ty`, met at `what`, in code whose generic
/// parameters are `generics`.
fn check_type(ty: &Type, generics: &[String], what: &str, problems: &mut Vec<String>) {
    match ty {
        Type::Param(name) if !generics.contains(name) => {
            problems.push(format!(
                "{what}: `{ty}` names `{name}`, not a generic in scope"
            ));
        }
        Type::Named { args, .. } => {
            for arg in args {
                check_type(arg, generics, what, problems);
            }
        }
        Type::Fn { params, ret } => {
            for param in params {
                check_type(param, generics, what, problems);
            }
            check_type(ret, generics, what, problems);
        }
        _ => {}
    }
}

fn check_pattern(pattern: &Pattern, generics: &[String], problems: &mut Vec<String>) {
    match pattern {
        Pattern::Bind(ty) => check_type(ty, generics, "a pattern's binding", problems),
        Pattern::Construct { fields, .. } => {
            for field in fields {
                check_pattern(field, generics, problems);
            }
        }
        Pattern::Wildcard | Pattern::Literal(_) => {}
    }
}

/// The problems with the types in `code`.
fn check_code(code: &Expr, generics: &[String], problems: &mut Vec<String>) {
    let _ = walk(code, &mut |visit| {
        let Visit::Expr(expr) = visit else {
            return ControlFlow::<()>::Continue(());
        };
        let what = format!("the expression at {:?}", expr.span);
        check_type(&expr.ty, generics, &what, problems);
        let mut types: Vec<&Type> = Vec::new();
        match &expr.kind {
            ExprKind::Call { type_args, .. }
            | ExprKind::MutatingCall { type_args, .. }
            | ExprKind::FnValue { type_args, .. }
            | ExprKind::Try { type_args, .. } => types.extend(type_args),
            ExprKind::Interpolated(parts) => {
                for part in parts {
                    if let Part::Hole { type_args, .. } = part {
                        types.extend(type_args);
                    }
                }
            }
            ExprKind::Closure(closure) => types.extend(closure.params.iter().map(|p| &p.ty)),
            ExprKind::Block { stmts, .. } => {
                for stmt in stmts {
                    if let Stmt::Let { ty, .. } | Stmt::For { ty, .. } = stmt {
                        types.push(ty);
                    }
                }
            }
            ExprKind::Match { arms, .. } => {
                for arm in arms {
                    check_pattern(&arm.pattern, generics, problems);
                }
            }
            _ => {}
        }
        for ty in types {
            check_type(ty, generics, &what, problems);
        }
        ControlFlow::Continue(())
    });
}

/// The problems with the types in `program`.
fn problems(program: &Program) -> Vec<String> {
    let mut problems: Vec<String> = program
        .unsolved
        .iter()
        .map(|span| format!("the type at {span:?} is not solved"))
        .collect();
    for function in program.functions.values() {
        let generics = &function.generics;
        let mut types: Vec<&Type> = function.params.iter().map(|p| &p.ty).collect();
        types.push(&function.ret);
        types.extend(&function.self_ty);
        for ty in types {
            check_type(ty, generics, &function.name, &mut problems);
        }
        for code in function.code() {
            check_code(code, generics, &mut problems);
        }
    }
    for test in &program.tests {
        check_code(&test.body, &[], &mut problems);
    }
    for (name, decl) in &program.types {
        let fields: Vec<&Type> = match &decl.kind {
            TypeDeclKind::Struct(fields) => fields.iter().map(|f| &f.ty).collect(),
            TypeDeclKind::Enum(variants) => variants
                .iter()
                .flat_map(|v| v.fields.iter().map(|f| &f.ty))
                .collect(),
        };
        for ty in fields {
            check_type(ty, &decl.generics, name, &mut problems);
        }
    }
    problems
}

#[test]
fn every_program_in_the_corpus_is_fully_typed() {
    let corpus = corpus();
    assert!(
        corpus.len() >= 100,
        "the corpus should hold the examples, the docs' samples and the fixtures, \
         found {} programs",
        corpus.len()
    );
    let found: Vec<String> = corpus
        .iter()
        .flat_map(|(name, program)| {
            problems(program)
                .into_iter()
                .map(move |p| format!("{name}: {p}"))
        })
        .collect();
    assert!(found.is_empty(), "{}", found.join("\n"));
}

#[test]
fn the_corpus_includes_packages_and_generic_calls() {
    let corpus = corpus();
    let multi = corpus
        .iter()
        .find(|(name, _)| name.ends_with("packages/multi"))
        .map(|(_, program)| program);
    assert!(
        multi.is_some(),
        "the `multi` fixture package is in the corpus"
    );
    let generic_calls = corpus
        .iter()
        .flat_map(|(_, program)| program.functions.values())
        .flat_map(|function| function.code().cloned().collect::<Vec<_>>())
        .filter(|code| {
            walk(code, &mut |visit| match visit {
                Visit::Expr(Expr {
                    kind: ExprKind::Call { type_args, .. },
                    ..
                }) if !type_args.is_empty() => ControlFlow::Break(()),
                _ => ControlFlow::Continue(()),
            })
            .is_break()
        })
        .count();
    assert!(
        generic_calls > 10,
        "found {generic_calls} with a generic call"
    );
}
