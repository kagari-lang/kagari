use crate::{
    hir::{
        attribute::{
            AssociatedOwner, AttributeArgument, AttributeForm, AttributePayload, AttributeTarget,
            AttributeValue,
        },
        expr::ExprKind,
        ids::{BodyOwner, HirOwner},
        item::{
            Item,
            function::{FunctionImplementation, FunctionKind},
        },
        origin::NodeOrigin,
        pattern::PatternKind,
        place::PlaceKind,
        stmt::StmtKind,
        ty::TypeKind,
        writeability::Writeability,
    },
    tests::common,
};
use kagari_common::identity::{
    map::DefinitionContext, mapping::DefinitionMappingError, metadata::DefinitionMetadata,
    table::DefinitionTableError,
};
use kagari_source::{diagnostic::DiagnosticKind, source::SourceFile};
use kagari_syntax::{
    ast::views::ItemView,
    parser::{self, ParseLimits, ParseOptions},
};
use kagari_types::visibility::Visibility;
use std::sync::Arc;

#[test]
fn source_lowering_rejects_foreign_ids_in_record_origins() {
    let lowered = common::lower_ok("fn f() -> i32 { 1 }");
    assert!(lowered.source_surface().is_some());
    let owner = DefinitionContext::new().unwrap();
    owner.intern_root(lowered.source.module_identity()).unwrap();
    let foreign = DefinitionContext::new()
        .unwrap()
        .intern_root(lowered.source.module_identity())
        .unwrap();
    let definitions = owner.snapshot();
    let cancel = Default::default();
    DefinitionMetadata::checked(definitions.clone(), lowered.clone(), &cancel).unwrap();

    let origin = NodeOrigin::Structured {
        declaration: foreign,
    };
    let mut wrong_function = lowered.clone();
    wrong_function.module.functions[0].origin = origin;

    let mut wrong_expression = lowered;
    let function = &wrong_expression.module.functions[0];
    let block = function.implementation.script_body().unwrap();
    let tail = wrong_expression.module.block(block).tail_expr.unwrap();
    wrong_expression.module.nodes.exprs[tail.index()].1.origin = origin;

    for (site, corrupted) in [
        ("function", wrong_function),
        ("expression", wrong_expression),
    ] {
        assert_eq!(
            DefinitionMetadata::checked(definitions.clone(), corrupted, &cancel).unwrap_err(),
            DefinitionMappingError::Identity(DefinitionTableError::ForeignTable),
            "{site} origin"
        );
    }
}

#[test]
fn declaration_only_functions_do_not_acquire_script_bodies() {
    let source = Arc::new(SourceFile::new(
        "declarations.kgr",
        include_str!("../../tests/fixtures/lower/declaration_only_functions.kgr"),
    ));
    let cancel = Default::default();
    let parsed = parser::parse(&source, ParseOptions::default(), &cancel).unwrap();
    assert!(parsed.diagnostics().is_empty());
    let lowered = crate::lower::lower_module_controlled(source, &parsed.module(), &cancel);
    let [native, required, script] = lowered.module.functions.as_slice() else {
        panic!("three declarations expected");
    };
    // Explicit Native syntax selects a declaration kind, not installation authority.
    assert!(matches!(
        native.implementation,
        FunctionImplementation::Native(_)
    ));
    assert_eq!(required.implementation, FunctionImplementation::Required);
    assert!(matches!(
        script.implementation,
        FunctionImplementation::Script(_)
    ));
    for function in [native, required, script] {
        for (index, parameter) in function.params.iter().enumerate() {
            assert_eq!(parameter.id.index(), index);
            assert_eq!(
                parameter.id.owner(),
                HirOwner::Body(BodyOwner::Function(function.id))
            );
            assert!(std::ptr::eq(
                lowered.module.parameter(parameter.id),
                parameter
            ));
            let span = lowered.module.parameter(parameter.id).binding_span();
            assert_eq!(&lowered.source.text()[span.start..span.end], parameter.name);
        }
    }
    assert_eq!(native.params[0].name, "value");
    assert_eq!(required.params[0].name, "self");
    assert_eq!(required.params[1].name, "value");
    assert!(
        matches!(&lowered.module.type_ref(required.params[0].ty).kind, TypeKind::Named(name) if name == "Self")
    );
    assert!(
        matches!(&lowered.module.type_ref(native.params[0].ty).kind, TypeKind::Named(name) if name == "i32")
    );
    let script_body = lowered
        .module
        .block(script.implementation.script_body().unwrap());
    assert!(script_body.statements.is_empty());
    assert_eq!(script_body.tail_expr, None);
    let methods = &lowered.module.traits[0].methods;
    assert!(!methods[0].has_default);
    assert!(methods[1].has_default);
}

