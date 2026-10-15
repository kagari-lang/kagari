//! External methods and nominal queries use the same common declaration owner.
use super::*;
use crate::{
    declarations::DeclarationId, hir::expr::ExprKind, native::NativeBinding,
    typeck::FunctionImplementation,
};

#[test]
fn host_methods_keep_checked_receiver_targets_and_offline_documentation() {
    let mut declarations = Hosts::new();
    let context = declarations.context.clone();
    let identity = declarations.method("add", Ty::Builtin(BuiltinType::I32));
    declarations.modules[0]
        .documentation
        .insert(identity, "Add to the host counter".into());
    for argument in ["2", "true", "", "2, 3"] {
        let text = format!("use left as api; fn main() -> i32 {{ api::make().add({argument}) }}");
        let mut sources = SourceDatabase::default();
        let root = sources
            .set("mem://method", text.clone(), SourceLayer::Base)
            .unwrap();
        let mut db = test_analysis();
        declarations.install(&mut db);
        let snapshot = db
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let file = snapshot.file(root).unwrap();
        assert_eq!(
            file.result().diagnostics().is_empty(),
            argument == "2",
            "{:?}",
            file.result().diagnostics()
        );
        let offset = text.find("add(").unwrap();
        let target = file.source_function_at(offset).unwrap();
        let FunctionImplementation::Native(NativeBinding::Entry(binding)) =
            target.signature.implementation
        else {
            panic!("checked native member");
        };
        let mapping = context
            .import(file.definitions(), [target.declaration, binding])
            .unwrap();
        assert_eq!(mapping.map(target.declaration).unwrap(), identity);
        assert_eq!(mapping.map(binding).unwrap(), identity);
        assert_eq!(target.signature.params.len(), 2);
        let provider = snapshot.file(target.site.location.file).unwrap();
        let contract = provider
            .result()
            .facts()
            .lowered
            .registered_native_declarations()
            .iter()
            .find(|contract| contract.declaration == binding)
            .unwrap();
        assert_eq!(
            contract.function.host_policy.as_ref().unwrap().passing[0],
            HostPassingStyle::SharedBorrow
        );
        assert_eq!(file.source_function_at(text.find(".add(").unwrap()), None);
        assert_eq!(
            file.source_function_at(text.find("make()").unwrap())
                .unwrap()
                .signature
                .name,
            "make"
        );
        if argument.is_empty() {
            assert!(file.result().diagnostics().iter().any(|d| matches!(
                d.kind,
                DiagnosticKind::CallArityMismatch {
                    expected: 1,
                    found: 0,
                    ..
                }
            )));
        }
        assert_eq!(
            snapshot
                .documentation_at(root, offset)
                .unwrap()
                .documentation,
            "Add to the host counter"
        );
        let signature = file.call_signature_at(offset).unwrap();
        assert_eq!(
            import_facts(&signature.declaration, file.definitions(), &context).into_records(),
            DeclarationId::Definition(identity)
        );
        assert_eq!(
            signature.parameters,
            [("amount".into(), TypeId::Builtin(BuiltinType::I32))]
        );
        assert_eq!(signature.result, TypeId::Builtin(BuiltinType::I32));
        assert_eq!(
            import_facts(&target.signature.params[0].ty, file.definitions(), &context)
                .into_records(),
            TypeId::Host(declarations.owners[0])
        );
        assert_eq!(
            snapshot.check_program(root, &Default::default()).is_ok(),
            argument == "2"
        );
    }
}

