use crate::ast::visit::NodeRef;

use crate::ast::views::ItemView;

use crate::{
    ast::borrow::AstNode,
    kind::SyntaxKind,
    parser::{parse, parse_declarations},
};

use kagari_source::source::SourceFile;

#[test]
fn outer_attributes_preserve_nested_arguments_trivia_and_item_dispatch() {
    let text = r#"/// Tagged module.
#[tool::syntax("text with ) and ]", tags = [[1, true], sample::tag],)]
# /* between delimiters */ [meta]
pub mod model {
    #[meta] pub struct Item { #[meta(readable = true)] val value: i32 }
    #[meta] pub enum State { Ready }
    #[meta] pub trait Inspect {
        #[meta] type Output;
        #[meta] const DEFAULT: i32;
        #[meta] fn read(self) -> Self::Output;
    }
    #[meta] impl Inspect for Item {
        #[meta] type Output = i32;
        #[meta] const DEFAULT: i32 = 7;
        #[meta] fn read(self) -> i32 { self.value }
    }
    #[meta] pub(super) fn make() -> i32 { 42 }
}
"#;
    let source = SourceFile::new("attributes.kgr", text);
    let parsed = parse(&source);
    assert!(
        parsed.diagnostics().is_empty(),
        "{:?}",
        parsed.diagnostics()
    );
    assert_eq!(parsed.module().text().to_owned(), text);
    let module = parsed.module();
    let attributes = NodeRef::ModuleBody(module.body())
        .descendants()
        .filter_map(|node| match node {
            NodeRef::Attribute(node) => Some(node),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(attributes.len(), 14);
    assert_eq!(
        attributes[0].path().unwrap().text().unwrap(),
        "tool::syntax"
    );
    let arguments = attributes[0]
        .args()
        .unwrap()
        .arguments()
        .collect::<Vec<_>>();
    assert_eq!(arguments.len(), 2);
    let nested = arguments[1].value().unwrap().elements().collect::<Vec<_>>();
    assert_eq!(nested.len(), 2);
    assert_eq!(nested[0].value().unwrap().elements().count(), 2);
    for attribute in attributes {
        assert_eq!(
            NodeRef::from(attribute).first_token().unwrap().kind(),
            SyntaxKind::Hash
        );
        assert_eq!(
            NodeRef::from(attribute).last_token().unwrap().kind(),
            SyntaxKind::RBracket
        );
    }
    assert_eq!(
        parsed.module().items().next().unwrap().documentation(),
        "Tagged module."
    );
}

#[test]
fn invalid_attribute_forms_are_rejected_and_following_functions_survive() {
    for prefix in [
        "@meta",
        "@meta(name = 1)",
        "#meta",
        "#![meta]",
        "#[meta",
        "#[meta(1)] ]",
        "#[meta(1]",
        "#[meta(tags = [1, 2)]",
        "#[]",
    ] {
        let source = SourceFile::new(
            "invalid-attribute.kgr",
            format!("{prefix}\nfn healthy() -> i32 {{ 42 }}"),
        );
        let parsed = parse(&source);
        assert!(!parsed.diagnostics().is_empty(), "{prefix}");
        assert_eq!(parsed.module().text().to_owned(), source.text());
        assert!(parsed.module().items().any(|item| matches!(item, ItemView::FnDef(function) if function.name_text().as_deref() == Some("healthy"))), "{prefix}");
    }
    for prefix in ["@intrinsic(ArrayLen)", "#![intrinsic(ArrayLen)]"] {
        let source = SourceFile::new(
            "invalid-sdk.kgr",
            format!("{prefix}\npub fn len<T>(value: [T]) -> u64;"),
        );
        let parsed = parse_declarations(&source, Default::default(), &Default::default()).unwrap();
        assert!(!parsed.diagnostics().is_empty(), "{prefix}");
    }
}
