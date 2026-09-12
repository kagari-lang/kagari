use kagari_common::DiagnosticKind;

use crate::{
    analyze_source,
    profile::{LanguageFeatureProfile, validate_profile},
};

#[test]
fn profile_rejects_script_visible_reflection_when_disabled() {
    let module =
        kagari_common::SourceFile::new("profile.kgr", "fn main() -> String { type_of(7) }");
    let diagnostics = analyze_source(&module, LanguageFeatureProfile::default())
        .into_codegen()
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
    let module = kagari_common::SourceFile::new(
        "profile.kgr",
        r#"
struct Point { var x: i32 }
fn main() -> Point {
    val point = Point { x: 1 };
    set_field(point, "x", 2)
}
"#,
    );
    let diagnostics = analyze_source(
        &module,
        LanguageFeatureProfile {
            allow_reflection: true,
            allow_reflection_write: false,
            ..LanguageFeatureProfile::default()
        },
    )
    .into_codegen()
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
    let module = kagari_common::SourceFile::new(
        "profile.kgr",
        r#"
trait Show { fn show(self) -> String; }
fn render(value: Show) -> String { value.show() }
"#,
    );
    let analyzed = crate::analyze_source(&module, Default::default())
        .into_codegen()
        .expect("interface value should analyze");
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
