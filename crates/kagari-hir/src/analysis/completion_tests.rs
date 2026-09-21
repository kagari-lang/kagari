use kagari_common::SourceFile;

#[test]
fn function_fallthrough_is_checked_only_when_reachable() {
    for (body, valid) in [
        ("return 42;", true),
        ("match true { _ => 42, _ => false }", true),
        ("match true { value => 42, _ => false }", true),
        (
            "match true { _ => if true { return 42; } else { return 7; }, _ => false };",
            true,
        ),
        ("match true { _ => 42, _ => missing }", false),
        ("return 42; false", true),
        ("if true { return 42; } else { return 7; };", true),
        ("if true { return 42; } else { 7 }", true),
        ("if true { 42 } else { return 7; }", true),
        (
            "match true { true => if true { return 42; } else { return 7; }, _ => 2 }",
            true,
        ),
        (
            "match true { _ => if true { return 42; } else { return 7; } };",
            true,
        ),
        ("loop { return 42; }", true),
        ("loop { continue; break; }", true),
        ("loop { loop { break; } }", true),
        ("loop { if true { break; }; }", false),
        ("loop { break; return 42; }", false),
        ("while true { return 42; }", false),
        ("if true { return 42; };", false),
        ("return false;", false),
        ("return;", false),
        ("return 42; val invalid: bool = 1;", false),
        ("42;", false),
    ] {
        let source = SourceFile::new("completion.kgr", format!("fn main() -> i32 {{ {body} }}"));
        let analysis = crate::analyze_source(&source, Default::default());
        assert_eq!(
            analysis.diagnostics().is_empty(),
            valid,
            "{body}: {:?}",
            analysis.diagnostics()
        );
        assert_eq!(analysis.into_codegen().is_ok(), valid, "{body}");
    }
}
