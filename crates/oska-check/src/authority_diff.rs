//! The **authority baseline** and the **authority diff**: what `oskac
//! check --update-baseline` writes and `oskac check --baseline` compares,
//! and the one comparison every tool uses to tell whether a change widened
//! a program's authority (`oskac fix`'s no-widening rule included).
//!
//! An [`AuthorityRecord`] is a report's authority without places: each
//! function's authority and purity, and audit sites and attenuations
//! counted per function. It comes from a [`CheckReport`]
//! ([`AuthorityRecord::of`]) or from a written baseline or a saved `oskac
//! check --format=json` report ([`AuthorityRecord::read`]).
//! [`AuthorityRecord::to_baseline`] writes it sorted, with LF line
//! endings, so the file changes only when authority does.
//!
//! [`diff`] compares a baseline with a current record by function name
//! and returns an [`AuthorityDiff`]; [`AuthorityDiff::fails`] is the gate.
//! A function's authority here is its report `authority` plus an entry
//! for each capability-bearing parameter that names no built-in kind:
//! `cap fn` for a `cap fn` parameter and `generic T` for a bounded type
//! parameter (#149), so gaining one is a widening even for a function
//! that was already impure.
//!
//! Built-in kinds are compared by [`covers`]: a kind covers every kind it
//! can be attenuated into, transitively, read from the prelude's
//! attenuation methods (`body/attenuation.rs`). Every other entry covers
//! only itself. Anything the reader cannot account for is an error, never
//! a pass.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use oska_syntax::ast::{ItemKind, TypeKind};
use oska_syntax::json_escape;
use oska_syntax::{parse_json, JsonValue as Json};

use crate::{builtins, json_lines, json_strings, prelude, CheckReport};

/// The baseline file format this toolchain writes and reads.
pub const FORMAT: u64 = 1;

/// The prefix of a test's name, `test "<name>"`, where its audit sites
/// and attenuations are listed. No function's name can start with it.
const TEST_PREFIX: &str = "test \"";

/// A report's authority, without places.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthorityRecord {
    /// The `oskac` version that wrote the baseline, when it says.
    pub oskac: Option<String>,
    /// Every function, in report order. Names should be unique; [`diff`]
    /// and [`AuthorityRecord::to_baseline`] refuse a record whose are not.
    pub functions: Vec<FunctionRecord>,
    /// Audit sites: how many of each kind each function has.
    pub audit: BTreeMap<AuditKey, u64>,
    /// Attenuations: how many of each each function makes.
    pub attenuations: BTreeMap<AttenuationKey, u64>,
}

/// One function's authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionRecord {
    pub name: String,
    /// Its authority entries: built-in kinds, `cap fn` and `generic T`.
    pub authority: BTreeSet<String>,
    pub pure: bool,
}

/// An audit site's function and kind, such as `assume_trusted`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AuditKey {
    pub function: String,
    pub kind: String,
}

/// An attenuation, without its place.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AttenuationKey {
    pub function: String,
    pub method: String,
    pub source: String,
    pub result: String,
    /// The literal root, or `None` when there is none or it is computed.
    pub root: Option<String>,
}

impl AuthorityRecord {
    /// The authority `report` records.
    pub fn of(report: &CheckReport) -> AuthorityRecord {
        let functions = report
            .functions
            .iter()
            .map(|f| {
                let mut authority: BTreeSet<String> = f.authority.iter().cloned().collect();
                for via in &f.via {
                    authority.extend(via_entries(&via.capabilities, &via.generics));
                }
                FunctionRecord {
                    name: f.name.clone(),
                    authority,
                    pure: f.pure,
                }
            })
            .collect();
        let mut audit = BTreeMap::new();
        for site in &report.audit {
            let key = AuditKey {
                function: site.function.clone(),
                kind: site.kind.as_str().to_string(),
            };
            *audit.entry(key).or_insert(0) += 1;
        }
        let mut attenuations = BTreeMap::new();
        for a in &report.attenuations {
            let key = AttenuationKey {
                function: a.function.clone(),
                method: a.method.clone(),
                source: a.source.clone(),
                result: a.result.clone(),
                root: a.root_text().map(str::to_string),
            };
            *attenuations.entry(key).or_insert(0) += 1;
        }
        AuthorityRecord {
            oskac: None,
            functions,
            audit,
            attenuations,
        }
    }

