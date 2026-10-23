use crate::{
    ast::{
        item::{AssociatedType, Item},
        traits::AstNode,
    },
    parser::{ParseLimits, parse, parse_declarations},
};

use {kagari_common::cancellation::CancellationToken, kagari_source::source::SourceFile};

#[test]
fn declaration_mode_preserves_signatures_docs_and_source() {
    let text = "/// Read one element.\r\n/// Returns None outside the array.\r\n#[intrinsic(array_get)]\r\npub fn get<T>(value:[T], index:u64)->Option<T>;\r\n\r\n/// The next operation.\r\npub fn len<T>(value:[T])->u64;";
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
    assert!(!parse(&source).diagnostics().is_empty());
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
fn declaration_mode_distinguishes_aliases_from_opaque_types() {
    let text = "#[builtin_type(Array)] pub type List<T>; pub type Items<T> = List<T>; mod nested { type Handle; type Count = i32; }";
    let source = SourceFile::new("native.kgr", text);
    let parsed =
        parse_declarations(&source, Default::default(), &CancellationToken::default()).unwrap();
    assert!(
        parsed.diagnostics().is_empty(),
        "{:?}",
        parsed.diagnostics()
    );
    assert_eq!(parsed.syntax().syntax().text().to_string(), text);
    let items = parsed.syntax().items().collect::<Vec<_>>();
    assert!(matches!(items[0], Item::TypeDecl(_)));
    assert!(matches!(items[1], Item::TypeAliasDef(_)));
    let Item::ModuleDef(module) = &items[2] else {
        panic!("expected module")
    };
    let nested = module.block().unwrap().items().collect::<Vec<_>>();
    assert!(matches!(nested[0], Item::TypeDecl(_)));
    assert!(matches!(nested[1], Item::TypeAliasDef(_)));
}

#[test]
fn declaration_mode_retains_native_alias_attributes_for_validation() {
    let text = "#[builtin_type(Array)] pub type List<T> = Vec<T>;";
    let source = SourceFile::new("native.kgr", text);
    let parsed =
        parse_declarations(&source, Default::default(), &CancellationToken::default()).unwrap();
    assert!(
        parsed.diagnostics().is_empty(),
        "{:?}",
        parsed.diagnostics()
    );
    let item = parsed.syntax().items().next().unwrap();
    assert!(matches!(item, Item::TypeAliasDef(_)));
    // Native interface validation must reject this attribute on an alias rather
    // than accepting it as an opaque declaration with a discarded definition.
    assert_eq!(
        item.attributes().next().unwrap().name_text().as_deref(),
        Some("builtin_type")
    );
}

#[test]
fn opaque_native_types_are_restricted_to_declaration_mode() {
    let source = SourceFile::new(
        "native.kgr",
        "/// A native array.\n#[builtin_type(Array)] pub type List<T>;",
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
            .any(|node| AssociatedType::cast(node).is_some())
    );
    assert!(!parse(&source).diagnostics().is_empty());
}
