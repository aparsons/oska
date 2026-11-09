//! Pattern usefulness: the standard algorithm behind exhaustiveness and
//! unreachable-arm checking.
//!
//! A `match` is lowered to a matrix of [`Pat`] rows, one per unguarded
//! arm. An arm is unreachable when its pattern is not *useful* against the
//! rows above it: it matches no value they don't already match. A `match`
//! is exhaustive when no value is left over; [`missing`] lists example
//! patterns for the values that are.
//!
//! The algorithm only needs to know, for each type, its constructors:
//! [`Space`] gives them for types with finitely many (enums, structs,
//! `Bool`, `()`), and `None` for types with infinitely many (`Int`,
//! `String`, and anything opaque), which only a wildcard can cover.

use crate::ty::Ty;

/// A pattern, reduced to what coverage depends on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pat {
    /// A wildcard or a binding: matches anything.
    Wild,
    /// A constructor with its field patterns, identified by its key: the
    /// way it is written in a pattern, such as `None`, `Shape.Circle`,
    /// `true`, `()`, or a literal such as `3` or `"a"`.
    Ctor(String, Vec<Pat>),
}

/// One constructor of a type with finitely many.
#[derive(Debug, Clone)]
pub struct CtorInfo {
    /// The constructor's key, as in [`Pat::Ctor`].
    pub key: String,
    /// The types of its fields.
    pub fields: Vec<Ty>,
    /// Each field's label, when the fields are named.
    pub labels: Vec<Option<String>>,
}

/// The constructors of a type, or `None` when there are infinitely many.
pub type Space<'s> = &'s dyn Fn(&Ty) -> Option<Vec<CtorInfo>>;

/// Whether `pat`, of type `ty`, matches a value that no row matches.
pub fn useful(rows: &[Pat], pat: &Pat, ty: &Ty, space: Space) -> bool {
    let rows: Vec<Vec<Pat>> = rows.iter().map(|r| vec![r.clone()]).collect();
    useful_vec(
        &rows,
        std::slice::from_ref(pat),
        std::slice::from_ref(ty),
        space,
    )
}

/// Example patterns for the values of type `ty` that no row matches.
/// With no rows at all, each constructor is listed rather than `_`.
pub fn missing(rows: &[Pat], ty: &Ty, space: Space) -> Vec<String> {
    if rows.is_empty() {
        if let Some(ctors) = space(ty) {
            return ctors
                .iter()
                .map(|c| display(c, &vec!["_".to_string(); c.fields.len()]))
                .collect();
        }
    }
    let rows: Vec<Vec<Pat>> = rows.iter().map(|r| vec![r.clone()]).collect();
    witnesses(&rows, std::slice::from_ref(ty), space)
        .into_iter()
        .filter_map(|mut w| w.pop())
        .collect()
}

fn useful_vec(rows: &[Vec<Pat>], q: &[Pat], tys: &[Ty], space: Space) -> bool {
    let Some((head, rest)) = q.split_first() else {
        return rows.is_empty();
    };
    let ctors = space(&tys[0]);
    match head {
        Pat::Ctor(key, args) => {
            let ctor = ctor_info(ctors.as_deref(), key, args.len());
            let rows = specialize(rows, &ctor);
            let q: Vec<Pat> = args.iter().chain(rest).cloned().collect();
            useful_vec(&rows, &q, &field_tys(&ctor, tys), space)
        }
        Pat::Wild => match complete(ctors.as_deref(), rows) {
            Some(ctors) => ctors.iter().any(|ctor| {
                let q: Vec<Pat> = wilds(ctor)
                    .into_iter()
                    .chain(rest.iter().cloned())
                    .collect();
                useful_vec(&specialize(rows, ctor), &q, &field_tys(ctor, tys), space)
            }),
            None => useful_vec(&default(rows), rest, &tys[1..], space),
        },
    }
}

