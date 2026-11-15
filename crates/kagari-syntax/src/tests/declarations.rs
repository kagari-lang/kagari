use crate::{
    ast::{
        borrow::AstNode,
        item::FunctionBody,
        misc::{ParameterType, Writeability},
        site::Required,
        views::ItemView,
        visit::NodeRef,
    },
    kind::SyntaxKind,
    parser::{ParseOptions, parse},
    tests::common,
};
use kagari_common::cancellation::CancellationToken;
use kagari_source::{diagnostic::DiagnosticKind, source::SourceFile};

#[test]
fn native_signatures_preserve_docs_source_and_grammar_tokens() {
    let fixture = include_str!("../../tests/fixtures/declarations/signatures.kgr");
    for text in [fixture.to_owned(), fixture.replace('\n', "\r\n")] {
        let source = SourceFile::new("kagari://std/array.kgr", &text);
        let parsed = parse(&source, ParseOptions::default(), &Default::default()).unwrap();
        assert!(
            parsed.diagnostics().is_empty(),
            "{:?}",
            parsed.diagnostics()
        );
        assert_eq!(parsed.module().text(), text);
        let module = parsed.module();
        let items = module.items().collect::<Vec<_>>();
        let ItemView::FnDef(get) = items[0] else {
            panic!("expected Native function")
        };
        assert!(get.is_native());
        assert!(get.body().is_none());
        assert!(matches!(
            get.data().body,
            FunctionBody::Declaration(Required::Present(_))
        ));
        assert_eq!(
            get.documentation(),
            "Read one element.\nReturns None outside the array."
        );
        assert_eq!(items[1].documentation(), "The next operation.");
        let root = NodeRef::ModuleBody(module.body());
        assert_eq!(
            root.tokens()
                .filter(|token| token.kind() == SyntaxKind::NativeKw)
                .count(),
            2
        );
        assert_eq!(
            common::grammar_text(root),
            parsed
                .tokens()
                .filter(|token| !token.lexical_kind().is_trivia())
                .map(|token| token.text())
                .collect::<String>()
        );
    }
}

#[test]
fn native_declarations_mix_with_complete_script_definitions() {
    let text = include_str!("../../tests/fixtures/declarations/mixed.kgr");
    let module = common::parse_ok(text);
    let items = module.items().collect::<Vec<_>>();
    let ItemView::NativeTypeDef(list) = items[0] else {
        panic!("expected Native type")
    };
    assert_eq!(NodeRef::from(list).kind(), SyntaxKind::NativeTypeDef);
    assert!(matches!(list.data().native_keyword, Required::Present(_)));
    assert!(list.field_list().is_none());
    assert!(matches!(list.data().terminator, Some(Required::Present(_))));
    assert_eq!(list.documentation(), "A native array.");
    assert!(matches!(items[1], ItemView::TypeAlias(_)));
    let ItemView::NativeTypeDef(counter) = items[2] else {
        panic!("expected Native type with fields")
    };
    assert!(counter.data().terminator.is_none());
    assert!(counter.bounds().is_some());
    assert!(counter.where_clause().is_some());
    let fields = counter.field_list().unwrap().fields().collect::<Vec<_>>();
    assert_eq!(
        fields
            .iter()
            .map(|field| (field.name_text().unwrap(), field.writeability()))
            .collect::<Vec<_>>(),
        [
            ("value".into(), Some(Writeability::Val)),
            ("count".into(), Some(Writeability::Var))
        ]
    );
    assert_eq!(
        fields[0]
            .attributes()
            .next()
            .unwrap()
            .name_text()
            .as_deref(),
        Some("tag")
    );
    assert!(matches!(items[3], ItemView::StructDef(_)));
    let ItemView::FnDef(identity) = items[4] else {
        panic!("expected Script function")
    };
    assert!(!identity.is_native());
    assert!(identity.body().is_some());
    let ItemView::TraitDef(reader) = items[5] else {
        panic!("expected Script trait")
    };
    let methods = reader.methods().collect::<Vec<_>>();
    assert!(!methods[0].is_native());
    assert!(methods[0].body().is_none());
    assert!(!methods[1].is_native());
    assert!(methods[1].body().is_some());
    let ItemView::ImplBlock(inherent) = items[6] else {
        panic!("expected inherent impl")
    };
    assert!(inherent.trait_ref().is_none());
    let methods = inherent.methods().collect::<Vec<_>>();
    assert!(
        methods
            .iter()
            .all(|method| method.is_native() && method.body().is_none())
    );
    assert_eq!(
        methods[0]
            .param_list()
            .params()
            .next()
            .unwrap()
            .name_text()
            .as_deref(),
        Some("value")
    );
    let receiver = methods[1].param_list().params().next().unwrap();
    assert_eq!(receiver.name_text().as_deref(), Some("self"));
    assert!(matches!(
        receiver.data().annotation,
        ParameterType::Receiver
    ));
    let parameters = methods[1].param_list().params().collect::<Vec<_>>();
    assert_eq!(parameters[1].ty().unwrap().source_text(), "Ref<T>");
    assert_eq!(parameters[2].ty().unwrap().source_text(), "RefMut<T>");
    let ItemView::ImplBlock(implementation) = items[7] else {
        panic!("expected trait impl")
    };
    assert!(implementation.trait_ref().is_some());
    assert_eq!(
        implementation
            .associated_types()
            .next()
            .unwrap()
            .ty()
            .unwrap()
            .source_text(),
        "T"
    );
    assert!(implementation.methods().next().unwrap().is_native());
    let ItemView::FnDef(borrow) = items[8] else {
        panic!("expected generic Native function")
    };
    assert_eq!(
        borrow.generic_params().unwrap().source_text(),
        "<T: Read<Item = i32>>"
    );
    assert_eq!(borrow.return_type().unwrap().source_text(), "RefMut<T>");
    let ItemView::ModuleDef(nested) = items[9] else {
        panic!("expected nested module")
    };
    let nested = nested.body().unwrap();
    assert!(matches!(
        nested.items().next().unwrap(),
        ItemView::NativeTypeDef(_)
    ));
}

