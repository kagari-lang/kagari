use crate::{ast::views::ItemView, tests::common};

#[test]
fn keeps_partial_syntax_tree_when_parse_has_errors() {
    let context = "tests/fixtures/parser/keeps_partial_syntax_tree_when_parse_has_errors.kgr (logical source: test.kg)";
    let parse = common::parse(include_str!(
        "../../../tests/fixtures/parser/keeps_partial_syntax_tree_when_parse_has_errors.kgr"
    ));

    let module = parse.module();

    let Some(ItemView::FnDef(function)) = module.items().next() else {
        panic!("{context}: expected a function item");
    };

    assert_eq!(function.name_text().as_deref(), Some("add"), "{context}");

    assert_eq!(
        function
            .return_type()
            .and_then(common::type_name)
            .as_deref(),
        Some("int"),
        "{context}"
    );

    assert!(function.body().is_some(), "{context}");

    assert_eq!(module.items().count(), 1, "{context}");

    assert_eq!(parse.diagnostics().len(), 1, "{context}");
}