#[test]
fn host_types_resolve_through_facades_and_keep_revision_owned_query_facts() {
    let declarations = Hosts::new();
    let context = declarations.context.clone();
    let mut sources = SourceDatabase::default();
    sources
        .bind_module(
            "mem://facade",
            ModuleIdentity {
                package: PackageId("pkg".into()),
                path: vec!["facade".into()],
            },
        )
        .unwrap();
    sources
        .set(
            "mem://facade",
            "pub use left::Item as Object; pub use left as api;".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let text = "use pkg::facade::{Object as Item, api}; use pkg::facade; fn pass(value: facade::Object) -> api::Item { value } fn main() -> i32 { val value: Item = left::make(); left::take(pass(value)) }";
    sources
        .bind_module(
            "mem://root",
            ModuleIdentity {
                package: PackageId("pkg".into()),
                path: vec!["root".into()],
            },
        )
        .unwrap();
    let root = sources
        .set("mem://root", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    declarations.install(&mut db);

    let old = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let file = old.file(root).unwrap();
    assert!(
        file.result().diagnostics().is_empty(),
        "{:?}",
        file.result().diagnostics()
    );
    old.check_program(root, &Default::default()).unwrap();
    let id = file
        .definitions()
        .lookup(
            &context
                .snapshot()
                .resolve(declarations.owners[0])
                .unwrap()
                .to_path(),
        )
        .unwrap();
    for spelling in ["Object as Item", "facade::Object", "api::Item", "Item ="] {
        assert_eq!(
            old.definition_at(
                root,
                text.find(spelling).unwrap()
                    + match spelling {
                        "facade::Object" => "facade::".len(),
                        "api::Item" => "api::".len(),
                        _ => 0,
                    },
            )
            .unwrap_or_else(|| panic!("missing declaration for {spelling}"))
            .id,
            DeclarationId::Definition(id),
            "{spelling}"
        );
    }
    let mut changed = Hosts {
        context: context.clone(),
        modules: declarations.modules.clone(),
        owners: declarations.owners,
    };
    changed.modules[0]
        .documentation
        .insert(changed.owners[0], "updated".into());
    changed.modules[0].functions[0].return_type = Ty::Host(changed.owners[1]);
    changed.install(&mut db);
    let new = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(!new.file(root).unwrap().result().diagnostics().is_empty());
    let offset = text.find("facade::Object").unwrap() + "facade::".len();
    let qualifier = text.find("facade::Object").unwrap();
    assert!(file.definition_at(qualifier).is_none());
    assert!(file.definition_at(qualifier + "facade".len()).is_none());
    assert!(file.definition_at(offset + "Object".len()).is_none());
    let signature = old.signature_snapshot().file(root).unwrap();
    assert_eq!(
        signature.definition_at(offset).unwrap().id,
        DeclarationId::Definition(id)
    );
    assert!(signature.definition_at(qualifier).is_none());
    assert!(
        signature
            .definition_at(qualifier + "facade".len())
            .is_none()
    );
    assert!(signature.definition_at(offset + "Object".len()).is_none());
    assert!(
        old.documentation_at(root, offset)
            .unwrap()
            .documentation
            .is_empty()
    );
    assert_eq!(
        new.documentation_at(root, offset).unwrap().documentation,
        "updated"
    );
    let actual = signature.declarations().external_type(id).unwrap();
    assert_eq!(
        import_facts(actual, signature.declarations().definitions(), &context).into_records(),
        declarations.modules[0].types[0]
    );
    assert_eq!(
        file.type_at(text.find("value))").unwrap()),
        Some(TypeId::Host(id))
    );
    assert_eq!(
        file.type_at(text.find("left::make()").unwrap()),
        Some(TypeId::Host(id))
    );
}

#[test]
fn erroneous_host_calls_retain_return_types_and_member_facts() {
    let mut declarations = Hosts::new();
    let context = declarations.context.clone();
    declarations.modules[0].functions[0].params.push(Param {
        name: "amount".into(),
        ty: Ty::Builtin(BuiltinType::I32),
        mutable: false,
    });
    declarations.modules[0].functions[0]
        .host_policy
        .as_mut()
        .unwrap()
        .passing
        .push(HostPassingStyle::Owned);
    declarations.root(PathAccess::ReadOnly);
    declarations.method("next", Ty::Host(declarations.owners[0]));
    let field = declarations.field(
        0,
        "score",
        Ty::Builtin(BuiltinType::I32),
        PathAccess::ReadOnly,
    );
    declarations.modules[0].paths.push(HostPathDeclaration {
        root: declarations.owners[0],
        segments: vec![HostPathSegmentDeclaration::Field(field)],
        access: PathAccess::ReadOnly,
        schema_epoch: 0,
    });
    let host_type = TypeId::Host(declarations.owners[0]);
    for call in [
        "left::make()",
        "left::make(true, 2)",
        "value.next()",
        "value.next(true, 2)",
    ] {
        let text =
            format!("fn bad(value: left::Item) -> i32 {{ {call}.score }} fn good() -> i32 {{ 7 }}");
        let mut sources = SourceDatabase::default();
        let root = sources
            .set("mem://recovery", text.clone(), SourceLayer::Base)
            .unwrap();
        let mut db = test_analysis();
        declarations.install(&mut db);
        let snapshot = db
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let file = snapshot.file(root).unwrap();
        assert!(file.result().diagnostics().iter().any(|d| matches!(
            d.kind,
            DiagnosticKind::CallArityMismatch { expected: 1, .. }
        )));
        // The available first argument must still be checked when arity is wrong.
        if call.contains("true") {
            assert!(
                file.result().diagnostics().iter().any(|d| matches!(
                    &d.kind,
                    DiagnosticKind::ArgumentTypeMismatch { parameter_name, .. }
                        if parameter_name == "amount"
                )),
                "{:?}",
                file.result().diagnostics()
            );
        }
        let authoring_facts = file.to_unverified(&Default::default()).unwrap();
        let facts = authoring_facts.records().facts();
        let call_type = facts
            .lowered
            .module
            .body
            .expressions()
            .find_map(|(id, expr)| {
                matches!(expr.kind, ExprKind::Call { .. })
                    .then(|| facts.typed.type_table.expr_type(id))
                    .flatten()
            });
        assert_eq!(
            call_type.map(|ty| import_facts(&ty, file.definitions(), &context).into_records()),
            Some(host_type.clone())
        );
        assert_eq!(
            imported_field_at(file, text.find("score").unwrap(), &declarations),
            Some(field)
        );
        assert_eq!(
            imported_field_at(file, text.find(call).unwrap(), &declarations),
            None
        );
        assert_eq!(
            imported_field_at(file, text.find(".score").unwrap(), &declarations),
            None
        );

        assert_eq!(
            file.type_at(text.rfind('7').unwrap()),
            Some(TypeId::Builtin(BuiltinType::I32))
        );
        assert!(snapshot.check_program(root, &Default::default()).is_err());
    }
}

#[test]
fn host_type_rules_preserve_value_namespaces_and_neighbor_functions() {
    let declarations = Hosts::new();
    let context = declarations.context.clone();
    for (source, valid) in [
        (
            "use right::Item; fn bad(value: Item) -> i32 { left::take(value) }",
            false,
        ),
        (
            "use left::Item; fn bad(a: Item, b: Item) -> bool { a == b }",
            false,
        ),
        ("use left::Item; fn bad() { val value = Item {}; }", false),
        ("use left::Item; fn bad(value: Item<i32>) {}", false),
        ("fn left() {} fn accept(value: left::Item) {}", true),
        ("use missing as left; fn bad(value: left::Item) {}", false),
        ("fn bad(value: left::Item) { value. }", false),
    ] {
        let mut sources = SourceDatabase::default();
        let text = format!("{source} fn good() -> i32 {{ 7 }}");
        let root = sources
            .set("mem://root", text.clone(), SourceLayer::Base)
            .unwrap();
        let mut db = test_analysis();
        declarations.install(&mut db);
        let snapshot = db
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let file = snapshot.file(root).unwrap();
        assert_eq!(
            file.result().diagnostics().is_empty(),
            valid,
            "{source}: {:?}",
            file.result().diagnostics()
        );
        if let Some(offset) = text.find("value.") {
            assert_eq!(
                file.member_receiver_type(offset + "value.".len()),
                Some(TypeId::Host(
                    file.definitions()
                        .lookup(
                            &context
                                .snapshot()
                                .resolve(declarations.owners[0])
                                .unwrap()
                                .to_path()
                        )
                        .unwrap()
                ))
            );
        }
        if let Some(offset) = text.find("Item<i32>") {
            assert_eq!(
                file.definition_at(offset).unwrap().id,
                DeclarationId::Definition(
                    file.definitions()
                        .lookup(
                            &context
                                .snapshot()
                                .resolve(declarations.owners[0])
                                .unwrap()
                                .to_path()
                        )
                        .unwrap()
                )
            );
        }
        assert_eq!(
            file.type_at(text.rfind('7').unwrap()),
            Some(TypeId::Builtin(BuiltinType::I32))
        );
        assert_eq!(
            snapshot.check_program(root, &Default::default()).is_ok(),
            valid
        );
    }
}