/// Rows of example patterns, one column per type in `tys`, covering the
/// values that no row of `rows` matches.
fn witnesses(rows: &[Vec<Pat>], tys: &[Ty], space: Space) -> Vec<Vec<String>> {
    let Some((ty, rest)) = tys.split_first() else {
        return if rows.is_empty() {
            vec![Vec::new()]
        } else {
            Vec::new()
        };
    };
    let ctors = space(ty);
    if let Some(ctors) = complete(ctors.as_deref(), rows) {
        let mut out = Vec::new();
        for ctor in &ctors {
            for w in witnesses(&specialize(rows, ctor), &field_tys(ctor, tys), space) {
                let (fields, tail) = w.split_at(ctor.fields.len());
                let mut row = vec![display(ctor, fields)];
                row.extend_from_slice(tail);
                out.push(row);
            }
        }
        return out;
    }
    let tails = witnesses(&default(rows), rest, space);
    if tails.is_empty() {
        return Vec::new();
    }
    let used = heads(rows);
    let heads: Vec<String> = match ctors {
        Some(ctors) if !used.is_empty() => ctors
            .iter()
            .filter(|c| !used.contains(&c.key.as_str()))
            .map(|c| display(c, &vec!["_".to_string(); c.fields.len()]))
            .collect(),
        _ => vec!["_".to_string()],
    };
    let mut out = Vec::new();
    for tail in &tails {
        for head in &heads {
            let mut row = vec![head.clone()];
            row.extend_from_slice(tail);
            out.push(row);
        }
    }
    out
}

/// How a constructor with these field patterns is written.
fn display(ctor: &CtorInfo, fields: &[String]) -> String {
    if fields.is_empty() {
        return ctor.key.clone();
    }
    let args: Vec<String> = fields
        .iter()
        .zip(&ctor.labels)
        .map(|(f, label)| match label {
            Some(label) => format!("{label}: {f}"),
            None => f.clone(),
        })
        .collect();
    format!("{}({})", ctor.key, args.join(", "))
}

/// The keys of the constructors heading `rows`.
fn heads(rows: &[Vec<Pat>]) -> Vec<&str> {
    rows.iter()
        .filter_map(|r| match &r[0] {
            Pat::Ctor(key, _) => Some(key.as_str()),
            Pat::Wild => None,
        })
        .collect()
}

/// The type's constructors, when it has finitely many and every one
/// heads some row.
fn complete(ctors: Option<&[CtorInfo]>, rows: &[Vec<Pat>]) -> Option<Vec<CtorInfo>> {
    let ctors = ctors?;
    let used = heads(rows);
    ctors
        .iter()
        .all(|c| used.contains(&c.key.as_str()))
        .then(|| ctors.to_vec())
}

/// The constructor `key` of a type: from its space, or, for a literal of
/// a type with infinitely many values, a constructor with no fields.
fn ctor_info(ctors: Option<&[CtorInfo]>, key: &str, arity: usize) -> CtorInfo {
    ctors
        .and_then(|cs| cs.iter().find(|c| c.key == key))
        .cloned()
        .unwrap_or_else(|| CtorInfo {
            key: key.to_string(),
            fields: vec![Ty::Error; arity],
            labels: vec![None; arity],
        })
}

fn wilds(ctor: &CtorInfo) -> Vec<Pat> {
    vec![Pat::Wild; ctor.fields.len()]
}

/// The column types after specializing the first column by `ctor`.
fn field_tys(ctor: &CtorInfo, tys: &[Ty]) -> Vec<Ty> {
    ctor.fields.iter().chain(&tys[1..]).cloned().collect()
}

/// The rows that match `ctor` in the first column, with its fields
/// spread into columns of their own.
fn specialize(rows: &[Vec<Pat>], ctor: &CtorInfo) -> Vec<Vec<Pat>> {
    rows.iter()
        .filter_map(|row| {
            let (head, rest) = row.split_first()?;
            let fields = match head {
                Pat::Wild => wilds(ctor),
                Pat::Ctor(key, args) if *key == ctor.key => args.clone(),
                Pat::Ctor(..) => return None,
            };
            Some(fields.into_iter().chain(rest.iter().cloned()).collect())
        })
        .collect()
}

/// The rows whose first column is a wildcard, without it.
fn default(rows: &[Vec<Pat>]) -> Vec<Vec<Pat>> {
    rows.iter()
        .filter(|row| row[0] == Pat::Wild)
        .map(|row| row[1..].to_vec())
        .collect()
}
