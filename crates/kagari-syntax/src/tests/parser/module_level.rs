use kagari_common::DiagnosticKind;

use crate::tests::common;

#[test]
fn rejects_executable_top_level_code() {
    for source in [
        "val boot = 1;",
        "var boot = 1;",
        "1 + 2",
        "while true {}",
        "return;",
    ] {
        let parse = common::parse(source);
        assert!(
            parse
                .diagnostics()
                .iter()
                .any(|diagnostic| { diagnostic.kind == DiagnosticKind::ExpectedTopLevelItem }),
            "{source}: {:?}",
            parse.diagnostics()
        );
    }
}

#[test]
fn accepts_declarations_without_an_implicit_entry() {
    let module = common::parse_ok("const N: i32 = 1; fn main() -> i32 { N }");
    assert_eq!(module.items().count(), 2);
}