#[test]
fn native_parsing_observes_cancellation() {
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(
        parse(
            &SourceFile::new(
                "decl.kgr",
                include_str!("../../tests/fixtures/declarations/mixed.kgr")
            ),
            ParseOptions::default(),
            &cancel
        )
        .is_err()
    );
}

#[test]
fn native_syntax_preserves_body_and_alias_requirements_and_recovery() {
    for (text, kind) in [
        (
            "fn missing(); fn healthy() {}",
            DiagnosticKind::ExpectedFunctionBodyStart,
        ),
        (
            "type Missing; fn healthy() {}",
            DiagnosticKind::ExpectedAssignmentOperator,
        ),
        (
            "native fn body() {} fn healthy() {}",
            DiagnosticKind::UnexpectedToken,
        ),
        (
            "native fn missing() fn healthy() {}",
            DiagnosticKind::ExpectedStatementTerminator,
        ),
        (
            "native type Missing fn healthy() {}",
            DiagnosticKind::ExpectedStatementTerminator,
        ),
        (
            "native type Broken { pub var field: } fn healthy() {}",
            DiagnosticKind::ExpectedType,
        ),
        (
            "impl T { native fn body(self) {} } fn healthy() {}",
            DiagnosticKind::UnexpectedToken,
        ),
        (
            "trait T { pub type Item; } fn healthy() {}",
            DiagnosticKind::UnexpectedToken,
        ),
    ] {
        let parsed = common::parse(text);
        assert!(
            parsed
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.kind == kind),
            "{text}: {:?}",
            parsed.diagnostics()
        );
        assert!(parsed.module().items().any(|item| matches!(item, ItemView::FnDef(function) if function.name_text().as_deref() == Some("healthy"))), "{text}");
        let module = parsed.module();
        assert_eq!(
            common::grammar_text(NodeRef::ModuleBody(module.body())),
            parsed
                .tokens()
                .filter(|token| !token.lexical_kind().is_trivia())
                .map(|token| token.text())
                .collect::<String>(),
            "{text}"
        );
    }
}

#[test]
fn native_alias_attributes_remain_available_for_semantic_validation() {
    let module = common::parse_ok("#[builtin_type(Array)] pub type List<T> = Vec<T>;");
    let item = module.items().next().unwrap();
    assert!(matches!(item, ItemView::TypeAlias(_)));
    assert_eq!(
        item.attributes().next().unwrap().name_text().as_deref(),
        Some("builtin_type")
    );
}
