//! Privacy below the item (spec #115, #119): a field without `pub`, a
//! method without `pub(pkg)` or `pub`, and a trait that is private to its
//! module cannot be used from another module of the package, even when no
//! name of theirs is written in an import. Each is reported as a private
//! item is (`E0206`), with a fix in the declaring file.

mod common;

use common::{assert_modules_errors, assert_modules_fix_in, assert_modules_ok};

/// A module with a `pub(pkg)` struct whose `secret` field is private and
/// whose `shown` field is `pub`.
const TOKEN: &str = "pub(pkg) struct Token {\n    secret: Int,\n    pub shown: Int,\n}\n\n\
                     pub(pkg) fn make() -> Token {\n    Token(secret: 1, shown: 2)\n}\n";

#[test]
fn a_private_field_cannot_be_read_from_another_module() {
    let main = "use pkg.auth\n\nfn main() {}\n\n\
                fn f() -> Int {\n    auth.make().secret\n}\n";
    assert_modules_errors(
        &[("main", main), ("auth", TOKEN)],
        &[("src/main.osk", "E0206", "secret")],
    );
}

#[test]
fn the_fix_marks_the_field_pub_in_its_own_file() {
    let main = "use pkg.auth\n\nfn main() {}\n\n\
                fn f() -> Int {\n    auth.make().secret\n}\n";
    assert_modules_fix_in(
        &[("main", main), ("auth", TOKEN)],
        "E0206",
        "src/auth.osk",
        &TOKEN.replace("    secret: Int", "    pub secret: Int"),
    );
}

#[test]
fn a_pub_field_and_the_declaring_module_read_fields_freely() {
    let main = "use pkg.auth\n\nfn main() {}\n\n\
                fn f() -> Int {\n    auth.make().shown\n}\n";
    let auth = format!("{TOKEN}\npub(pkg) fn peek(t: Token) -> Int {{\n    t.secret\n}}\n");
    assert_modules_ok(&[("main", main), ("auth", &auth)]);
}

#[test]
fn a_private_field_cannot_be_assigned_from_another_module() {
    let main = "use pkg.auth\n\nfn main() {}\n\n\
                fn f() {\n    var t = auth.make()\n    t.secret = 5\n}\n";
    assert_modules_errors(
        &[("main", main), ("auth", TOKEN)],
        &[("src/main.osk", "E0206", "secret")],
    );
}

#[test]
fn a_struct_with_a_private_field_cannot_be_constructed_from_another_module() {
    let main = "use pkg.auth.{Token}\n\nfn main() {}\n\n\
                fn f() -> Token {\n    Token(secret: 9, shown: 2)\n}\n";
    assert_modules_errors(
        &[("main", main), ("auth", TOKEN)],
        &[("src/main.osk", "E0206", "Token")],
    );
}

#[test]
fn a_private_field_cannot_be_matched_from_another_module() {
    let main = "use pkg.auth.{Token}\n\nfn main() {}\n\n\
                fn f(t: Token) -> Int {\n    match t {\n        Token(secret: s, shown: _) => s\n    }\n}\n";
    assert_modules_errors(
        &[("main", main), ("auth", TOKEN)],
        &[("src/main.osk", "E0206", "Token(secret: s, shown: _)")],
    );
}

#[test]
fn a_private_positional_field_cannot_be_matched_or_built_from_another_module() {
    let id = "pub(pkg) struct Id(Int)\n\npub(pkg) fn new_id() -> Id {\n    Id(7)\n}\n";
    let main = "use pkg.ids.{Id, new_id}\n\nfn main() {}\n\n\
                fn f() -> Int {\n    match new_id() {\n        Id(n) => n\n    }\n}\n\n\
                fn g() -> Id {\n    Id(9)\n}\n";
    assert_modules_errors(
        &[("main", main), ("ids", id)],
        &[
            ("src/main.osk", "E0206", "Id(n)"),
            ("src/main.osk", "E0206", "Id"),
        ],
    );
}

/// A module with a `pub(pkg)` struct, a private method and a `pub(pkg)`
/// one.
const COUNTER: &str = "pub(pkg) struct Counter {\n    pub n: Int\n}\n\n\
                       impl Counter {\n    fn bump(self) -> Int {\n        self.n + 1\n    }\n\n    \
                       pub(pkg) fn get(self) -> Int {\n        self.bump()\n    }\n\n    \
                       fn zero() -> Counter {\n        Counter(n: 0)\n    }\n}\n";

