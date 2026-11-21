use crate::tests::standard::standard_modules;
use crate::{analyze_source, types::TypeId};
use kagari_source::{diagnostic::DiagnosticKind, source::SourceFile};
use kagari_types::scalar::BuiltinType;

const ORDINARY_TYPES: &str = include_str!("../../../../tests/fixtures/native/reference_names.kgr");

#[test]
fn user_reference_names_allow_ordinary_generic_storage_and_captures() {
    let result = analyze_source(
        &SourceFile::new("reference-names.kgr", ORDINARY_TYPES),
        standard_modules(),
    )
    .unwrap();
    assert!(
        result.diagnostics().is_empty(),
        "{:?}",
        result.diagnostics()
    );
    let functions = result.facts().typed.signatures().functions();
    let store = functions.iter().find(|item| item.name == "store").unwrap();
    let TypeId::Struct(outer) = &store.params[0].ty else {
        panic!("expected an ordinary struct, got {:?}", store.params[0].ty);
    };
    let [TypeId::Struct(inner)] = &outer.arguments[..] else {
        panic!("expected a nested struct, got {:?}", outer.arguments);
    };
    assert_ne!(outer.declaration, inner.declaration);
    assert_eq!(inner.arguments, [TypeId::Builtin(BuiltinType::I32)]);
    assert_eq!(store.return_type, store.params[0].ty);
    assert!(result.into_codegen().is_ok());
}

#[test]
fn user_reference_names_do_not_project_or_narrow() {
    for (body, matches_diagnostic) in [
        (
            "fn bad(value: Ref<Point>) -> i32 { value.x }",
            (|kind: &DiagnosticKind| matches!(kind, DiagnosticKind::UnknownName { name } if name == "x"))
                as fn(&DiagnosticKind) -> bool,
        ),
        (
            "fn bad(value: RefMut<Point>) -> i32 { value.read() }",
            |kind| matches!(kind, DiagnosticKind::UnknownName { name } if name == "read"),
        ),
        ("fn bad(value: RefMut<i32>) -> Ref<i32> { value }", |kind| {
            matches!(kind, DiagnosticKind::ReturnTypeMismatch { .. })
        }),
    ] {
        let result = analyze_source(
            &SourceFile::new(
                "reference-names-rejected.kgr",
                format!("{ORDINARY_TYPES}\n{body}"),
            ),
            standard_modules(),
        )
        .unwrap();
        assert!(
            result
                .diagnostics()
                .iter()
                .any(|d| matches_diagnostic(&d.kind)),
            "{body}: {:?}",
            result.diagnostics()
        );
        assert!(result.into_codegen().is_err());
    }
}