    /// Read a baseline that [`AuthorityRecord::to_baseline`] wrote, or a
    /// saved `oskac check --format=json` report: its `functions`, `audit`
    /// and `attenuations`. Every other key is ignored, except that a
    /// `format` other than [`FORMAT`] and a report with diagnostics, whose
    /// authority is incomplete, are refused.
    pub fn read(text: &str) -> Result<AuthorityRecord, String> {
        let json = parse_json(text)?;
        if !matches!(json, Json::Object(_)) {
            return Err("expected a JSON object".to_string());
        }
        // The reader keeps the first of a repeated key; a second could
        // hide a widening from a reviewer reading the file.
        if let Some(key) = json.duplicate_key() {
            return Err(format!("the key `{key}` is given twice in one object"));
        }
        if let Some(format) = json.get("format") {
            if format.as_u64() != Some(FORMAT) {
                return Err(format!(
                    "unsupported baseline format (this oskac reads format {FORMAT})"
                ));
            }
        }
        if let Some(diagnostics) = json.get("diagnostics") {
            match diagnostics.as_array() {
                Some([]) => {}
                _ => {
                    return Err(
                        "the report has diagnostics, so its authority is incomplete".to_string()
                    )
                }
            }
        }
        let oskac = match json.get("oskac") {
            None => None,
            Some(version) => Some(text_of(Some(version), "oskac")?),
        };
        let functions = array(&json, "functions")?
            .iter()
            .map(read_function)
            .collect::<Result<_, _>>()?;
        let mut audit = BTreeMap::new();
        for site in array(&json, "audit")? {
            let key = AuditKey {
                function: text_of(site.get("function"), "audit entry's `function`")?,
                kind: text_of(site.get("kind"), "audit entry's `kind`")?,
            };
            add_count(&mut audit, key, count_of(site)?)?;
        }
        let mut attenuations = BTreeMap::new();
        for a in array(&json, "attenuations")? {
            let field = |name: &str| text_of(a.get(name), &format!("attenuation's `{name}`"));
            let root = match a.get("root") {
                Some(Json::Null) => None,
                root => Some(text_of(root, "attenuation's `root`")?),
            };
            let key = AttenuationKey {
                function: field("function")?,
                method: field("method")?,
                source: field("source")?,
                result: field("result")?,
                root,
            };
            add_count(&mut attenuations, key, count_of(a)?)?;
        }
        Ok(AuthorityRecord {
            oskac,
            functions,
            audit,
            attenuations,
        })
    }

    /// The baseline file: `format`, the `oskac` version that writes it,
    /// and the record, sorted, with no spans or paths, one entry per line,
    /// LF line endings and a final newline. A record with two functions
    /// of the same name is refused, as [`diff`] refuses it.
    pub fn to_baseline(&self, oskac_version: &str) -> Result<String, String> {
        unique_names(self)?;
        let mut functions: Vec<&FunctionRecord> = self.functions.iter().collect();
        functions.sort_by(|a, b| a.name.cmp(&b.name));
        let functions: Vec<String> = functions
            .iter()
            .map(|f| {
                format!(
                    "{{\"name\":\"{}\",\"authority\":{},\"pure\":{}}}",
                    json_escape(&f.name),
                    strings(&f.authority),
                    f.pure
                )
            })
            .collect();
        let audit: Vec<String> = self
            .audit
            .iter()
            .map(|(key, count)| format!("{},\"count\":{count}}}", audit_json(key)))
            .collect();
        let attenuations: Vec<String> = self
            .attenuations
            .iter()
            .map(|(key, count)| format!("{},\"count\":{count}}}", attenuation_json(key)))
            .collect();
        Ok(format!(
            "{{\n  \"format\": {FORMAT},\n  \"oskac\": \"{}\",\n  \"functions\": {},\n  \"audit\": {},\n  \"attenuations\": {}\n}}\n",
            json_escape(oskac_version),
            json_lines(&functions),
            json_lines(&audit),
            json_lines(&attenuations)
        ))
    }