#[test]
fn a_private_method_cannot_be_called_from_another_module() {
    let main = "use pkg.count.{Counter}\n\nfn main() {}\n\n\
                fn f(c: Counter) -> Int {\n    c.bump() + c.get()\n}\n";
    assert_modules_errors(
        &[("main", main), ("count", COUNTER)],
        &[("src/main.osk", "E0206", "bump")],
    );
}

#[test]
fn a_private_associated_function_cannot_be_called_from_another_module() {
    let main = "use pkg.count.{Counter}\n\nfn main() {}\n\n\
                fn f() -> Counter {\n    Counter.zero()\n}\n";
    assert_modules_errors(
        &[("main", main), ("count", COUNTER)],
        &[("src/main.osk", "E0206", "zero")],
    );
}

#[test]
fn a_private_method_cannot_be_called_qualified_from_another_module() {
    let main = "use pkg.count.{Counter}\n\nfn main() {}\n\n\
                fn f(c: Counter) -> Int {\n    Counter.bump(c)\n}\n";
    assert_modules_errors(
        &[("main", main), ("count", COUNTER)],
        &[("src/main.osk", "E0206", "bump")],
    );
}

#[test]
fn the_fix_marks_the_method_pub_pkg_in_its_own_file() {
    let main = "use pkg.count.{Counter}\n\nfn main() {}\n\n\
                fn f(c: Counter) -> Int {\n    c.bump()\n}\n";
    assert_modules_fix_in(
        &[("main", main), ("count", COUNTER)],
        "E0206",
        "src/count.osk",
        &COUNTER.replace("    fn bump", "    pub(pkg) fn bump"),
    );
}

#[test]
fn a_private_method_of_an_impl_in_a_third_module_is_private_to_that_module() {
    let types = "pub(pkg) struct Point {\n    pub x: Int\n}\n";
    let ext = "use pkg.types.{Point}\n\nimpl Point {\n    fn hidden(self) -> Int {\n        self.x\n    }\n}\n";
    let main = "use pkg.types.{Point}\n\nfn main() {}\n\n\
                fn f(p: Point) -> Int {\n    p.hidden()\n}\n";
    assert_modules_errors(
        &[("main", main), ("types", types), ("ext", ext)],
        &[("src/main.osk", "E0206", "hidden")],
    );
}

/// A module with a private trait that a `pub(pkg)` type implements.
const SECRET_TRAIT: &str = "trait Unlock {\n    fn unlock(self) -> Int\n}\n\n\
                            pub(pkg) struct Vault {\n    pub n: Int\n}\n\n\
                            impl Unlock for Vault {\n    fn unlock(self) -> Int {\n        self.n\n    }\n}\n";

#[test]
fn a_private_traits_method_cannot_be_called_from_another_module() {
    let main = "use pkg.vault.{Vault}\n\nfn main() {}\n\n\
                fn f(v: Vault) -> Int {\n    v.unlock()\n}\n";
    assert_modules_errors(
        &[("main", main), ("vault", SECRET_TRAIT)],
        &[("src/main.osk", "E0206", "unlock")],
    );
}

#[test]
fn the_fix_marks_the_private_trait_pub_pkg() {
    let main = "use pkg.vault.{Vault}\n\nfn main() {}\n\n\
                fn f(v: Vault) -> Int {\n    v.unlock()\n}\n";
    assert_modules_fix_in(
        &[("main", main), ("vault", SECRET_TRAIT)],
        "E0206",
        "src/vault.osk",
        &SECRET_TRAIT.replace("trait Unlock", "pub(pkg) trait Unlock"),
    );
}

#[test]
fn a_private_traits_method_cannot_be_called_qualified_by_the_type() {
    let main = "use pkg.vault.{Vault}\n\nfn main() {}\n\n\
                fn f(v: Vault) -> Int {\n    Vault.unlock(v)\n}\n";
    assert_modules_errors(
        &[("main", main), ("vault", SECRET_TRAIT)],
        &[("src/main.osk", "E0206", "unlock")],
    );
}

#[test]
fn a_pub_pkg_traits_method_is_callable_from_another_module() {
    let main = "use pkg.vault.{Vault}\n\nfn main() {}\n\n\
                fn f(v: Vault) -> Int {\n    v.unlock()\n}\n";
    let vault = SECRET_TRAIT.replace("trait Unlock", "pub(pkg) trait Unlock");
    assert_modules_ok(&[("main", main), ("vault", &vault)]);
}
