use crate::{
    ast::{Expr, Item},
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
        Item::ModuleDef(module_def) => {
            assert!(!module_def.is_pub());
            assert_eq!(module_def.name_text().as_deref(), Some("gameplay"));
            assert!(module_def.block().is_none());
        }
        other => panic!("unexpected first item: {other:?}"),
    }

    match &items[1] {
        Item::ModuleDef(module_def) => {
            assert!(module_def.is_pub());
            assert_eq!(module_def.name_text().as_deref(), Some("ui"));
            let block = module_def.block().expect("expected inline module body");
            assert_eq!(block.items().count(), 1);
        }
        other => panic!("unexpected second item: {other:?}"),
    }
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
        Item::UseDecl(use_decl) => {
            assert!(use_decl.is_pub());
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
        Item::UseDecl(use_decl) => {
            assert!(!use_decl.is_pub());
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
            .and_then(|ty| ty.name_text())
            .as_deref(),
        Some("crate::math::Number")
    );

    let body = function.body().expect("expected function body");
    match body.tail_expr().expect("expected tail expression") {
        Expr::PathExpr(path) => {
            assert_eq!(path.name_text().as_deref(), Some("crate::math::zero"));
        }
        other => panic!("unexpected tail expression: {other:?}"),
    }
}