    /// Every authority entry any function has.
    fn reach(&self) -> BTreeSet<&str> {
        self.functions
            .iter()
            .flat_map(|f| f.authority.iter().map(String::as_str))
            .collect()
    }
}

/// The entries a capability-bearing parameter adds beyond the built-in
/// kinds it names: `generic T` for each bounded type parameter, and
/// `cap fn` for a parameter that names nothing else.
fn via_entries(capabilities: &[String], generics: &[String]) -> Vec<String> {
    let mut entries: Vec<String> = capabilities.to_vec();
    entries.extend(generics.iter().map(|g| format!("generic {g}")));
    if entries.is_empty() {
        entries.push("cap fn".to_string());
    }
    entries
}

fn read_function(entry: &Json) -> Result<FunctionRecord, String> {
    let name = text_of(entry.get("name"), "function's `name`")?;
    let mut authority: BTreeSet<String> =
        texts_of(entry.get("authority"), "function's `authority`")?
            .into_iter()
            .collect();
    let pure = entry
        .get("pure")
        .and_then(Json::as_bool)
        .ok_or_else(|| format!("function `{name}` has no boolean `pure`"))?;
    if let Some(via) = entry.get("via") {
        let via = via
            .as_array()
            .ok_or_else(|| format!("function `{name}`'s `via` is not an array"))?;
        for param in via {
            let capabilities = texts_of(param.get("capabilities"), "`via` entry's `capabilities`")?;
            let generics = match param.get("generics") {
                None => Vec::new(),
                generics => texts_of(generics, "`via` entry's `generics`")?,
            };
            authority.extend(via_entries(&capabilities, &generics));
        }
    }
    Ok(FunctionRecord {
        name,
        authority,
        pure,
    })
}

fn array<'j>(json: &'j Json, key: &str) -> Result<&'j [Json], String> {
    json.get(key)
        .and_then(Json::as_array)
        .ok_or_else(|| format!("expected a `{key}` array"))
}

fn text_of(value: Option<&Json>, what: &str) -> Result<String, String> {
    value
        .and_then(Json::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("expected a string for the {what}"))
}

fn texts_of(value: Option<&Json>, what: &str) -> Result<Vec<String>, String> {
    let items = value
        .and_then(Json::as_array)
        .ok_or_else(|| format!("expected an array of strings for the {what}"))?;
    items.iter().map(|item| text_of(Some(item), what)).collect()
}

/// An entry's `count`: 1 when absent (a report lists each site), and
/// otherwise a positive integer.
fn count_of(entry: &Json) -> Result<u64, String> {
    match entry.get("count") {
        None => Ok(1),
        Some(count) => count
            .as_u64()
            .filter(|&n| n > 0)
            .ok_or_else(|| "expected a positive integer `count`".to_string()),
    }
}

fn add_count<K: Ord>(map: &mut BTreeMap<K, u64>, key: K, count: u64) -> Result<(), String> {
    let total = map.entry(key).or_insert(0);
    *total = total
        .checked_add(count)
        .ok_or_else(|| "a `count` is too large".to_string())?;
    Ok(())
}

/// Refuse a record in which two functions share a name: one could hide
/// the other (#123).
fn unique_names(record: &AuthorityRecord) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    let mut duplicates = BTreeSet::new();
    for f in &record.functions {
        if !seen.insert(f.name.as_str()) {
            duplicates.insert(f.name.as_str());
        }
    }
    if duplicates.is_empty() {
        return Ok(());
    }
    let names: Vec<String> = duplicates.iter().map(|n| format!("`{n}`")).collect();
    Err(format!(
        "more than one function is named {}, so the authority diff cannot tell them apart",
        names.join(", ")
    ))
}

// ---- Capability coverage ---------------------------------------------------------

/// Whether the authority entry `old` covers `new`: they are equal, or
/// both are built-in kinds and `old` can be attenuated into `new`.
pub fn covers(old: &str, new: &str) -> bool {
    old == new || coverage().get(old).is_some_and(|kinds| kinds.contains(new))
}

