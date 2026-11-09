//! Type checker for the Oska programming language.
//!
//! [`check_module`] checks one source file against the embedded
//! **prelude** (`src/prelude.osk`) and returns a [`CheckReport`]: every
//! diagnostic, plus the **authority** of every function. The report is
//! produced even when there are errors. [`CheckReport::to_json`] is what
//! `oskac check --format=json` prints; `docs/checker.md` describes it.
//!
//! The pipeline runs in order: collect declarations, resolve names, check
//! declarations (`cap`, `main`, capitalization, recursive types), check
//! function bodies (`body.rs`, with local bidirectional inference), then
//! compute authority. Under `--release`, bodies are also scanned for
//! `dbg(...)`.
//!
//! # Diagnostic codes
//!
//! Checker codes are `E02xx`. Each is listed here and in
//! `docs/checker.md`.
//!
//! | Code | Meaning |
//! | --- | --- |
//! | `E0200` | Unknown type or trait name. The fix replaces it with the single closest name, when there is one. |
//! | `E0201` | A trait used where a type is expected, or a type used where a trait is expected. |
//! | `E0202` | A type or trait name declared twice, or one that redeclares a prelude name. |
//! | `E0210` | A struct or enum whose own fields name a capability-bearing type is not declared `cap`. The fix inserts `cap`. |
//! | `E0211` | A `main` parameter that is not a built-in capability or `Args`. |
//! | `E0212` | `main` returns something other than `()` or `Result<(), E>`, or `E` does not implement `Display`. |
//! | `E0213` | A type, variant, trait, or type parameter name that does not start uppercase. |
//! | `E0214` | A function, field, or parameter name that starts uppercase. |
//! | `E0215` | A recursive struct or enum with no base case. |
//! | `E0216` | `dbg(...)` under `--release`. |
//! | `E0220` | Type mismatch: an expression's type differs from the type expected there. There are no implicit conversions. |
//! | `E0221` | Unknown value name, or `self` outside a method with a receiver. |
//! | `E0222` | Integer literal out of range for its type, such as `300` as a `U8`. |
//! | `E0223` | Operator not defined for its operand's type: arithmetic and ordering need numbers (`+` is not for strings), unary `-` needs a signed number, `..` needs integers. |
//! | `E0224` | Wrong number of arguments in a call or construction. |
//! | `E0225` | Argument label that does not match its parameter or field, or a label on a positional field. |
//! | `E0226` | Named field constructed without its label. |
//! | `E0227` | Unknown field, or a method used without calling it. |
//! | `E0228` | Unknown method, variant, or associated function, or an associated function called on a value. |
//! | `E0229` | Assignment to something that is not a `var` binding: a `let` binding, a parameter, or a field of `self` in a method without `var self`. |
//! | `E0230` | Mutating method (`var self`) called on something that is not a `var` binding. |
//! | `E0231` | Call of a value that is not a function. |
//! | `E0232` | `break` or `continue` outside a loop. |
//! | `E0233` | A type that inference cannot work out, such as `let xs = List.new()` with no later use. |
//! | `E0234` | A type name used as a value, or a type that cannot be constructed. |
//! | `E0235` | `for` over a value that is not a `List` or a range. |
//! | `E0240` | A `Result` dropped by an expression statement. `let _ = f()` discards one deliberately. |
//! | `E0241` | `?` on a value that is neither a `Result` nor an `Option`. |
//! | `E0242` | `?` on a `Result` whose error type differs from the function's, with no `impl From` conversion. |
//! | `E0243` | `?` in a function that does not return the same kind (`Option` vs `Result`), or returns neither. |
//! | `E0270` | A `cap fn` used where a plain `fn` is expected, such as a closure that captures a capability-bearing value. |
//! | `E0271` | Construction of a prelude struct with private fields, such as a built-in capability: capabilities cannot be forged. |
//! | `E0272` | Reading a private field of a prelude type. |

use oska_syntax::Diagnostic;

mod authority;
mod body;
mod declarations;
mod decls;
mod infer;
mod items;
mod prelude;
mod recursion;
mod release;
mod resolve;
mod ty;