#[test]
fn attributes_preserve_structured_metadata_and_typed_targets() {
    let source = include_str!("../../tests/fixtures/lower/attributes.kgr");
    let lowered = common::lower_ok(source);
    let [bare, empty_args, attribute, empty_list, rest @ ..] = lowered.module.attributes.as_slice()
    else {
        panic!("attribute facts expected");
    };
    let module = &lowered.module;
    let contract = &module.traits[0];
    let implementation = &module.impls[0];
    let trait_owner = AssociatedOwner::Trait(contract.id);
    let impl_owner = AssociatedOwner::Impl(implementation.id);
    let function = AttributeTarget::Item(Item::Function(module.functions[0].id));
    assert_eq!(
        module
            .attributes
            .iter()
            .map(|fact| fact.target)
            .collect::<Vec<_>>(),
        [
            function,
            function,
            function,
            AttributeTarget::Field(module.structs[0].fields[0].id),
            AttributeTarget::Use(module.uses[0].id),
            AttributeTarget::Item(Item::Trait(contract.id)),
            AttributeTarget::AssociatedConst {
                owner: trait_owner,
                index: 0
            },
            AttributeTarget::AssociatedType {
                owner: trait_owner,
                index: 0
            },
            AttributeTarget::Item(Item::Function(contract.methods[0].function)),
            AttributeTarget::AssociatedConst {
                owner: trait_owner,
                index: 1
            },
            AttributeTarget::Item(Item::Impl(implementation.id)),
            AttributeTarget::AssociatedType {
                owner: impl_owner,
                index: 0
            },
            AttributeTarget::AssociatedConst {
                owner: impl_owner,
                index: 0
            },
            AttributeTarget::Item(Item::Function(implementation.methods[0].function)),
        ]
    );
    assert!(
        module
            .attributes
            .windows(2)
            .all(|pair| pair[0].span.start < pair[1].span.start)
    );
    assert_eq!(contract.associated_consts[0].name, "REQUIRED");
    assert_eq!(contract.associated_consts[0].initializer, None);
    assert_eq!(contract.associated_types[0].name, "Output");
    assert_eq!(contract.associated_types[0].ty, None);
    let default = module.constant(contract.associated_consts[1].initializer.unwrap());
    assert_eq!(default.name, "DEFAULT");
    assert_eq!(module.consts.len(), 2);
    assert!(
        !module
            .attributes
            .iter()
            .any(|fact| matches!(fact.target, AttributeTarget::Item(Item::Const(_))))
    );
    assert_eq!(module.uses.len(), 1);
    assert_eq!(module.uses[0].imports, 0..2);
    assert_eq!(module.imports[0].segments, ["sample", "first"]);
    assert_eq!(module.imports[1].local_name(), Some("other"));
    for (fact, expected) in [
        (&rest[0], "#[meta]\nuse sample::{ first, second as other };"),
        (&rest[2], "#[meta]\n    const REQUIRED: i32;"),
        (&rest[3], "#[meta]\n    type Output;"),
        (&rest[4], "#[meta]\n    fn method(self);"),
    ] {
        let range = lowered
            .module
            .attribute_target_origin(fact.target)
            .diagnostic_span();
        assert_eq!(&source[range.start..range.end], expected);
    }
    assert_eq!(bare.form, AttributeForm::Bare);
    assert_eq!(empty_args.form, AttributeForm::Arguments(Vec::new()));
    assert_eq!(
        empty_list.form,
        AttributeForm::Value(AttributeValue::List(Vec::new()))
    );
    for fact in &lowered.module.attributes {
        assert_eq!(fact.name, "meta");
    }
    assert_eq!(bare.target, attribute.target);
    assert_eq!(empty_args.target, attribute.target);
    let field_span = lowered
        .module
        .attribute_target_origin(empty_list.target)
        .diagnostic_span();
    assert_eq!(
        &source[field_span.start..field_span.end],
        "#[meta =[]]\n    val value: i32"
    );
    assert_eq!(
        &source[attribute.span.start..attribute.span.end],
        "#[meta(name = \"answer\", tags = [1, true, sample:: /* qualifier */ tag])]"
    );
    let declaration = lowered
        .module
        .attribute_target_origin(attribute.target)
        .diagnostic_span();
    assert_eq!(
        &source[declaration.start..declaration.end],
        source.split_once("\n\n").unwrap().0
    );
    assert_eq!(
        attribute.form,
        AttributeForm::Arguments(vec![
            AttributeArgument {
                name: Some("name".into()),
                value: AttributeValue::Literal("\"answer\"".into()),
            },
            AttributeArgument {
                name: Some("tags".into()),
                value: AttributeValue::List(vec![
                    AttributeArgument {
                        name: None,
                        value: AttributeValue::Literal("1".into()),
                    },
                    AttributeArgument {
                        name: None,
                        value: AttributeValue::Literal("true".into()),
                    },
                    AttributeArgument {
                        name: None,
                        value: AttributeValue::Path("sample:: /* qualifier */ tag".into()),
                    },
                ]),
            },
        ])
    );

    let source = Arc::new(SourceFile::new(
        "attributes-recovery.kgr",
        include_str!("../../tests/fixtures/lower/attributes_recovery.kgr"),
    ));
    let cancel = Default::default();
    let parsed = parser::parse(&source, ParseOptions::default(), &cancel).unwrap();
    assert!(!parsed.diagnostics().is_empty());
    let lowered = crate::lower::lower_module_controlled(source.clone(), &parsed.module(), &cancel);
    let [
        missing,
        missing_arg,
        recovered_path,
        malformed_args,
        missing_list_element,
        unclosed_args,
        unclosed_value,
        unclosed_bare,
        malformed_value,
    ] = lowered.module.attributes.as_slice()
    else {
        panic!(
            "nine retained attribute facts expected: {:?}",
            lowered.module.attributes
        );
    };
    assert_eq!(missing.form, AttributeForm::Value(AttributeValue::Missing));
    assert_eq!(
        missing_arg.form,
        AttributeForm::Arguments(vec![AttributeArgument {
            name: Some("key".into()),
            value: AttributeValue::Missing,
        }])
    );
    // The parser retains `sample` and leaves `::` outside that path node.
    assert_eq!(
        recovered_path.form,
        AttributeForm::Malformed {
            recovered: Some(AttributePayload::Value(AttributeValue::Path(
                "sample".into()
            ))),
        }
    );
    assert_eq!(
        malformed_value.form,
        AttributeForm::Value(AttributeValue::Malformed)
    );
    assert_eq!(
        malformed_args.form,
        AttributeForm::Malformed {
            recovered: Some(AttributePayload::Arguments(vec![
                AttributeArgument {
                    name: None,
                    value: AttributeValue::Literal("1".into()),
                },
                AttributeArgument {
                    name: None,
                    value: AttributeValue::Literal("2".into()),
                },
            ])),
        }
    );
    assert_eq!(
        missing_list_element.form,
        AttributeForm::Arguments(vec![AttributeArgument {
            name: None,
            value: AttributeValue::List(vec![AttributeArgument {
                name: None,
                value: AttributeValue::Missing,
            }]),
        }])
    );
    let one = AttributeArgument {
        name: None,
        value: AttributeValue::Literal("1".into()),
    };
    assert_eq!(
        unclosed_args.form,
        AttributeForm::Malformed {
            recovered: Some(AttributePayload::Arguments(vec![one.clone()])),
        }
    );
    assert_eq!(
        unclosed_value.form,
        AttributeForm::Malformed {
            recovered: Some(AttributePayload::Value(AttributeValue::List(vec![one]))),
        }
    );
    assert_eq!(
        unclosed_bare.form,
        AttributeForm::Malformed {
            recovered: Some(AttributePayload::Bare),
        }
    );
    assert_eq!(
        &source.text()[missing.span.start..missing.span.end],
        "#[meta = ]"
    );
    assert_eq!(
        &source.text()[unclosed_args.span.start..unclosed_args.span.end],
        "#[meta(1]"
    );
    assert_eq!(lowered.module.structs[0].fields.len(), 4);
    for (fact, field) in [
        unclosed_args,
        unclosed_value,
        unclosed_bare,
        malformed_value,
    ]
    .into_iter()
    .zip(&lowered.module.structs[0].fields)
    {
        assert_eq!(fact.target, AttributeTarget::Field(field.id));
    }
    // Exhaustion after the use keyword retains an authored declaration with no tree/leaves.
    let source = Arc::new(SourceFile::new(
        "use-recovery.kgr",
        include_str!("../../tests/fixtures/lower/attributes_use.kgr"),
    ));
    let parsed = parser::parse(
        &source,
        ParseOptions {
            limits: ParseLimits {
                max_nesting: 0,
                ..Default::default()
            },
        },
        &cancel,
    )
    .unwrap();
    assert!(
        parsed.diagnostics().iter().any(|diagnostic| matches!(
            diagnostic.kind,
            DiagnosticKind::CompileLimitExceeded { .. }
        ))
    );
    let syntax = parsed.module();
    let ItemView::UseDecl(declaration) = syntax.items().next().unwrap() else {
        panic!("recovered use expected")
    };
    assert!(declaration.tree().is_none());
    let lowered = crate::lower::lower_module_controlled(source.clone(), &syntax, &cancel);
    let [recovered_use] = lowered.module.attributes.as_slice() else {
        panic!("one recovered use attribute expected")
    };
    assert_eq!(recovered_use.form, AttributeForm::Bare);
    assert_eq!(lowered.module.uses.len(), 1);
    let recovered = &lowered.module.uses[0];
    assert_eq!(recovered_use.target, AttributeTarget::Use(recovered.id));
    assert_eq!(recovered.imports, 0..0);
    assert!(lowered.module.imports.is_empty());
    let range = lowered
        .module
        .attribute_target_origin(recovered_use.target)
        .diagnostic_span();
    assert_eq!(&source.text()[range.start..range.end], "#[meta]\nuse");
}

