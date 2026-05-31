use kagari_common::DiagnosticKind;

use crate::{
    analyze_module_with_profile,
    profile::{LanguageFeatureProfile, validate_profile},
};

use super::common;

#[test]
fn profile_rejects_script_visible_reflection_when_disabled() {
    let module = common::parse_ok("fn main() -> String { type_of(7) }");
    let diagnostics = analyze_module_with_profile(&module, LanguageFeatureProfile::default())
        .expect_err("profile should reject reflection");

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::ProfileFeatureDisabled {
                feature: "reflection",
            }
    }));
}

#[test]
fn profile_requires_separate_reflection_write_feature() {
    let module = common::parse_ok(
        r#"
struct Point { var x: i32 }
fn main() -> Point {
    val point = Point { x: 1 };
    set_field(point, "x", 2)
}
"#,
    );
    let diagnostics = analyze_module_with_profile(
        &module,
        LanguageFeatureProfile {
            allow_reflection: true,
            allow_reflection_write: false,
            ..LanguageFeatureProfile::default()
        },
    )
    .expect_err("profile should reject reflective writes");

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::ProfileFeatureDisabled {
                feature: "reflective writes",
            }
    }));
}

#[test]
fn profile_rejects_interface_value_types_when_disabled() {
    let module = common::parse_ok(
        r#"
trait Show { fn show(self) -> String; }
fn render(value: Show) -> String { value.show() }
"#,
    );
    let analyzed = crate::analyze_module(&module).expect("interface value should analyze");
    let diagnostics = validate_profile(
        &analyzed,
        LanguageFeatureProfile {
            allow_interface_values: false,
            ..LanguageFeatureProfile::default()
        },
    )
    .expect_err("profile should reject interface values");

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::ProfileFeatureDisabled {
                feature: "interface values",
            }
    }));
}