/// The diagnostic codes this crate reports.
pub(crate) mod codes {
    pub const UNKNOWN_NAME: &str = "E0200";
    pub const WRONG_NAMESPACE: &str = "E0201";
    pub const DUPLICATE_NAME: &str = "E0202";
    pub const CAP_REQUIRED: &str = "E0210";
    pub const MAIN_PARAM: &str = "E0211";
    pub const MAIN_RETURN: &str = "E0212";
    pub const NOT_UPPERCASE: &str = "E0213";
    pub const NOT_LOWERCASE: &str = "E0214";
    pub const NO_BASE_CASE: &str = "E0215";
    pub const DBG_IN_RELEASE: &str = "E0216";
    pub const MISMATCH: &str = "E0220";
    pub const UNKNOWN_VALUE: &str = "E0221";
    pub const LITERAL_OUT_OF_RANGE: &str = "E0222";
    pub const BAD_OPERAND: &str = "E0223";
    pub const ARGUMENT_COUNT: &str = "E0224";
    pub const BAD_LABEL: &str = "E0225";
    pub const MISSING_LABEL: &str = "E0226";
    pub const NO_FIELD: &str = "E0227";
    pub const NO_MEMBER: &str = "E0228";
    pub const ASSIGN_TO_LET: &str = "E0229";
    pub const MUTATING_CALL_ON_LET: &str = "E0230";
    pub const NOT_CALLABLE: &str = "E0231";
    pub const OUTSIDE_LOOP: &str = "E0232";
    pub const CANNOT_INFER: &str = "E0233";
    pub const NOT_A_VALUE: &str = "E0234";
    pub const NOT_ITERABLE: &str = "E0235";
    pub const IGNORED_RESULT: &str = "E0240";
    pub const TRY_OPERAND: &str = "E0241";
    pub const TRY_ERROR_TYPE: &str = "E0242";
    pub const TRY_KIND: &str = "E0243";
    pub const CAPTURING_CLOSURE: &str = "E0270";
    pub const FORGED: &str = "E0271";
    pub const PRIVATE_FIELD: &str = "E0272";
}

/// Options that change what is checked.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CheckOptions {
    /// Checking for a release build: `--release`.
    pub release: bool,
}

/// The result of checking a module.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CheckReport {
    /// Every diagnostic, in source order.
    pub diagnostics: Vec<Diagnostic>,
    /// One entry per function and `impl` method, in source order.
    pub functions: Vec<FunctionAuthority>,
}

/// One function's entry in the authority report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionAuthority {
    /// The function's name, or `Type.method` for a method.
    pub name: String,
    /// The built-in capabilities the function can reach, in order of first
    /// appearance. Attenuated capabilities keep their names, such as
    /// `FsRead`.
    pub authority: Vec<String>,
    /// The capability-bearing parameters, the receiver (`self`) included,
    /// and what each supplies.
    pub via: Vec<Via>,
    /// No parameter is capability-bearing.
    pub pure: bool,
}

/// A capability-bearing parameter and the capabilities it supplies. A
/// `cap fn` parameter is capability-bearing but names no capabilities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Via {
    pub param: String,
    pub capabilities: Vec<String>,
}

impl CheckReport {
    /// Render as `{"diagnostics": [...], "functions": [...]}`, with one
    /// diagnostic or function per line.
    pub fn to_json(&self) -> String {
        let diagnostics: Vec<String> = self.diagnostics.iter().map(Diagnostic::to_json).collect();
        let functions: Vec<String> = self
            .functions
            .iter()
            .map(FunctionAuthority::to_json)
            .collect();
        format!(
            "{{\n  \"diagnostics\": {},\n  \"functions\": {}\n}}\n",
            json_lines(&diagnostics),
            json_lines(&functions)
        )
    }
}

impl FunctionAuthority {
    /// Render as a single-line JSON object. Names are identifiers, so they
    /// need no escaping.
    pub fn to_json(&self) -> String {
        let via: Vec<String> = self
            .via
            .iter()
            .map(|v| {
                format!(
                    "{{\"param\":\"{}\",\"capabilities\":{}}}",
                    v.param,
                    json_strings(&v.capabilities)
                )
            })
            .collect();
        format!(
            "{{\"name\":\"{}\",\"authority\":{},\"via\":[{}],\"pure\":{}}}",
            self.name,
            json_strings(&self.authority),
            via.join(","),
            self.pure
        )
    }
}

fn json_strings(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|s| format!("\"{s}\"")).collect();
    format!("[{}]", quoted.join(","))
}

/// A JSON array with one element per line.
fn json_lines(items: &[String]) -> String {
    if items.is_empty() {
        return "[]".to_string();
    }
    format!("[\n    {}\n  ]", items.join(",\n    "))
}

/// Check `src` as a module, with default options.
pub fn check_module(src: &str) -> CheckReport {
    check_module_with(src, CheckOptions::default())
}

/// Check `src` as a module. A parse error is reported as the only
/// diagnostic, with no functions.
pub fn check_module_with(src: &str, options: CheckOptions) -> CheckReport {
    let module = match oska_syntax::parse_module(src) {
        Ok(module) => module,
        Err(err) => {
            return CheckReport {
                diagnostics: vec![err],
                functions: Vec::new(),
            }
        }
    };
    let mut diagnostics = Vec::new();
    let decls = decls::Decls::collect(&module, &mut diagnostics);
    resolve::module(&module, &decls, &mut diagnostics);
    declarations::module(src, &module, &decls, &mut diagnostics);
    recursion::module(&module, &mut diagnostics);
    if options.release {
        release::module(&module, &mut diagnostics);
    }
    let items = items::Items::collect(&module, &decls);
    body::module(&module, &decls, &items, &mut diagnostics);
    let functions = authority::report(&module, &decls);
    diagnostics.sort_by_key(|d| d.span.start);
    CheckReport {
        diagnostics,
        functions,
    }
}