#[test]
fn lowers_items_into_hir_module() {
    let lowered = common::lower_ok(include_str!("../../tests/fixtures/lower/items.kgr"));

    assert_eq!(lowered.module.items.len(), 3);
    assert!(matches!(lowered.module.items[0], Item::Struct(_)));
    assert!(matches!(lowered.module.items[1], Item::Enum(_)));
    assert!(matches!(lowered.module.items[2], Item::Function(_)));

    assert_eq!(lowered.module.structs[0].name, "Player");
    assert_eq!(lowered.module.structs[0].fields.len(), 2);
    assert_eq!(
        lowered.module.structs[0].fields[0].writeability,
        Writeability::Val
    );
    assert_eq!(
        lowered.module.structs[0].fields[1].writeability,
        Writeability::Var
    );
    assert_eq!(lowered.module.enums[0].name, "Color");
    assert_eq!(lowered.module.functions[0].name, "main");
}

#[test]
fn lowers_module_trait_and_impl_namespace_items() {
    let lowered = common::lower_ok(include_str!(
        "../../tests/fixtures/lower/module_trait_and_impl_namespace.kgr"
    ));

    assert_eq!(lowered.module.modules.len(), 1);
    assert_eq!(lowered.module.modules[0].name, "gameplay");
    assert!(!lowered.module.modules[0].inline);

    assert_eq!(lowered.module.traits.len(), 1);
    assert_eq!(lowered.module.traits[0].name, "Display");
    assert_eq!(lowered.module.traits[0].methods.len(), 1);
    let trait_method = &lowered.module.traits[0].methods[0];
    assert_eq!(trait_method.name, "show");
    let trait_function = lowered
        .module
        .functions
        .iter()
        .find(|function| function.id == trait_method.function)
        .expect("expected lowered trait method function");
    assert!(matches!(trait_function.kind, FunctionKind::TraitMethod));
    assert_eq!(trait_function.params[0].name, "self");

    assert_eq!(lowered.module.impls.len(), 1);
    let reference = lowered.module.impls[0]
        .trait_ref
        .as_ref()
        .expect("impl trait reference");
    assert!(
        matches!(&lowered.module.type_ref(reference.ty).kind, TypeKind::Named(name) if name == "Display")
    );
    let for_type = lowered.module.impls[0]
        .for_type
        .expect("expected impl target type");
    assert!(matches!(
        lowered.module.type_ref(for_type).kind,
        TypeKind::Named(ref name) if name == "Player"
    ));
    assert_eq!(lowered.module.impls[0].methods.len(), 1);
    let impl_method = &lowered.module.impls[0].methods[0];
    assert_eq!(impl_method.name, "show");
    let impl_function = lowered
        .module
        .functions
        .iter()
        .find(|function| function.id == impl_method.function)
        .expect("expected lowered impl method function");
    assert!(matches!(impl_function.kind, FunctionKind::ImplMethod));
    assert_eq!(impl_function.params[0].name, "self");

    assert!(matches!(lowered.module.items[0], Item::Module(_)));
    assert!(matches!(lowered.module.items[1], Item::Trait(_)));
    assert!(matches!(lowered.module.items[2], Item::Struct(_)));
    assert!(matches!(lowered.module.items[3], Item::Impl(_)));
}

