use kagari_common::DiagnosticKind;

use crate::{resolver::resolve_names, tests::common, typeck::check_module};

struct SemanticCase {
    spec_section: &'static str,
    feature: &'static str,
    source: &'static str,
    expected: DiagnosticKind,
}

#[test]
fn semantic_spec_accepts_var_storage_mutation_without_rebinding_val_slots() {
    let lowered = common::lower_ok(
        r#"
struct Point {
    var x: i32,
}

fn main() -> i32 {
    var local = 1;
    local = 2;
    val point = Point { x: local };
    point.x = 3;
    val values = [point.x, 4];
    values[1] = 5;
    values[1]
}
"#,
    );
    let names = resolve_names(&lowered).expect("resolver should succeed");

    check_module(&lowered, &names).expect("spec-valid var field and index mutation should typeck");
}

#[test]
fn semantic_spec_rejects_non_var_rebinding_and_val_field_assignment() {
    let cases = [
        SemanticCase {
            spec_section: "docs/spec/syntax.md#binding-and-field-writeability",
            feature: "val local rebinding",
            source: "fn main() -> i32 { val x: i32 = 1; x = 2; x }",
            expected: DiagnosticKind::InvalidAssignmentTarget {
                reason: "`val` binding cannot be reassigned".to_string(),
            },
        },
        SemanticCase {
            spec_section: "docs/spec/syntax.md#ordinary-parameter-semantics",
            feature: "parameter rebinding",
            source: "fn main(value: i32) -> i32 { value = 1; value }",
            expected: DiagnosticKind::InvalidAssignmentTarget {
                reason: "function parameters are `val` bindings and cannot be reassigned"
                    .to_string(),
            },
        },
        SemanticCase {
            spec_section: "docs/spec/syntax.md#rebinding-rules",
            feature: "const item rebinding",
            source: r#"
const VERSION: i32 = 1;
fn main() -> i32 { VERSION = 2; 0 }
"#,
            expected: DiagnosticKind::InvalidAssignmentTarget {
                reason: "`const` item cannot be reassigned".to_string(),
            },
        },
        SemanticCase {
            spec_section: "docs/spec/syntax.md#binding-and-field-writeability",
            feature: "val field assignment",
            source: r#"
struct Point {
    val x: i32,
}

fn main() -> i32 {
    val point = Point { x: 1 };
    point.x = 3;
    point.x
}
"#,
            expected: DiagnosticKind::InvalidAssignmentTarget {
                reason: "`val` field `x` cannot be assigned".to_string(),
            },
        },
    ];

    for case in cases {
        let lowered = common::lower_ok(case.source);
        let names = resolve_names(&lowered).expect("resolver should succeed");
        let diagnostics =
            check_module(&lowered, &names).expect_err("type checker should reject source");

        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.kind == case.expected),
            "{} ({}) should reject with {:?}, got {:?}",
            case.spec_section,
            case.feature,
            case.expected,
            diagnostics
        );
    }
}