/// Each built-in kind and the kinds it can be attenuated into,
/// transitively: the prelude's attenuation methods on a built-in
/// capability that return one.
fn coverage() -> &'static BTreeMap<String, BTreeSet<String>> {
    static COVERAGE: OnceLock<BTreeMap<String, BTreeSet<String>>> = OnceLock::new();
    COVERAGE.get_or_init(|| {
        let mut direct: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for item in &prelude::module().items {
            let ItemKind::Impl(imp) = &item.kind else {
                continue;
            };
            let (Some(source), None) = (capability_name(&imp.target.kind), &imp.trait_ref) else {
                continue;
            };
            for method in &imp.methods {
                let ItemKind::Fn(f) = &method.kind else {
                    continue;
                };
                let attenuates = crate::body::attenuation::METHODS
                    .iter()
                    .any(|(name, _)| *name == f.name.name);
                let result = f
                    .return_type
                    .as_ref()
                    .and_then(|t| capability_name(&t.kind));
                if let (true, Some(result)) = (attenuates, result) {
                    direct
                        .entry(source.to_string())
                        .or_default()
                        .insert(result.to_string());
                }
            }
        }
        let mut closed = direct.clone();
        loop {
            let mut grew = false;
            for kinds in closed.values_mut() {
                let reachable: Vec<String> = kinds
                    .iter()
                    .filter_map(|k| direct.get(k))
                    .flatten()
                    .cloned()
                    .collect();
                for kind in reachable {
                    grew |= kinds.insert(kind);
                }
            }
            if !grew {
                return closed;
            }
        }
    })
}

/// The built-in capability a type names, for a plain `Name`.
fn capability_name(kind: &TypeKind) -> Option<&str> {
    match kind {
        TypeKind::Path { segments, args } if segments.len() == 1 && args.is_empty() => {
            let name = segments[0].name.as_str();
            builtins::is_capability(name).then_some(name)
        }
        _ => None,
    }
}

// ---- The diff --------------------------------------------------------------------

/// How the current record differs from the baseline. Every list is
/// sorted by function name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthorityDiff {
    /// The `oskac` version that wrote the baseline, when it says.
    pub baseline_oskac: Option<String>,
    /// Functions whose authority gained entries no old entry covers.
    pub widened: Vec<Change>,
    /// Functions that lost entries, or gained only entries an old one
    /// covers.
    pub narrowed: Vec<Change>,
    /// Functions that were pure and are not.
    pub became_impure: Vec<String>,
    pub added: Vec<Added>,
    pub removed: Vec<FunctionRecord>,
    pub new_audit_sites: Vec<(AuditKey, u64)>,
    pub removed_audit_sites: Vec<(AuditKey, u64)>,
    pub new_attenuations: Vec<(AttenuationKey, u64)>,
    pub removed_attenuations: Vec<(AttenuationKey, u64)>,
}

/// A function in both records whose authority changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub function: String,
    pub gained: Vec<String>,
    pub lost: Vec<String>,
}

/// A function only in the current record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Added {
    pub function: FunctionRecord,
    /// Its entries that no function in the baseline covers: the program
    /// as a whole can now reach more.
    pub new_kinds: Vec<String>,
}

