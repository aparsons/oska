//! `match` and patterns.
//!
//! Each pattern is checked against the scrutinee's type, which gives its
//! bindings their types, and lowered to a [`Pat`]. A bare uppercase name
//! is always a fieldless variant (ADR 0005), never a binding. The lowered
//! arms then go through the usefulness check: an arm that matches nothing
//! new is unreachable (`E0254`), and values no arm matches make the
//! `match` non-exhaustive (`E0253`). Guards never count towards coverage.

use oska_syntax::ast::{
    Expr, ExprKind, Ident, MatchArm, Pattern, PatternArg, PatternKind, StrPart,
};
use oska_syntax::{Diagnostic, Edit, Span};

use super::{Binding, Body, Callable, Labels};
use crate::codes;
use crate::resolve::closest;
use crate::ty::Ty;
use crate::usefulness::{self, CtorInfo, Pat};

/// What a constructor pattern's path names.
enum Target {
    Variant(String, String),
    Struct(String),
}

impl Body<'_, '_, '_> {
    /// `match`. Arm bodies must agree, every pattern must fit the
    /// scrutinee, and the arms must be exhaustive and all reachable.
    pub(super) fn match_expr(
        &mut self,
        span: Span,
        scrutinee: &Expr,
        arms: &[MatchArm],
        expected: Option<&Ty>,
    ) -> Ty {
        let scrutinee_ty = self.infer(scrutinee);
        let mut result: Option<Ty> = expected.cloned();
        let mut pats = Vec::new();
        for arm in arms {
            let height = self.locals.len();
            pats.push(self.pattern(&arm.pattern, &scrutinee_ty));
            if let Some(guard) = &arm.guard {
                self.check(guard, &Ty::bool());
            }
            match &result {
                Some(ty) => {
                    let ty = ty.clone();
                    self.check(&arm.body, &ty);
                }
                None => {
                    let ty = self.infer(&arm.body);
                    if self.infer.shallow(&ty) != Ty::Never {
                        result = Some(ty);
                    }
                }
            }
            self.locals.truncate(height);
        }
        // A pattern that failed to check has already been reported, and
        // coverage of it would only add noise.
        if let Some(pats) = pats.into_iter().collect::<Option<Vec<Pat>>>() {
            self.coverage(span, scrutinee, arms, &pats, &scrutinee_ty);
        }
        result.unwrap_or(Ty::Never)
    }

    /// Report unreachable arms and a non-exhaustive `match`.
    fn coverage(
        &mut self,
        span: Span,
        scrutinee: &Expr,
        arms: &[MatchArm],
        pats: &[Pat],
        scrutinee_ty: &Ty,
    ) {
        let ty = self.infer.resolve(scrutinee_ty);
        if ty.is_error() {
            return;
        }
        let space = |t: &Ty| self.space(t);
        let mut rows = Vec::new();
        let mut unreachable = Vec::new();
        for (arm, pat) in arms.iter().zip(pats) {
            if !usefulness::useful(&rows, pat, &ty, &space) {
                unreachable.push(arm.pattern.span);
            }
            if arm.guard.is_none() {
                rows.push(pat.clone());
            }
        }
        let missing = usefulness::missing(&rows, &ty, &space);
        for span in unreachable {
            self.error(
                codes::UNREACHABLE_ARM,
                "unreachable arm: earlier arms already match every value this one does",
                span,
            );
        }
        if missing.is_empty() {
            return;
        }
        let list: Vec<String> = missing.iter().map(|p| format!("`{p}`")).collect();
        let mut diagnostic = Diagnostic::new(
            codes::NOT_EXHAUSTIVE,
            format!("`match` is not exhaustive: missing {}", list.join(", ")),
            scrutinee.span,
        );
        if let Some(edit) = self.missing_arms(span, scrutinee, arms, &missing) {
            diagnostic = diagnostic.with_fix("add arms for the missing patterns", vec![edit]);
        }
        self.diagnostics.push(diagnostic);
    }

    /// An edit adding an arm for each missing pattern, after the last arm
    /// and at its indentation. Each new arm's body is `todo()`.
    fn missing_arms(
        &self,
        span: Span,
        scrutinee: &Expr,
        arms: &[MatchArm],
        missing: &[String],
    ) -> Option<Edit> {
        let new_arms = |indent: &str| -> String {
            missing
                .iter()
                .map(|p| format!("\n{indent}{p} => todo()"))
                .collect()
        };
        if let Some(last) = arms.last() {
            let indent = line_indent(self.src, last.span.start);
            return Some(Edit {
                span: Span {
                    start: last.span.end,
                    end: last.span.end,
                },
                replacement: new_arms(indent),
            });
        }
        let open = scrutinee.span.end + self.src[scrutinee.span.end..span.end].find('{')?;
        let base = line_indent(self.src, span.start);
        Some(Edit {
            span: Span {
                start: open + 1,
                end: span.end - 1,
            },
            replacement: format!("{}\n{base}", new_arms(&format!("{base}    "))),
        })
    }

    /// The constructors of `ty`, or `None` when it has infinitely many
    /// or is opaque.
    fn space(&self, ty: &Ty) -> Option<Vec<CtorInfo>> {
        let ctor = |key: &str, fields: Vec<(Option<String>, Ty)>| CtorInfo {
            key: key.to_string(),
            labels: fields.iter().map(|(l, _)| l.clone()).collect(),
            fields: fields.into_iter().map(|(_, t)| t).collect(),
        };
        let (name, args) = match self.infer.resolve(ty) {
            Ty::Unit => return Some(vec![ctor("()", Vec::new())]),
            Ty::Named(name, args) => (name, args),
            _ => return None,
        };
        if name == "Bool" {
            return Some(vec![ctor("true", Vec::new()), ctor("false", Vec::new())]);
        }
        if let Some(info) = self.items.enums.get(&name) {
            let subst: Vec<(String, Ty)> = info.generics.iter().cloned().zip(args).collect();
            return Some(
                info.variants
                    .iter()
                    .map(|(variant, fields)| {
                        let fields = fields
                            .fields
                            .iter()
                            .map(|(l, t)| (l.clone(), t.substitute(&subst)))
                            .collect();
                        ctor(&self.variant_key(&name, variant), fields)
                    })
                    .collect(),
            );
        }
        let user_struct = self.decls().get(&name).is_some_and(|d| !d.in_prelude);
        match self.items.structs.get(&name) {
            Some(info) if user_struct => {
                let subst: Vec<(String, Ty)> = info.generics.iter().cloned().zip(args).collect();
                let fields = info
                    .fields
                    .fields
                    .iter()
                    .map(|(l, t)| (l.clone(), t.substitute(&subst)))
                    .collect();
                Some(vec![ctor(&name, fields)])
            }
            _ => None,
        }
    }

    /// How a variant is written in a pattern: bare for the prelude's
    /// variants, such as `None`, and `Enum.Variant` otherwise.
    fn variant_key(&self, enum_name: &str, variant: &str) -> String {
        if self.items.bare_variants.get(variant).map(String::as_str) == Some(enum_name) {
            variant.to_string()
        } else {
            format!("{enum_name}.{variant}")
        }
    }

    /// Check `pattern` against `expected` and bind its names. Returns the
    /// lowered pattern, or `None` when it failed and was reported.
    fn pattern(&mut self, pattern: &Pattern, expected: &Ty) -> Option<Pat> {
        match &pattern.kind {
            PatternKind::Wildcard => Some(Pat::Wild),
            PatternKind::Binding(ident) if starts_uppercase(&ident.name) => {
                self.constructor_pattern(pattern.span, std::slice::from_ref(ident), &[], expected)
            }
            PatternKind::Binding(ident) => {
                self.bind(&ident.name, expected.clone(), Binding::Pattern);
                Some(Pat::Wild)
            }
            PatternKind::Literal(expr) => self.literal_pattern(expr, expected),
            PatternKind::Constructor { path, args } => {
                self.constructor_pattern(pattern.span, path, args, expected)
            }
        }
    }

    fn literal_pattern(&mut self, expr: &Expr, expected: &Ty) -> Option<Pat> {
        if self.check(expr, expected).is_error() {
            return None;
        }
        let key = match &expr.kind {
            ExprKind::Int(text) => int_key(text, false),
            ExprKind::Unary { operand, .. } => match &operand.kind {
                ExprKind::Int(text) => int_key(text, true),
                _ => return None,
            },
            ExprKind::Str(parts) => {
                let text: String = parts
                    .iter()
                    .map(|part| match part {
                        StrPart::Text { value, .. } => value.as_str(),
                        StrPart::Hole { .. } => "",
                    })
                    .collect();
                format!("{text:?}")
            }
            ExprKind::Bool(b) => b.to_string(),
            ExprKind::Unit => "()".to_string(),
            _ => return None,
        };
        Some(Pat::Ctor(key, Vec::new()))
    }

    /// `Some(x)`, `Shape.Circle(radius: r)`, `Point(x: a, y: b)`, or a
    /// fieldless variant such as `None` or `Shape.Empty`.
    fn constructor_pattern(
        &mut self,
        span: Span,
        path: &[Ident],
        args: &[PatternArg],
        expected: &Ty,
    ) -> Option<Pat> {
        let checked = self.constructor_target(path, expected).and_then(|target| {
            let (callable, key) = match &target {
                Target::Variant(enum_name, variant) => (
                    self.variant(enum_name, variant)?,
                    self.variant_key(enum_name, variant),
                ),
                Target::Struct(name) => {
                    let info = self.items.structs.get(name)?.clone();
                    let callable =
                        self.constructor(name, &info.generics, &info.fields, name.clone());
                    (callable, name.clone())
                }
            };
            if self.expect(&callable.ret, expected, span).is_error() {
                return None;
            }
            if callable.params.len() != args.len() {
                let fields = match callable.params.len() {
                    1 => "1 field".to_string(),
                    n => format!("{n} fields"),
                };
                self.error(
                    codes::PATTERN_FIELD_COUNT,
                    format!("`{key}` has {fields}, but the pattern has {}", args.len()),
                    span,
                );
                return None;
            }
            Some((callable, key))
        });
        let Some((callable, key)) = checked else {
            for arg in args {
                self.bind_unchecked(&arg.pattern);
            }
            return None;
        };
        self.check_args(&callable, args, key)
    }

    /// Check a constructor pattern's arguments against its fields.
    fn check_args(&mut self, callable: &Callable, args: &[PatternArg], key: String) -> Option<Pat> {
        let mut fields = Vec::new();
        let mut ok = true;
        for (arg, (name, ty)) in args.iter().zip(&callable.params) {
            self.pattern_label(arg, name.as_deref(), callable.labels);
            match self.pattern(&arg.pattern, ty) {
                Some(pat) => fields.push(pat),
                None => ok = false,
            }
        }
        ok.then_some(Pat::Ctor(key, fields))
    }

    /// Named fields are matched with their labels, in order; positional
    /// fields without (ADR 0004).
    fn pattern_label(&mut self, arg: &PatternArg, field: Option<&str>, labels: Labels) {
        let (message, edit) = match (&arg.label, field) {
            (Some(label), Some(field)) if label.name != field => (
                format!("expected the field `{field}`, found `{}`", label.name),
                Edit {
                    span: label.span,
                    replacement: field.to_string(),
                },
            ),
            (None, Some(field)) if labels == Labels::Required => (
                format!("the field `{field}` must be labelled in the pattern"),
                Edit {
                    span: Span {
                        start: arg.span.start,
                        end: arg.span.start,
                    },
                    replacement: format!("{field}: "),
                },
            ),
            (Some(label), None) => (
                format!("`{}` labels a positional field", label.name),
                Edit {
                    span: Span {
                        start: label.span.start,
                        end: arg.pattern.span.start,
                    },
                    replacement: String::new(),
                },
            ),
            _ => return,
        };
        let span = arg.label.as_ref().map_or(arg.span, |l| l.span);
        self.diagnostics.push(
            Diagnostic::new(codes::PATTERN_LABEL, message, span)
                .with_fix("use the field's label", vec![edit]),
        );
    }

    /// What a constructor pattern's path names, reporting it when it names
    /// nothing a pattern can match.
    fn constructor_target(&mut self, path: &[Ident], expected: &Ty) -> Option<Target> {
        match path {
            [name] => {
                if let Some(enum_name) = self.items.bare_variants.get(&name.name) {
                    return Some(Target::Variant(enum_name.clone(), name.name.clone()));
                }
                let user_struct = self.decls().get(&name.name).is_some_and(|d| !d.in_prelude);
                if user_struct && self.items.structs.contains_key(&name.name) {
                    return Some(Target::Struct(name.name.clone()));
                }
                self.unknown_variant(name, expected);
                None
            }
            [enum_name, variant] => {
                let Some(info) = self.items.enums.get(&enum_name.name) else {
                    self.error(
                        codes::UNKNOWN_VARIANT,
                        format!("`{}` is not an enum", enum_name.name),
                        enum_name.span,
                    );
                    return None;
                };
                if info.variants.iter().any(|(v, _)| *v == variant.name) {
                    return Some(Target::Variant(
                        enum_name.name.clone(),
                        variant.name.clone(),
                    ));
                }
                let candidates: Vec<&str> = info.variants.iter().map(|(v, _)| v.as_str()).collect();
                let message = format!("`{}` has no variant `{}`", enum_name.name, variant.name);
                self.report_with_suggestion(codes::UNKNOWN_VARIANT, message, variant, &candidates);
                None
            }
            _ => {
                let span = Span {
                    start: path.first().map_or(0, |i| i.span.start),
                    end: path.last().map_or(0, |i| i.span.end),
                };
                self.error(
                    codes::UNKNOWN_VARIANT,
                    "a pattern path names an enum variant: `Enum.Variant`",
                    span,
                );
                None
            }
        }
    }

    /// A bare uppercase name that is not a variant: misspelled, or a
    /// variant of the scrutinee's enum written without its enum's name.
    /// The fix names the closest variant the pattern could mean.
    fn unknown_variant(&mut self, name: &Ident, expected: &Ty) {
        let mut candidates: Vec<(String, String)> = self
            .items
            .bare_variants
            .keys()
            .map(|v| (v.clone(), v.clone()))
            .collect();
        if let Some(enum_name) = self.infer.resolve(expected).head() {
            if let Some(info) = self.items.enums.get(enum_name) {
                for (variant, _) in &info.variants {
                    candidates.push((variant.clone(), self.variant_key(enum_name, variant)));
                }
            }
        }
        let names: Vec<&str> = candidates.iter().map(|(n, _)| n.as_str()).collect();
        let suggestion = closest(&name.name, &names)
            .and_then(|n| candidates.iter().find(|(c, _)| c == n))
            .map(|(_, written)| written.clone());
        let message = format!(
            "unknown variant `{}`; an uppercase name in a pattern must be a variant",
            name.name
        );
        let diagnostic = match suggestion {
            Some(suggestion) => Diagnostic::new(
                codes::UNKNOWN_VARIANT,
                format!("{message}; did you mean `{suggestion}`?"),
                name.span,
            )
            .with_fix(
                format!("replace with `{suggestion}`"),
                vec![Edit {
                    span: name.span,
                    replacement: suggestion,
                }],
            ),
            None => Diagnostic::new(codes::UNKNOWN_VARIANT, message, name.span),
        };
        self.diagnostics.push(diagnostic);
    }

    /// Bind the names in a pattern that could not be checked, with the
    /// error type, so their uses cause no further errors.
    fn bind_unchecked(&mut self, pattern: &Pattern) {
        match &pattern.kind {
            PatternKind::Binding(ident) if !starts_uppercase(&ident.name) => {
                self.bind(&ident.name, Ty::Error, Binding::Pattern);
            }
            PatternKind::Constructor { args, .. } => {
                for arg in args {
                    self.bind_unchecked(&arg.pattern);
                }
            }
            _ => {}
        }
    }
}

fn starts_uppercase(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_uppercase())
}

/// An integer literal's value as a key, so `1_000` and `1000` agree.
fn int_key(text: &str, negative: bool) -> String {
    let digits: String = text.chars().filter(|c| *c != '_').collect();
    match digits.parse::<i128>() {
        Ok(value) if negative => (-value).to_string(),
        Ok(value) => value.to_string(),
        Err(_) => digits,
    }
}

/// The whitespace that indents the line containing `offset`.
fn line_indent(src: &str, offset: usize) -> &str {
    let start = src[..offset].rfind('\n').map_or(0, |i| i + 1);
    let line = &src[start..];
    let end = line
        .find(|c: char| !c.is_whitespace() || c == '\n')
        .unwrap_or(line.len());
    &line[..end]
}