#[test]
fn lowers_function_body_expressions_and_statements() {
    let lowered = common::lower_ok(
        "fn main(value: i32) -> i32 { val next: i32 = value + 1; match next { _ => next } }",
    );
    let function = &lowered.module.functions[0];

    assert_eq!(function.params.len(), 1);
    assert!(matches!(
        &lowered.module.type_ref(function.params[0].ty).kind,
        TypeKind::Named(name) if name == "i32"
    ));

    let block = lowered
        .module
        .block(function.implementation.script_body().unwrap());
    let stmt = lowered.module.stmt(block.statements[0]);
    match &stmt.kind {
        StmtKind::Binding {
            pattern,
            writeability,
            ty,
            initializer,
        } => {
            assert_eq!(*writeability, Writeability::Val);
            assert!(matches!(
                &lowered.module.pattern(*pattern).kind,
                PatternKind::Name { local } if lowered.module.nodes.local(*local).name == "next"
            ));
            assert!(matches!(
                ty.map(|ty| &lowered.module.type_ref(ty).kind),
                Some(TypeKind::Named(name)) if name == "i32"
            ));
            assert!(matches!(
                &lowered.module.expr(*initializer).kind,
                ExprKind::Binary { .. }
            ));
        }
        other => panic!("unexpected stmt kind: {other:?}"),
    }

    let tail_expr = block.tail_expr.expect("expected tail expr");
    match &lowered.module.expr(tail_expr).kind {
        ExprKind::Match { arms, .. } => {
            assert_eq!(arms.len(), 1);
            assert!(matches!(
                lowered.module.pattern(arms[0].pattern).kind,
                PatternKind::Wildcard
            ));
        }
        other => panic!("unexpected tail expr: {other:?}"),
    }

    let assign = common::lower_ok("fn main(value: i32) -> i32 { value = 1; value }");
    let assign_block = assign.module.block(
        assign.module.functions[0]
            .implementation
            .script_body()
            .unwrap(),
    );
    match &assign.module.stmt(assign_block.statements[0]).kind {
        StmtKind::Assign { target, .. } => {
            assert!(matches!(
                assign.module.place(*target).kind,
                PlaceKind::Name(ref name) if name == "value"
            ));
        }
        other => panic!("unexpected assign stmt kind: {other:?}"),
    }

    let nested = common::lower_ok(include_str!(
        "../../tests/fixtures/lower/nested_field_assignment.kgr"
    ));
    let block = nested.module.block(
        nested.module.functions[0]
            .implementation
            .script_body()
            .unwrap(),
    );
    match &nested.module.stmt(block.statements[1]).kind {
        StmtKind::Assign { target, .. } => {
            assert!(matches!(
                &nested.module.place(*target).kind,
                PlaceKind::Field { name, .. } if name == "x"
            ));
        }
        other => panic!("unexpected nested assign stmt kind: {other:?}"),
    }
}