/// Compare `current` with `baseline`. Functions are matched by name only:
/// a rename is a removal and an addition. Two functions of the same name
/// in either record are an error.
pub fn diff(
    baseline: &AuthorityRecord,
    current: &AuthorityRecord,
) -> Result<AuthorityDiff, String> {
    unique_names(baseline).map_err(|e| format!("in the baseline, {e}"))?;
    unique_names(current)?;
    let old: BTreeMap<&str, &FunctionRecord> = baseline
        .functions
        .iter()
        .map(|f| (f.name.as_str(), f))
        .collect();
    let new: BTreeMap<&str, &FunctionRecord> = current
        .functions
        .iter()
        .map(|f| (f.name.as_str(), f))
        .collect();
    let reach = baseline.reach();
    let mut out = AuthorityDiff {
        baseline_oskac: baseline.oskac.clone(),
        ..AuthorityDiff::default()
    };
    for (name, now) in &new {
        let Some(before) = old.get(name) else {
            let new_kinds = now
                .authority
                .iter()
                .filter(|e| !reach.iter().any(|r| covers(r, e)))
                .cloned()
                .collect();
            out.added.push(Added {
                function: (*now).clone(),
                new_kinds,
            });
            continue;
        };
        let gained: Vec<&String> = now.authority.difference(&before.authority).collect();
        let lost: Vec<String> = before
            .authority
            .difference(&now.authority)
            .cloned()
            .collect();
        let (widening, within): (Vec<&String>, Vec<&String>) = gained
            .into_iter()
            .partition(|e| !before.authority.iter().any(|o| covers(o, e)));
        if !widening.is_empty() {
            out.widened.push(Change {
                function: name.to_string(),
                gained: widening.into_iter().cloned().collect(),
                lost: Vec::new(),
            });
        }
        if !lost.is_empty() || !within.is_empty() {
            out.narrowed.push(Change {
                function: name.to_string(),
                gained: within.into_iter().cloned().collect(),
                lost,
            });
        }
        if before.pure && !now.pure {
            out.became_impure.push(name.to_string());
        }
    }
    out.removed = old
        .iter()
        .filter(|(name, _)| !new.contains_key(*name))
        .map(|(_, f)| (*f).clone())
        .collect();
    (out.new_audit_sites, out.removed_audit_sites) = counted(&baseline.audit, &current.audit);
    (out.new_attenuations, out.removed_attenuations) =
        counted(&baseline.attenuations, &current.attenuations);
    Ok(out)
}

/// The keys whose count grew from `old` to `new`, by how much, and those
/// whose count fell.
type Counted<K> = (Vec<(K, u64)>, Vec<(K, u64)>);

fn counted<K: Ord + Clone>(old: &BTreeMap<K, u64>, new: &BTreeMap<K, u64>) -> Counted<K> {
    let keys: BTreeSet<&K> = old.keys().chain(new.keys()).collect();
    let mut grew = Vec::new();
    let mut fell = Vec::new();
    for key in keys {
        let before = old.get(key).copied().unwrap_or(0);
        let after = new.get(key).copied().unwrap_or(0);
        if after > before {
            grew.push((key.clone(), after - before));
        } else if before > after {
            fell.push((key.clone(), before - after));
        }
    }
    (grew, fell)
}

/// Whether `function` names a test block, whose entries never fail the
/// gate.
fn is_test(function: &str) -> bool {
    function.starts_with(TEST_PREFIX)
}

impl AuthorityDiff {
    /// Whether the gate fails: a function widened or became impure, an
    /// audit site outside a test was added, or an added function reaches
    /// a kind no function in the baseline did.
    pub fn fails(&self) -> bool {
        !self.widenings().is_empty()
    }

