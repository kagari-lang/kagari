use kagari_common::{DiagnosticKind, Severity};

use crate::tests::common;

#[test]
fn parses_struct_definition_with_fields() {
    let module = common::parse_ok("struct Player { val hp: i32, pub var name: string }");

    let struct_def = common::first_struct(&module);

    assert_eq!(struct_def.name_text().as_deref(), Some("Player"));

    let field_list = struct_def.field_list().expect("expected field list");
    let fields: Vec<_> = field_list.fields().collect();

    assert_eq!(fields.len(), 2);
    assert!(fields[0].is_val());
    assert_eq!(fields[0].name_text().as_deref(), Some("hp"));
    assert_eq!(
        fields[0].ty().and_then(|ty| ty.name_text()).as_deref(),
        Some("i32")
    );
    assert!(fields[1].is_var());
    assert_eq!(fields[1].name_text().as_deref(), Some("name"));
    assert_eq!(
        fields[1].ty().and_then(|ty| ty.name_text()).as_deref(),
        Some("string")
    );
}

#[test]
fn rejects_struct_field_without_val_or_var() {
    let parse = common::parse("struct Player { hp: i32 }");

    assert_eq!(parse.diagnostics().len(), 1);
    assert_eq!(parse.diagnostics()[0].severity, Severity::Error);
    assert_eq!(
        parse.diagnostics()[0].kind,
        DiagnosticKind::ExpectedFieldBinding
    );
}

#[test]
fn parses_enum_definition_with_variants() {
    let module = common::parse_ok("enum Color { Red, Green, Blue }");

    let enum_def = common::first_enum(&module);

    assert_eq!(enum_def.name_text().as_deref(), Some("Color"));

    let variant_list = enum_def.variant_list().expect("expected variant list");
    let variants: Vec<_> = variant_list.variants().collect();

    assert_eq!(variants.len(), 3);
    assert_eq!(variants[0].name_text().as_deref(), Some("Red"));
    assert_eq!(variants[1].name_text().as_deref(), Some("Green"));
    assert_eq!(variants[2].name_text().as_deref(), Some("Blue"));
}

#[test]
fn parses_public_const_item() {
    let module = common::parse_ok("pub const VERSION: i32 = 1;");
    let const_def = common::first_const(&module);

    assert!(const_def.is_pub());
    assert_eq!(const_def.name_text().as_deref(), Some("VERSION"));
    assert_eq!(
        const_def.ty().and_then(|ty| ty.name_text()).as_deref(),
        Some("i32")
    );
    assert!(const_def.initializer().is_some());
}

#[test]
fn rejects_script_static_item() {
    let parse = common::parse("pub static mut COUNTER: i32 = 0;");

    assert_eq!(
        parse.diagnostics()[0].severity,
        Severity::Error,
        "expected an error, got {:?}",
        parse.diagnostics()
    );
}