#[test]
fn lowers_var_binding() {
    let lowered = common::lower_ok("fn main() { var value = 1; value = 2; }");
    let function = &lowered.module.functions[0];
    let block = lowered
        .module
        .block(function.implementation.script_body().unwrap());
    let stmt = lowered.module.stmt(block.statements[0]);

    match &stmt.kind {
        StmtKind::Binding {
            writeability,
            pattern,
            ..
        } => {
            assert_eq!(*writeability, Writeability::Var);
            assert!(matches!(
                &lowered.module.pattern(*pattern).kind,
                PatternKind::Name { local } if lowered.module.nodes.local(*local).name == "value"
            ));
        }
        other => panic!("unexpected stmt kind: {other:?}"),
    }
}

#[test]
fn lowers_public_const_items() {
    let lowered = common::lower_ok(
        r#"
pub const VERSION: i32 = 1;
"#,
    );

    assert_eq!(lowered.module.items.len(), 1);

    let const_item = &lowered.module.consts[0];
    assert_eq!(const_item.name, "VERSION");
    let full = const_item.origin.source_span().unwrap();
    let name = const_item.name_span.unwrap();
    assert_eq!(
        &lowered.source.text()[full.start..full.end],
        "pub const VERSION: i32 = 1;"
    );
    assert_eq!(&lowered.source.text()[name.start..name.end], "VERSION");
    assert!(matches!(const_item.visibility, Visibility::Public));
    assert!(matches!(
        lowered.module.expr(const_item.initializer).kind,
        ExprKind::Literal(_)
    ));
}

