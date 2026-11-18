//! The wording of system errors in `std.rt` (spec #154, tickets #167
//! and #171): each platform table's part of `system_errors` is
//! `oska-sys`'s table for its operating system, worded by its kinds, for
//! every number. It reads only `std.rt`'s source, so it runs on every
//! host, for every platform.

/// The constant `field` of the `std.rt` platform table the function
/// `table` gives.
fn table_field(table: &str, field: &str) -> usize {
    let source = oska_check::STD_RT_SOURCE;
    let body = &source[source
        .find(&format!("fn {table}() -> Platform {{"))
        .expect("the table")..];
    let head = format!("        {field}: ");
    let start = body.find(&head).expect("the field") + head.len();
    let end = start + body[start..].find(',').expect("its end");
    body[start..end].parse().expect("a number")
}

#[test]
fn each_platform_s_errno_table_is_oskas_wording_for_every_number() {
    let source = oska_check::STD_RT_SOURCE;
    let head = "fn system_errors() -> String {\n    \"";
    let start = source.find(head).expect("the wordings") + head.len();
    let end = start + source[start..].find('"').expect("their end");
    let all: Vec<&str> = source[start..end].split('|').collect();
    let tables = [
        ("linux_aarch64", oska_sys::LINUX_ERRORS, 133),
        ("linux_x86_64", oska_sys::LINUX_ERRORS, 133),
        // `ELAST`, macOS's largest error number.
        ("macos", oska_sys::MACOS_ERRORS, 106),
    ];
    // Linux's 133, then macOS's 106, and nothing else.
    assert_eq!(all.len(), 133 + 106);
    for (table, errors, length) in tables {
        let at = table_field(table, "errors_at");
        assert_eq!(table_field(table, "errors_count"), length, "{table}");
        let wordings = &all[at..at + length];
        for (index, wording) in wordings.iter().enumerate() {
            let errno = index as i32 + 1;
            let oska = oska_sys::system_wording(errors, errno);
            let expected = if wording.is_empty() {
                oska_sys::OTHER_WORDING
            } else {
                wording
            };
            assert_eq!(oska, expected, "{table}: errno {errno}");
            assert!(!wording.contains("os error"), "{table}: errno {errno}");
        }
        for (code, _) in errors {
            assert!(
                (1..=length).contains(&(*code as usize)),
                "{table}: {code} is outside its part"
            );
        }
    }
}
