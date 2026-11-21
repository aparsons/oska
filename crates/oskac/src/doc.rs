//! `oskac doc`: the API reference of the prelude and the user-facing `std`
//! modules (spec #294), as text or as JSON.
//!
//! The items come from the checker (`oska_check::api`), and whether each
//! runs from the runtime's own lists, the ones that refuse a program with
//! `E0370` or `E0371`. The command reads only the toolchain's embedded
//! sources and writes only to standard output.

use oska_check::api::{self, ApiItem, Kind, PRELUDE};
use oska_syntax::json_escape;

/// Where the reference is published.
pub const SITE: &str = "https://oskalang.dev";

/// One entry of the reference, with whether a program that uses it runs.
pub struct Entry {
    pub item: ApiItem,
    /// `None` when it runs, or why `oskac run` refuses it.
    pub refused: Option<Refusal>,
}

/// Why `oskac run` refuses a program that uses an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Its `std` module has no runtime (`E0371`).
    Module,
    /// A capability it needs has no runtime (`E0370`).
    Capability,
}

impl Refusal {
    /// The code `oskac run` refuses with.
    pub fn code(self) -> &'static str {
        match self {
            Refusal::Module => "E0371",
            Refusal::Capability => "E0370",
        }
    }
}

impl Entry {
    fn runs(&self) -> bool {
        self.refused.is_none()
    }

    /// The item's address on oskalang.dev: its module's page, and its
    /// anchor there.
    pub fn url(&self) -> String {
        let page = format!("{SITE}/api/{}.html", page_name(&self.item.module));
        match self.item.kind {
            Kind::Module => page,
            _ => format!("{page}#{}", self.item.anchor()),
        }
    }
}

/// The file name of a module's page, without `.html`: `std-sql` for
/// `std.sql`.
pub fn page_name(module: &str) -> String {
    module.replace('.', "-")
}

/// Every entry of the reference, in order.
pub fn entries() -> Vec<Entry> {
    api::toolchain_items()
        .into_iter()
        .map(|item| {
            let refused = if item.module != PRELUDE && !oska_run::runs_module(&item.module) {
                Some(Refusal::Module)
            } else if item
                .capabilities
                .iter()
                .any(|c| !oska_run::runs_capability(c))
            {
                Some(Refusal::Capability)
            } else {
                None
            };
            Entry { item, refused }
        })
        .collect()
}

/// The entries `filter` selects: a whole module by its path (`std.sql`),
/// or an item by its name (`List.push`), with a type's or trait's methods
/// after it (`List`). A method named with its impl's type arguments
/// (`Secret<U8>.rotl`) also matches its name without them (`Secret.rotl`,
/// `Secret`). A name may start with its module's path
/// (`std.sql.connect`). `None` when nothing matches.
pub fn select(entries: Vec<Entry>, filter: &str) -> Option<Vec<Entry>> {
    if api::MODULES.contains(&filter) {
        return Some(
            entries
                .into_iter()
                .filter(|e| e.item.module == filter)
                .collect(),
        );
    }
    let (module, name) = match api::MODULES
        .iter()
        .find(|m| filter.starts_with(&format!("{m}.")))
    {
        Some(m) => (Some(*m), &filter[m.len() + 1..]),
        None => (None, filter),
    };
    let selected: Vec<Entry> = entries
        .into_iter()
        .filter(|e| module.map_or(true, |m| e.item.module == m))
        .filter(|e| e.item.kind != Kind::Module)
        .filter(|e| {
            [e.item.name.clone(), e.item.unqualified_name()]
                .iter()
                .any(|n| {
                    n == name
                        || n.strip_prefix(name)
                            .is_some_and(|rest| rest.starts_with('.'))
                })
        })
        .collect();
    (!selected.is_empty()).then_some(selected)
}

/// The text form: a Markdown heading per module and per item, each item's
/// signature as an indented code block, its authority, whether it runs,
/// and its description.
pub fn to_human(entries: &[Entry]) -> String {
    let mut out = String::new();
    let mut module = "";
    for entry in entries {
        let item = &entry.item;
        if item.module != module {
            module = &item.module;
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&format!("# {module}\n"));
            if !modules_run(entries, module) {
                out.push_str(&format!(
                    "\nChecks only: `oskac run` refuses a program that uses `{module}` (E0371).\n"
                ));
            }
        }
        if item.kind == Kind::Module {
            if !item.description.is_empty() {
                out.push_str(&format!("\n{}\n", item.description));
            }
            continue;
        }
        out.push_str(&format!("\n## {} {}\n\n", item.kind.as_str(), item.name));
        out.push_str(&format!("    {}\n\n", item.signature));
        let mut facts = format!("Authority: {}.", item.authority);
        // A module's refusal is said once, under its heading.
        if entry.refused == Some(Refusal::Capability) {
            facts.push_str(&format!(
                " Checks only: `oskac run` refuses a program given it ({}).",
                Refusal::Capability.code()
            ));
        }
        out.push_str(&facts);
        out.push('\n');
        if !item.description.is_empty() {
            out.push_str(&format!("\n{}\n", item.description));
        }
    }
    out
}

/// Whether the module of `entries` named `module` runs: its own entry has
/// no `E0371`.
fn modules_run(entries: &[Entry], module: &str) -> bool {
    entries
        .iter()
        .find(|e| e.item.module == module)
        .map_or(true, |e| e.refused != Some(Refusal::Module))
}

/// The JSON form: `{"items": [...]}`, one object per item and per line,
/// with the keys `module`, `name`, `kind`, `signature`, `description`,
/// `authority`, `runs` and `url`. `docs/guide/cli-reference.md` documents
/// the shape.
pub fn to_json(entries: &[Entry]) -> String {
    let items: Vec<String> = entries
        .iter()
        .map(|entry| {
            let item = &entry.item;
            format!(
                "{{\"module\":\"{}\",\"name\":\"{}\",\"kind\":\"{}\",\"signature\":\"{}\",\"description\":\"{}\",\"authority\":\"{}\",\"runs\":{},\"url\":\"{}\"}}",
                json_escape(&item.module),
                json_escape(&item.name),
                item.kind.as_str(),
                json_escape(&item.signature),
                json_escape(&item.description),
                json_escape(&item.authority),
                entry.runs(),
                json_escape(&entry.url())
            )
        })
        .collect();
    if items.is_empty() {
        return "{\"items\": []}\n".to_string();
    }
    format!("{{\"items\": [\n  {}\n]}}\n", items.join(",\n  "))
}