#[test]
fn inline_ast_lowering_owns_only_its_items_and_attributes() {
    let source = Arc::new(SourceFile::new(
        "inline.kgr",
        "// π\r\n#[meta] mod a { #[meta] fn f() {} mod b { #[meta] fn g() {} } }",
    ));
    let parsed = parser::parse(&source, ParseOptions::default(), &Default::default())
        .expect("fresh cancellation token");
    let root = crate::lower::lower_module_controlled(
        source.clone(),
        &parsed.module(),
        &Default::default(),
    );
    let module = parsed.module();
    let ItemView::ModuleDef(child) = module.items().next().unwrap() else {
        panic!()
    };
    let view = child.body().unwrap();
    let lowered = crate::lower::lower_module_controlled(source.clone(), &view, &Default::default());
    assert!(root.module.functions.is_empty());
    let [root_attribute] = root.module.attributes.as_slice() else {
        panic!("root attribute expected")
    };
    assert_eq!(
        root_attribute.target,
        AttributeTarget::Item(Item::Module(root.module.modules[0].id))
    );
    let [child_attribute] = lowered.module.attributes.as_slice() else {
        panic!("child attribute expected")
    };
    assert_eq!(
        child_attribute.target,
        AttributeTarget::Item(Item::Function(lowered.module.functions[0].id))
    );
    let range = lowered
        .module
        .attribute_target_origin(child_attribute.target)
        .diagnostic_span();
    assert_eq!(&source.text()[range.start..range.end], "#[meta] fn f() {}");
    assert_eq!(
        &source.text()[child_attribute.span.start..child_attribute.span.end],
        "#[meta]"
    );
    assert!(Arc::ptr_eq(&source, &lowered.source));
    assert_eq!(lowered.module.functions.len(), 1);
    assert_eq!(lowered.module.functions[0].name, "f");
    assert_eq!(lowered.module.modules.len(), 1);
    let span = lowered
        .module
        .block(
            lowered.module.functions[0]
                .implementation
                .script_body()
                .unwrap(),
        )
        .origin
        .diagnostic_span();
    assert_eq!(&source.text()[span.start..span.end].trim(), &"{}");
}

