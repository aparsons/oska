//! The E0261 fix for a missing trait method that has its own generic
//! parameters (#46, spec #184): the stub copies the generics and their
//! bounds, with the trait's arguments substituted and a generic renamed
//! when it would shadow one of the impl's.

mod common;

use common::assert_fix;

#[test]
fn a_missing_generic_trait_method_fix_copies_the_generics_and_bounds() {
    assert_fix(
        "trait Shower {\n    fn show<T: Display + Eq>(self, item: T, other: T) -> String\n}\n\
         struct Foo { x: Int }\n\
         impl Shower for Foo {\n}",
        "E0261",
        "trait Shower {\n    fn show<T: Display + Eq>(self, item: T, other: T) -> String\n}\n\
         struct Foo { x: Int }\n\
         impl Shower for Foo {\n    fn show<T: Display + Eq>(self, item: T, other: T) -> String {\n        todo()\n    }\n}",
    );
}

#[test]
fn a_missing_generic_trait_method_fix_copies_unbounded_and_several_generics() {
    assert_fix(
        "trait Mapper {\n    fn pair<A, B: Display>(self, a: A, b: B) -> A\n}\n\
         struct Foo { x: Int }\n\
         impl Mapper for Foo {}",
        "E0261",
        "trait Mapper {\n    fn pair<A, B: Display>(self, a: A, b: B) -> A\n}\n\
         struct Foo { x: Int }\n\
         impl Mapper for Foo {\n    fn pair<A, B: Display>(self, a: A, b: B) -> A {\n        todo()\n    }\n}",
    );
}

#[test]
fn a_missing_generic_trait_method_fix_substitutes_the_trait_s_arguments_and_avoids_the_impl_s_generics(
) {
    assert_fix(
        "trait Boxer<U> {\n    fn put<T: From<U>>(self, item: T, u: U) -> List<T>\n}\n\
         struct Foo { x: Int }\n\
         impl Boxer<Int> for Foo {\n}",
        "E0261",
        "trait Boxer<U> {\n    fn put<T: From<U>>(self, item: T, u: U) -> List<T>\n}\n\
         struct Foo { x: Int }\n\
         impl Boxer<Int> for Foo {\n    fn put<T: From<Int>>(self, item: T, u: Int) -> List<T> {\n        todo()\n    }\n}",
    );
    assert_fix(
        "trait Mapper {\n    fn pair<T: Display>(self, t: T) -> T\n}\n\
         struct Wrap<T> { inner: T }\n\
         impl<T> Mapper for Wrap<T> {\n}",
        "E0261",
        "trait Mapper {\n    fn pair<T: Display>(self, t: T) -> T\n}\n\
         struct Wrap<T> { inner: T }\n\
         impl<T> Mapper for Wrap<T> {\n    fn pair<T2: Display>(self, t: T2) -> T2 {\n        todo()\n    }\n}",
    );
}

#[test]
fn applying_the_generic_stub_fix_introduces_no_new_error() {
    for src in [
        "trait Shower {\n    fn show<T: Display + Eq>(self, item: T, other: T) -> String\n}\n\
         struct Foo { x: Int }\n\
         impl Shower for Foo {\n}",
        // The trait's generics and `Self` are replaced in the bounds and types.
        "trait Boxer<U> {\n    fn put<T: From<U>>(self, item: T, u: U) -> List<T>\n}\n\
         struct Foo { x: Int }\n\
         impl Boxer<Int> for Foo {\n}",
        // The method's generic keeps clear of the impl's own.
        "trait Mapper {\n    fn pair<T: Display>(self, t: T) -> T\n}\n\
         struct Wrap<T> { inner: T }\n\
         impl<T> Mapper for Wrap<T> {\n}",
        // Two missing methods, one generic.
        "trait Both {\n    fn plain(self) -> Int\n    fn show<T: Display>(self, item: T) -> String\n}\n\
         struct Foo { x: Int }\n\
         impl Both for Foo {}",
    ] {
        let fixed = common::apply_fix(src, "E0261");
        let after = common::check(&fixed);
        let errors: Vec<_> = after
            .diagnostics
            .iter()
            .map(|d| (d.code, d.message.clone()))
            .collect();
        assert!(
            errors.is_empty(),
            "the fix for {src:?} gave {fixed:?}, which reports {errors:?}"
        );
    }
}
