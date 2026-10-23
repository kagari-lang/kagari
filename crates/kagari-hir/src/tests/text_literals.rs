use crate::{
    analyze_source,
    hir::expr::{ExprKind, literal::LiteralKind},
    tests::common,
    typeck::scalar::ScalarValue,
};
use kagari_source::{diagnostic::DiagnosticKind, source::SourceFile};
use kagari_stdlib::catalog;

#[test]
fn string_literals_lower_decoded_content_without_synthetic_quoting() {
    for (literal, expected) in [
        (r#""\u{7B}name\u{7D}""#, "{name}"),
        (r#""{{name}}""#, "{name}"),
        (r#""\\n\"""#, "\\n\""),
        (r##"r#"\n"{name}"#"##, "\\n\"{name}"),
        ("\"a\r\n  b\\\r\n \tc\"", "a\n  bc"),
    ] {
        let source = SourceFile::new(
            "literal.kgr",
            format!("fn main() -> String {{ {literal} }}"),
        );
        let analysis = analyze_source(&source, catalog::shared().unwrap()).unwrap();
        assert!(
            analysis.diagnostics().is_empty(),
            "{literal}: {:?}",
            analysis.diagnostics()
        );
        let facts = analysis.facts();
        let module = &facts.lowered.module;
        let expr = module
            .block(module.functions[0].body.unwrap())
            .tail_expr
            .unwrap();
        let ExprKind::Literal(value) = &module.expr(expr).kind else {
            panic!("no-hole literal");
        };
        assert_eq!(value.kind, LiteralKind::String);
        assert_eq!(value.text, expected);
        assert_eq!(
            facts.typed.type_table.scalar_value(expr),
            Some(&ScalarValue::String(expected.into()))
        );
    }
}

#[test]
fn interpolation_text_and_occurrences_keep_decoded_values_and_physical_spans() {
    let text =
        "fn render(name: i32) -> String { \"\\u{7B}{{\\\"\\\\n\r\n{name}\\\r\n \t{name:?}}}\" }";
    let lowered = common::lower_ok(text);
    let module = &lowered.module;
    let expr = module
        .block(module.functions[0].body.unwrap())
        .tail_expr
        .unwrap();
    let ExprKind::InterpolatedString(parts) = &module.expr(expr).kind else {
        panic!("interpolation");
    };
    let mut names = Vec::new();
    let mut strings = Vec::new();
    for part in parts {
        match &module.expr(*part).kind {
            ExprKind::Literal(value) => {
                assert_eq!(value.kind, LiteralKind::String);
                strings.push(value.text.as_str());
            }
            ExprKind::FormatPart { expr, debug } => {
                let ExprKind::Name { name, .. } = &module.expr(*expr).kind else {
                    panic!("name hole");
                };
                assert_eq!(name, "name");
                let span = lowered.source_map.expr_span(*expr);
                assert_eq!(&text[span.start..span.end], "name");
                names.push((*expr, *debug));
            }
            other => panic!("unexpected part: {other:?}"),
        }
    }
    assert_eq!(strings, ["{{\"\\n\n", "", "}"]);
    assert_eq!(names.len(), 2);
    assert_ne!(names[0].0, names[1].0);
    assert!(!names[0].1);
    assert!(names[1].1);
}

#[test]
fn interpolation_checks_name_resolution_and_each_format_protocol() {
    let source = SourceFile::new(
        "names.kgr",
        r#"
        const N: i32 = 1;
        static val STATE: i32 = 2;
        fn render(parameter: i32) -> String {
            val local = 3;
            "{parameter}{local:?}{N}{STATE}{parameter:?}"
        }
    "#,
    );
    let analysis = analyze_source(&source, catalog::shared().unwrap()).unwrap();
    assert!(
        analysis.diagnostics().is_empty(),
        "{:?}",
        analysis.diagnostics()
    );
    for (hole, protocol) in [("{value}", "Display"), ("{value:?}", "Debug")] {
        let source = SourceFile::new(
            "protocol.kgr",
            format!("fn render<T>(value: T) -> String {{ \"{hole}\" }}"),
        );
        let analysis = analyze_source(&source, catalog::shared().unwrap()).unwrap();
        assert!(analysis.diagnostics().iter().any(|d| matches!(&d.kind, DiagnosticKind::InvalidCallTarget { type_name } if type_name.contains(&format!("interpolation requires {protocol}")))), "{:?}", analysis.diagnostics());
    }
    let source = SourceFile::new("missing.kgr", r#"fn render() -> String { "{missing}" }"#);
    let analysis = analyze_source(&source, catalog::shared().unwrap()).unwrap();
    assert!(!analysis.diagnostics().is_empty());
}

#[test]
fn string_literals_keep_const_and_pattern_policy_without_runtime_holes() {
    let source = SourceFile::new(
        "constant.kgr",
        r##"
        const SAME: bool = r#"{name}\n"# == "{{name}}\\n";
        fn read(text: String) -> i32 {
            match text { "{{name}}\\n" => 1, r#"other"# => 2, _ => 0 }
        }
    "##,
    );
    let analysis = analyze_source(&source, catalog::shared().unwrap()).unwrap();
    assert!(
        analysis.diagnostics().is_empty(),
        "{:?}",
        analysis.diagnostics()
    );
    assert_eq!(
        analysis.facts().typed.const_values.values().next(),
        Some(&ScalarValue::Bool(true))
    );

    let source = SourceFile::new(
        "constant-hole.kgr",
        r#"const N: i32 = 1; const TEXT: bool = "{N}" == "1";"#,
    );
    let analysis = analyze_source(&source, catalog::shared().unwrap()).unwrap();
    assert!(analysis.diagnostics().iter().any(|d| matches!(&d.kind, DiagnosticKind::InvalidConstInitializer { const_name, reason } if const_name == "TEXT" && reason == "unsupported const initializer expression")), "{:?}", analysis.diagnostics());
}