#[test]
fn local_function_rows_preserve_owners_and_module_membership() {
    let lowered = common::lower_ok(include_str!(
        "../../tests/fixtures/lower/local_function_owners.kgr"
    ));
    let functions = &lowered.module.functions;
    assert_eq!(
        functions
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>(),
        [
            "outer", "first", "first", "last", "method", "child", "after"
        ]
    );
    for (index, function) in functions.iter().enumerate() {
        assert_eq!(function.id.index(), index);
        let body = function.implementation.script_body().unwrap();
        assert_eq!(
            body.owner(),
            HirOwner::Body(BodyOwner::Function(function.id))
        );
        let declaration = lowered.module.functions[function.id.index()]
            .origin
            .diagnostic_span();
        let body_span = lowered.module.block(body).origin.diagnostic_span();
        assert!(declaration.start <= body_span.start && declaration.end >= body_span.end);
        if let FunctionKind::Local { block } = function.kind {
            assert!(!lowered.module.items.contains(&Item::Function(function.id)));
            assert!(lowered.module.block(block).statements.iter().any(|stmt|
                matches!(lowered.module.stmt(*stmt).kind, StmtKind::Function(id) if id == function.id)));
        }
    }
    assert_eq!(
        lowered
            .module
            .attributes
            .iter()
            .map(|fact| fact.target)
            .collect::<Vec<_>>(),
        [
            AttributeTarget::Item(Item::Function(functions[1].id)),
            AttributeTarget::Item(Item::Function(functions[2].id)),
            AttributeTarget::Item(Item::Function(functions[5].id)),
        ]
    );
    for fact in &lowered.module.attributes {
        let AttributeTarget::Item(Item::Function(id)) = fact.target else {
            panic!("function target expected")
        };
        assert!(matches!(
            functions[id.index()].kind,
            FunctionKind::Local { .. }
        ));
        assert_eq!(
            lowered
                .module
                .attribute_target_origin(fact.target)
                .diagnostic_span(),
            lowered.module.functions[id.index()]
                .origin
                .diagnostic_span()
        );
    }
    assert_eq!(functions[1].generic_params.len(), 1);
    assert_eq!(
        lowered
            .module
            .generic_parameter(functions[1].generic_params[0])
            .name,
        "U"
    );
    assert!(functions[2].generic_params.is_empty());
    assert_eq!(lowered.module.impls[0].methods[0].function, functions[4].id);
}