    /// Each change that fails the gate, said in a few words, such as
    /// "`fn q` gains `NetConnect`": empty when there is none. `oskac fix`
    /// names these when it refuses to write.
    pub fn widenings(&self) -> Vec<String> {
        let list = |entries: &[String]| {
            entries
                .iter()
                .map(|e| format!("`{e}`"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let mut out = Vec::new();
        for c in &self.widened {
            out.push(format!(
                "`{}` gains {}",
                fn_name(&c.function),
                list(&c.gained)
            ));
        }
        for f in &self.became_impure {
            out.push(format!("`{}` is no longer pure", fn_name(f)));
        }
        for a in self.added.iter().filter(|a| !a.new_kinds.is_empty()) {
            out.push(format!(
                "the new `{}` reaches {}, which no function did",
                fn_name(&a.function.name),
                list(&a.new_kinds)
            ));
        }
        for (key, more) in &self.new_audit_sites {
            if is_test(&key.function) {
                continue;
            }
            let sites = if *more == 1 { "site" } else { "sites" };
            out.push(format!(
                "{more} new `{}` audit {sites} in `{}`",
                key.kind,
                fn_name(&key.function)
            ));
        }
        out
    }

    /// Whether nothing changed.
    pub fn is_empty(&self) -> bool {
        self.widened.is_empty()
            && self.narrowed.is_empty()
            && self.became_impure.is_empty()
            && self.added.is_empty()
            && self.removed.is_empty()
            && self.new_audit_sites.is_empty()
            && self.removed_audit_sites.is_empty()
            && self.new_attenuations.is_empty()
            && self.removed_attenuations.is_empty()
    }

    /// The diff as the JSON value of `check`'s `authority_diff`, for the
    /// baseline read from `baseline`.
    pub fn to_json(&self, baseline: &str) -> String {
        let widened = |c: &Change| {
            format!(
                "{{\"function\":\"{}\",\"gained\":{}}}",
                json_escape(&c.function),
                strings(&c.gained)
            )
        };
        let narrowed = |c: &Change| {
            format!(
                "{{\"function\":\"{}\",\"gained\":{},\"lost\":{}}}",
                json_escape(&c.function),
                strings(&c.gained),
                strings(&c.lost)
            )
        };
        let function = |f: &FunctionRecord| {
            format!(
                "\"function\":\"{}\",\"authority\":{},\"pure\":{}",
                json_escape(&f.name),
                strings(&f.authority),
                f.pure
            )
        };
        let fields = [
            ("widened", self.widened.iter().map(widened).collect()),
            ("narrowed", self.narrowed.iter().map(narrowed).collect()),
            (
                "became_impure",
                self.became_impure
                    .iter()
                    .map(|f| format!("{{\"function\":\"{}\"}}", json_escape(f)))
                    .collect(),
            ),
            (
                "added",
                self.added
                    .iter()
                    .map(|a| {
                        format!(
                            "{{{},\"new_kinds\":{}}}",
                            function(&a.function),
                            strings(&a.new_kinds)
                        )
                    })
                    .collect(),
            ),
            (
                "removed",
                self.removed
                    .iter()
                    .map(|f| format!("{{{}}}", function(f)))
                    .collect(),
            ),
            ("new_audit_sites", audit_counts(&self.new_audit_sites)),
            (
                "removed_audit_sites",
                audit_counts(&self.removed_audit_sites),
            ),
            (
                "new_attenuations",
                attenuation_counts(&self.new_attenuations),
            ),
            (
                "removed_attenuations",
                attenuation_counts(&self.removed_attenuations),
            ),
        ];
        let mut out = format!("{{\n    \"baseline\": \"{}\",\n", json_escape(baseline));
        match &self.baseline_oskac {
            Some(version) => out.push_str(&format!(
                "    \"baseline_oskac\": \"{}\",\n",
                json_escape(version)
            )),
            None => out.push_str("    \"baseline_oskac\": null,\n"),
        }
        for (key, items) in fields {
            let items: Vec<String> = items;
            let items = if items.is_empty() {
                "[]".to_string()
            } else {
                format!("[\n      {}\n    ]", items.join(",\n      "))
            };
            out.push_str(&format!("    \"{key}\": {items},\n"));
        }
        out.push_str(&format!("    \"fails\": {}\n  }}", self.fails()));
        out
    }

    /// The `baseline:` section of the human report, for the baseline
    /// read from `baseline`, running as `oskac` version `oskac_version`:
    /// one line per change, those that fail the gate first, each marked
    /// with what it gained (`+`) or lost (`-`).
    pub fn to_human(&self, baseline: &str, oskac_version: &str) -> String {
        let mut out = format!("baseline: {baseline}\n");
        if self.is_empty() {
            out.push_str("  no change in authority\n");
            return out;
        }
        let mut line = |text: String| out.push_str(&format!("  {text}\n"));
        for c in &self.widened {
            line(format!(
                "widened {}: {}",
                fn_name(&c.function),
                marked(&c.gained, &[])
            ));
        }
        for f in &self.became_impure {
            line(format!("became impure {}", fn_name(f)));
        }
        for (key, count) in &self.new_audit_sites {
            let test = if is_test(&key.function) {
                " (in a test: not counted)"
            } else {
                ""
            };
            line(format!(
                "new audit site {}: {} +{count}{test}",
                fn_name(&key.function),
                key.kind
            ));
        }
        for a in self.added.iter().filter(|a| !a.new_kinds.is_empty()) {
            line(format!(
                "added {}: {} (new to the program: {})",
                fn_name(&a.function.name),
                authority_text(&a.function),
                a.new_kinds.join(", ")
            ));
        }
        for a in self.added.iter().filter(|a| a.new_kinds.is_empty()) {
            line(format!(
                "added {}: {}",
                fn_name(&a.function.name),
                authority_text(&a.function)
            ));
        }
        for c in &self.narrowed {
            line(format!(
                "narrowed {}: {}",
                fn_name(&c.function),
                marked(&c.gained, &c.lost)
            ));
        }
        for f in &self.removed {
            line(format!(
                "removed {}: {}",
                fn_name(&f.name),
                authority_text(f)
            ));
        }
        for (key, count) in &self.removed_audit_sites {
            line(format!(
                "removed audit site {}: {} -{count}",
                fn_name(&key.function),
                key.kind
            ));
        }
        for (key, count) in &self.new_attenuations {
            line(format!(
                "new attenuation {}: {} +{count}",
                fn_name(&key.function),
                attenuation_text(key)
            ));
        }
        for (key, count) in &self.removed_attenuations {
            line(format!(
                "removed attenuation {}: {} -{count}",
                fn_name(&key.function),
                attenuation_text(key)
            ));
        }
        if let Some(version) = self
            .baseline_oskac
            .as_deref()
            .filter(|v| *v != oskac_version)
        {
            line(format!(
                "note: the baseline was written by oskac {version} and this is oskac \
                 {oskac_version}; a change in the authority report between releases shows \
                 here too (see the release notes)"
            ));
        }
        out
    }
}

/// `fn name`, or a test's name as it is, `test "<name>"`.
fn fn_name(function: &str) -> String {
    if is_test(function) {
        function.to_string()
    } else {
        format!("fn {function}")
    }
}

fn marked(gained: &[String], lost: &[String]) -> String {
    let mut marks: Vec<String> = gained.iter().map(|g| format!("+{g}")).collect();
    marks.extend(lost.iter().map(|l| format!("-{l}")));
    marks.join(", ")
}

fn authority_text(f: &FunctionRecord) -> String {
    if f.pure {
        "pure".to_string()
    } else {
        f.authority.iter().cloned().collect::<Vec<_>>().join(", ")
    }
}

fn attenuation_text(key: &AttenuationKey) -> String {
    let root = key
        .root
        .as_ref()
        .map_or_else(String::new, |r| format!(" {r:?}"));
    format!("{} -> {}: {}{root}", key.source, key.result, key.method)
}

fn strings<'a>(items: impl IntoIterator<Item = &'a String>) -> String {
    let items: Vec<String> = items.into_iter().cloned().collect();
    json_strings(&items)
}

/// An audit key as an unclosed JSON object, so a count can follow.
fn audit_json(key: &AuditKey) -> String {
    format!(
        "{{\"function\":\"{}\",\"kind\":\"{}\"",
        json_escape(&key.function),
        json_escape(&key.kind)
    )
}

/// An attenuation key as an unclosed JSON object, so a count can follow.
fn attenuation_json(key: &AttenuationKey) -> String {
    let root = key
        .root
        .as_ref()
        .map_or_else(|| "null".to_string(), |r| format!("\"{}\"", json_escape(r)));
    format!(
        "{{\"function\":\"{}\",\"method\":\"{}\",\"source\":\"{}\",\"result\":\"{}\",\"root\":{root}",
        json_escape(&key.function),
        json_escape(&key.method),
        json_escape(&key.source),
        json_escape(&key.result)
    )
}

fn audit_counts(items: &[(AuditKey, u64)]) -> Vec<String> {
    items
        .iter()
        .map(|(key, count)| format!("{},\"count\":{count}}}", audit_json(key)))
        .collect()
}

fn attenuation_counts(items: &[(AttenuationKey, u64)]) -> Vec<String> {
    items
        .iter()
        .map(|(key, count)| format!("{},\"count\":{count}}}", attenuation_json(key)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{covers, diff, AuthorityRecord};
    use crate::{AuditKind, AuditSite, CheckReport, FunctionAuthority};
    use oska_syntax::Span;

    fn function(name: &str, authority: &[&str]) -> FunctionAuthority {
        FunctionAuthority {
            name: name.to_string(),
            public: false,
            authority: authority.iter().map(|a| a.to_string()).collect(),
            via: Vec::new(),
            pure: authority.is_empty(),
        }
    }

    fn report(functions: Vec<FunctionAuthority>, audit: &[(&str, AuditKind)]) -> CheckReport {
        CheckReport {
            functions,
            audit: audit
                .iter()
                .map(|(function, kind)| AuditSite {
                    function: function.to_string(),
                    kind: *kind,
                    span: Span::at(0),
                    location: None,
                })
                .collect(),
            ..CheckReport::default()
        }
    }

    fn widenings(before: &CheckReport, after: &CheckReport) -> Vec<String> {
        diff(&AuthorityRecord::of(before), &AuthorityRecord::of(after))
            .expect("the names are unique")
            .widenings()
    }

    #[test]
    fn the_same_authority_is_no_widening() {
        let before = report(vec![function("main", &["Stdout"])], &[]);
        assert_eq!(widenings(&before, &before.clone()), Vec::<String>::new());
    }

    #[test]
    fn a_function_that_gains_an_entry_or_becomes_impure_widens() {
        let before = report(vec![function("main", &["Stdout"]), function("q", &[])], &[]);
        let after = report(
            vec![
                function("main", &["Stdout", "Net"]),
                function("q", &["Clock"]),
            ],
            &[],
        );
        assert_eq!(
            widenings(&before, &after),
            [
                "`fn main` gains `Net`",
                "`fn q` gains `Clock`",
                "`fn q` is no longer pure"
            ]
        );
    }

    #[test]
    fn a_new_function_widens_only_with_a_kind_the_program_never_covered() {
        let before = report(vec![function("main", &["Fs", "Stdout"])], &[]);
        let after = report(
            vec![
                function("main", &["Fs", "Stdout"]),
                function("helper", &["DirRead"]),
                function("fetch", &["Net"]),
            ],
            &[],
        );
        assert_eq!(
            widenings(&before, &after),
            ["the new `fn fetch` reaches `Net`, which no function did"]
        );
    }

    #[test]
    fn narrowing_and_removal_are_not_widenings() {
        let before = report(
            vec![function("main", &["Stdout", "Fs"]), function("old", &[])],
            &[],
        );
        let after = report(vec![function("main", &["Stdout", "DirRead"])], &[]);
        assert_eq!(widenings(&before, &after), Vec::<String>::new());
    }

    #[test]
    fn a_new_audit_site_widens_outside_a_test_and_a_removed_one_does_not() {
        let one = report(vec![], &[("f", AuditKind::TrustAssertion)]);
        let two = report(
            vec![],
            &[
                ("f", AuditKind::TrustAssertion),
                ("f", AuditKind::TrustAssertion),
                ("test \"t\"", AuditKind::SecretExposure),
            ],
        );
        assert_eq!(
            widenings(&one, &two),
            ["1 new `assume_trusted` audit site in `fn f`"]
        );
        assert_eq!(widenings(&two, &one), Vec::<String>::new());
    }

    #[test]
    fn duplicate_names_are_an_error() {
        let twice = report(vec![function("f", &[]), function("f", &["Net"])], &[]);
        let once = report(vec![function("f", &[])], &[]);
        assert!(diff(&AuthorityRecord::of(&once), &AuthorityRecord::of(&twice)).is_err());
        assert!(diff(&AuthorityRecord::of(&twice), &AuthorityRecord::of(&once)).is_err());
    }

    #[test]
    fn coverage_follows_attenuation_transitively() {
        assert!(covers("Fs", "DirRead"));
        assert!(covers("Fs", "FileWrite"));
        assert!(covers("FsRead", "FileRead"));
        assert!(!covers("DirRead", "Fs"));
        assert!(!covers("FsRead", "DirWrite"));
        assert!(!covers("Stdout", "Net"));
        assert!(!covers("cap fn", "generic T"));
        assert!(covers("generic T", "generic T"));
    }
}
