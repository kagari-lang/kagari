use crate::ast::views::ItemView;

use crate::{
    ast::{item::VisibilityKind, views::ExprView},
    tests::common,
};

#[test]
fn parses_external_and_inline_module_declarations() {
    let module = common::parse_ok(
        r#"
mod gameplay;
pub mod ui {
    pub const VERSION: i32 = 1;
}
"#,
    );
    let items = module.items().collect::<Vec<_>>();

    assert_eq!(items.len(), 2);

    match &items[0] {
        ItemView::ModuleDef(module_def) => {
            assert!(module_def.visibility() == VisibilityKind::Private);
            assert_eq!(module_def.name_text().as_deref(), Some("gameplay"));
            assert!(module_def.body().is_none());
        }
        other => panic!("unexpected first item: {other:?}"),
    }

    match &items[1] {
        ItemView::ModuleDef(module_def) => {
            assert!(module_def.visibility() == VisibilityKind::Public);
            assert_eq!(module_def.name_text().as_deref(), Some("ui"));
            let block = module_def.body().expect("expected inline module body");
            assert_eq!(block.items().count(), 1);
        }
        other => panic!("unexpected second item: {other:?}"),
    }
}

#[test]
fn parses_scoped_visibility_and_rejects_unimplemented_scopes() {
    for (scope, expected) in [
        ("super", VisibilityKind::PublicSuper),
        ("crate", VisibilityKind::PublicCrate),
    ] {
        let module = common::parse_ok(&format!(
            "pub({scope}) mod child {{ pub({scope}) fn read() -> i32 {{ 1 }} pub({scope}) struct Data {{ pub({scope}) val value: i32 }} }}",
        ));
        let child = match module.items().next().unwrap() {
            ItemView::ModuleDef(child) => child,
            other => panic!("{other:?}"),
        };
        assert_eq!(child.visibility(), expected);
        let body = child.body().unwrap();
        let items = body.items().collect::<Vec<_>>();
        assert!(matches!(&items[0], ItemView::FnDef(item) if item.visibility() == expected));
        assert!(
            matches!(&items[1], ItemView::StructDef(item) if item.visibility() == expected && item.field_list().unwrap().fields().next().unwrap().visibility() == expected)
        );
    }
    assert!(
        !common::parse("pub(in parent) fn invalid() {}")
            .diagnostics()
            .is_empty()
    );
}

#[test]
fn parses_use_declarations_and_import_trees() {
    let module = common::parse_ok(
        r#"
pub use crate::gameplay::{Player as Hero, inventory::*};
use {std::math, super::util as util};
"#,
    );
    let items = module.items().collect::<Vec<_>>();

    assert_eq!(items.len(), 2);

    match &items[0] {
        ItemView::UseDecl(use_decl) => {
            assert!(use_decl.visibility() == VisibilityKind::Public);
            let tree = use_decl.tree().expect("expected use tree");
            assert_eq!(
                tree.path().and_then(|path| path.text()).as_deref(),
                Some("crate::gameplay")
            );
            let nested = tree.nested_trees().collect::<Vec<_>>();
            assert_eq!(nested.len(), 2);
            assert_eq!(
                nested[0].path().and_then(|path| path.text()).as_deref(),
                Some("Player")
            );
            assert_eq!(
                nested[0].alias().and_then(|alias| alias.text()).as_deref(),
                Some("Hero")
            );
            assert_eq!(
                nested[1].path().and_then(|path| path.text()).as_deref(),
                Some("inventory")
            );
        }
        other => panic!("unexpected first item: {other:?}"),
    }

    match &items[1] {
        ItemView::UseDecl(use_decl) => {
            assert!(use_decl.visibility() == VisibilityKind::Private);
            let tree = use_decl.tree().expect("expected use tree");
            let nested = tree.nested_trees().collect::<Vec<_>>();
            assert_eq!(nested.len(), 2);
            assert_eq!(
                nested[0].path().and_then(|path| path.text()).as_deref(),
                Some("std::math")
            );
            assert_eq!(
                nested[1].path().and_then(|path| path.text()).as_deref(),
                Some("super::util")
            );
            assert_eq!(
                nested[1].alias().and_then(|alias| alias.text()).as_deref(),
                Some("util")
            );
        }
        other => panic!("unexpected second item: {other:?}"),
    }
}

#[test]
fn parses_module_paths_in_types_and_expressions() {
    let module = common::parse_ok(
        r#"
fn main() -> crate::math::Number {
    crate::math::zero
}
"#,
    );
    let function = common::first_function(&module);

    assert_eq!(
        function
            .return_type()
            .and_then(common::type_name)
            .as_deref(),
        Some("crate::math::Number")
    );

    let body = function.body().expect("expected function body");
    match body.tail_expr().expect("expected tail expression") {
        ExprView::PathExpr(path) => {
            assert_eq!(path.name_text().as_deref(), Some("crate::math::zero"));
        }
        other => panic!("unexpected tail expression: {other:?}"),
    }
}
