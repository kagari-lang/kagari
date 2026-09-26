use crate::{
    ast::{AstNode, Item},
    parser::{ParseLimits, parse_declarations},
};
use kagari_common::{SourceFile, cancellation::CancellationToken};

#[test]
fn declaration_mode_preserves_signatures_docs_and_source() {
    let text = "/// Read one element.\r\n/// Returns None outside the array.\r\n#[intrinsic(array_get)]\r\npub fn get<T>(value:[T], index:usize)->Option<T>;\r\n\r\n/// The next operation.\r\npub fn len<T>(value:[T])->usize;";
    let source = SourceFile::new("kagari://std/array.kgr", text);
    let parsed = parse_declarations(
        &source,
        ParseLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    assert!(
        parsed.diagnostics().is_empty(),
        "{:?}",
        parsed.diagnostics()
    );
    assert_eq!(parsed.syntax().syntax().text().to_string(), text);
    let items = parsed.syntax().items().collect::<Vec<_>>();
    let Item::FnDef(get) = &items[0] else {
        panic!()
    };
    assert!(get.body().is_none());
    assert_eq!(
        get.documentation(text),
        "Read one element.\nReturns None outside the array."
    );
    assert_eq!(items[1].documentation(text), "The next operation.");
    assert!(!crate::parse(&source).diagnostics().is_empty());
}

#[test]
fn declaration_parsing_observes_cancellation() {
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(
        parse_declarations(
            &SourceFile::new("decl.kgr", "pub fn get();"),
            Default::default(),
            &cancel
        )
        .is_err()
    );
}

#[test]
fn opaque_native_types_are_restricted_to_declaration_mode() {
    let source = SourceFile::new(
        "native.kgr",
        "/// A native array.\n#[builtin_type(Array)] pub type Array<T>;",
    );
    let parsed =
        parse_declarations(&source, Default::default(), &CancellationToken::default()).unwrap();
    assert!(
        parsed.diagnostics().is_empty(),
        "{:?}",
        parsed.diagnostics()
    );
    assert_eq!(parsed.syntax().syntax().text().to_string(), source.text());
    assert!(
        parsed
            .syntax()
            .syntax()
            .children()
            .any(|node| crate::ast::AssociatedType::cast(node).is_some())
    );
    assert!(!crate::parse(&source).diagnostics().is_empty());
}
