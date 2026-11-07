use crate::ast::visit::NodeRef;

use crate::ast::views::ItemView;

use crate::{
    ast::{borrow::AstNode, item::VisibilityKind},
    kind::SyntaxKind,
    tests::common,
};
use kagari_source::diagnostic::DiagnosticKind;

#[test]
fn parses_module_type_aliases_with_lossless_metadata() {
    let text = "/// Items in order.\r\n#[tag] pub type Items<T: Read + Write> = Vec<T> where T: Clone,;\r\nmod nested { type Count = i32; pub(super) type Shared = super::Items<i32>; pub(crate) type Pair = (i32, bool); }";
    let module = common::parse_ok(text);
    assert_eq!(module.text().to_owned(), text);
    let items = module.items().collect::<Vec<_>>();
    let ItemView::TypeAlias(alias) = &items[0] else {
        panic!("expected alias")
    };
    assert_eq!(NodeRef::from(*alias).kind(), SyntaxKind::TypeAliasDef);
    assert_eq!(alias.name_text().as_deref(), Some("Items"));
    assert_eq!(alias.visibility(), VisibilityKind::Public);
    assert_eq!(items[0].documentation(), "Items in order.");
    assert_eq!(
        items[0].attributes().next().unwrap().name_text().as_deref(),
        Some("tag")
    );
    let params = alias.generic_params().unwrap().params().collect::<Vec<_>>();
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].name_text().as_deref(), Some("T"));
    assert_eq!(
        params[0].bounds().unwrap().source_text().to_owned().trim(),
        "Read + Write"
    );
    assert_eq!(
        alias.ty().unwrap().source_text().to_owned().trim(),
        "Vec<T>"
    );
    assert_eq!(alias.where_clause().unwrap().predicates().count(), 1);

    let ItemView::ModuleDef(nested) = &items[1] else {
        panic!("expected module")
    };
    let nested = nested.body().unwrap();
    let nested_items = nested.items().collect::<Vec<_>>();
    for (item, visibility) in nested_items.iter().zip([
        VisibilityKind::Private,
        VisibilityKind::PublicSuper,
        VisibilityKind::PublicCrate,
    ]) {
        let ItemView::TypeAlias(alias) = item else {
            panic!("expected nested alias")
        };
        assert_eq!(alias.visibility(), visibility);
        assert!(alias.ty().is_some());
    }
    assert_eq!(nested_items.len(), 3);
}

#[test]
fn type_aliases_reuse_where_and_target_type_syntax() {
    for text in [
        "type Items<T> where T: Read, = Vec<T>;",
        "type Projection<T> = <T as Read>::Output where T: Read;",
        "type Callback = fn([i32], (bool, i32)) -> Option<Vec<i32>>;",
    ] {
        let module = common::parse_ok(text);
        assert_eq!(module.text().to_owned(), text);
        let ItemView::TypeAlias(alias) = module.items().next().unwrap() else {
            panic!("expected alias")
        };
        assert!(alias.ty().is_some());
    }
}

#[test]
fn type_alias_recovery_retains_following_items() {
    for (text, diagnostic) in [
        (
            "type Missing; fn next() {}",
            DiagnosticKind::ExpectedAssignmentOperator,
        ),
        (
            "type Missing = ; fn next() {}",
            DiagnosticKind::ExpectedType,
        ),
        (
            "type Missing = i32 fn next() {}",
            DiagnosticKind::ExpectedStatementTerminator,
        ),
        (
            "type Wrong: Read = i32; fn next() {}",
            DiagnosticKind::UnexpectedToken,
        ),
    ] {
        let parsed = common::parse(text);
        assert!(
            parsed
                .diagnostics()
                .iter()
                .any(|error| error.kind == diagnostic),
            "{text}: {:?}",
            parsed.diagnostics()
        );
        assert_eq!(parsed.module().text().to_owned(), text);
        let module = parsed.module();
        let items = module.items().collect::<Vec<_>>();
        assert_eq!(items.len(), 2, "{text}");
        assert!(matches!(items[0], ItemView::TypeAlias(_)));
        assert!(matches!(items[1], ItemView::FnDef(_)));
    }
}

#[test]
fn type_aliases_do_not_replace_associated_types() {
    let module = common::parse_ok(
        "type Count = i32; trait Read { type Output; } impl Read for Counter { type Output = Count; }",
    );
    let items = module.items().collect::<Vec<_>>();
    assert!(matches!(items[0], ItemView::TypeAlias(_)));
    let ItemView::TraitDef(trait_def) = &items[1] else {
        panic!("expected trait")
    };
    let ItemView::ImplBlock(impl_def) = &items[2] else {
        panic!("expected impl")
    };
    let declaration = trait_def.associated_types().next().unwrap();
    let definition = impl_def.associated_types().next().unwrap();
    assert_eq!(
        NodeRef::from(declaration).kind(),
        SyntaxKind::AssociatedType
    );
    assert!(declaration.ty().is_none());
    assert_eq!(NodeRef::from(definition).kind(), SyntaxKind::AssociatedType);
    assert_eq!(
        common::type_name(definition.ty().unwrap()).as_deref(),
        Some("Count")
    );
}

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
        fields[0].ty().and_then(common::type_name).as_deref(),
        Some("i32")
    );
    assert!(fields[1].is_var());
    assert_eq!(fields[1].name_text().as_deref(), Some("name"));
    assert_eq!(
        fields[1].ty().and_then(common::type_name).as_deref(),
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

#[test]
fn parses_public_const_item() {
    let module = common::parse_ok("pub const VERSION: i32 = 1;");
    let const_def = common::first_const(&module);

    assert!(const_def.visibility() == VisibilityKind::Public);
    assert_eq!(const_def.name_text().as_deref(), Some("VERSION"));
    assert_eq!(
        const_def.ty().and_then(common::type_name).as_deref(),
        Some("i32")
    );
    assert!(const_def.initializer().is_some());
}
