use crate::tests::common;

#[test]
fn parses_struct_definition_with_fields() {
    let module = common::parse_ok("struct Player { hp: i32, name: string }");

    let struct_def = common::first_struct(&module);

    assert_eq!(struct_def.name_text().as_deref(), Some("Player"));

    let field_list = struct_def.field_list().expect("expected field list");
    let fields: Vec<_> = field_list.fields().collect();

    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].name_text().as_deref(), Some("hp"));
    assert_eq!(
        fields[0].ty().and_then(|ty| ty.name_text()).as_deref(),
        Some("i32")
    );
    assert_eq!(fields[1].name_text().as_deref(), Some("name"));
    assert_eq!(
        fields[1].ty().and_then(|ty| ty.name_text()).as_deref(),
        Some("string")
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
