use super::*;
use crate::{
    analysis::FileAnalysis,
    callable::CallableSignature,
    declarations::DeclarationId,
    hir::expr::ExprKind,
    host::tests::metadata,
    native::NativeBinding,
    tests::{common::import_facts, test_analysis},
    typeck::FunctionImplementation,
};
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_source::{
    diagnostic::DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};

use kagari_types::{
    declaration::{
        host::{HostPassingStyle, HostTypeOwnership, PathAccess},
        path::{HostPathDeclaration, HostPathSegmentDeclaration},
    },
    host_interface::{
        HostParameter,
        type_declaration::{HostFieldDeclaration, HostMethodDeclaration},
    },
};

fn interface(context: &DefinitionContext) -> HostInterface {
    let left = HostTypeDeclaration::new(context, "left.Item").unwrap();
    let right = HostTypeDeclaration::new(context, "right.Item").unwrap();
    let make =
        HostFunctionDeclaration::new(context, "left.make", vec![], HostValueType::Opaque(left.id))
            .unwrap();
    let take = HostFunctionDeclaration::new(
        context,
        "left.take",
        vec![HostParameter {
            name: "value".into(),
            ty: HostValueType::Opaque(left.id),
            passing: HostPassingStyle::SharedBorrow,
        }],
        HostValueType::I32,
    )
    .unwrap();
    HostInterface {
        paths: vec![],
        types: vec![left, right],
        functions: vec![make, take],
    }
}

#[test]
fn host_methods_keep_checked_receiver_targets_and_offline_documentation() {
    let context = DefinitionContext::new().unwrap();
    let mut declarations = interface(&context);
    let owner = &mut declarations.types[0];
    let mut method = HostMethodDeclaration::new(
        &context,
        owner.id,
        "add",
        vec![HostParameter {
            name: "amount".into(),
            ty: HostValueType::I32,
            passing: HostPassingStyle::Owned,
        }],
        HostValueType::I32,
    )
    .unwrap();
    method.documentation = "Add to the host counter".into();
    let identity = method.id;
    owner.methods.push(method);
    for argument in ["2", "true", "", "2, 3"] {
        let text = format!("use left as api; fn main() -> i32 {{ api::make().add({argument}) }}");
        let mut sources = SourceDatabase::default();
        let root = sources
            .set("mem://method", text.clone(), SourceLayer::Base)
            .unwrap();
        let mut db = test_analysis();
        let hosts = HostDeclarations::new(metadata(&context, declarations.clone())).unwrap();
        let method_id = hosts.method(&declarations.types[0].id, "add").unwrap();
        let callable = hosts.callable(method_id).unwrap();
        assert_eq!(
            callable.implementation(),
            FunctionImplementation::Native(NativeBinding::Host(method_id))
        );
        assert_eq!(
            callable.contract().params[0].passing,
            HostPassingStyle::SharedBorrow
        );
        assert_eq!(callable.parameters().len(), 2);
        db.set_host_declarations(hosts);
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
        let target = file.host_function_at(text.find("add(").unwrap()).unwrap();
        let imported_target = import_facts(target, file.definitions(), &context);
        let target = imported_target.records();
        assert_eq!(file.host_function_at(text.find(".add(").unwrap()), None);
        assert_eq!(
            file.host_function_at(text.find("make()").unwrap())
                .unwrap()
                .symbol,
            "left.make"
        );
        if argument.is_empty() {
            assert!(
                file.result()
                    .diagnostics()
                    .iter()
                    .any(|diagnostic| matches!(
                        diagnostic.kind,
                        DiagnosticKind::CallArityMismatch {
                            expected: 1,
                            found: 0,
                            ..
                        }
                    ))
            );
        }
        assert_eq!(target.id, identity);
        assert_eq!(target.documentation, "Add to the host counter");
        let signature = file.call_signature_at(text.find("add(").unwrap()).unwrap();
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
            target.params[0].ty,
            HostValueType::Opaque(declarations.types[0].id)
        );
        assert_eq!(
            snapshot.check_program(root, &Default::default()).is_ok(),
            argument == "2"
        );
    }
}

#[test]
fn host_types_resolve_through_facades_and_keep_revision_owned_query_facts() {
    let context = DefinitionContext::new().unwrap();
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
    let root = sources
        .set("mem://root", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    db.set_host_declarations(
        HostDeclarations::new(metadata(&context, interface(&context))).unwrap(),
    );

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
                .resolve(interface(&context).types[0].id)
                .unwrap()
                .to_path(),
        )
        .unwrap();
    for spelling in [
        "Object as Item",
        "facade::Object",
        "api::Item",
        "Item =",
        "value))",
    ] {
        assert_eq!(
            file.host_type_at(
                text.find(spelling).unwrap()
                    + match spelling {
                        "facade::Object" => "facade::".len(),
                        "api::Item" => "api::".len(),
                        _ => 0,
                    },
            )
            .unwrap()
            .id,
            id,
            "{spelling}"
        );
    }
    let mut changed = interface(&context);
    changed.types[0].documentation = "updated".into();
    changed.functions[0].return_type = HostValueType::Opaque(changed.types[1].id);
    db.set_host_declarations(HostDeclarations::new(metadata(&context, changed)).unwrap());
    let new = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(!new.file(root).unwrap().result().diagnostics().is_empty());
    let offset = text.find("facade::Object").unwrap() + "facade::".len();
    let qualifier = text.find("facade::Object").unwrap();
    assert!(file.host_type_at(qualifier).is_none());
    assert!(file.host_type_at(qualifier + "facade".len()).is_none());
    assert!(file.host_type_at(offset + "Object".len()).is_none());
    let signature = old.signature_snapshot().file(root).unwrap();
    assert_eq!(signature.host_type_at(offset).unwrap().id, id);
    assert!(signature.host_type_at(qualifier).is_none());
    assert!(signature.host_type_at(qualifier + "facade".len()).is_none());
    assert!(signature.host_type_at(offset + "Object".len()).is_none());
    assert!(file.host_type_at(offset).unwrap().documentation.is_empty());
    assert_eq!(
        new.file(root)
            .unwrap()
            .host_type_at(offset)
            .unwrap()
            .documentation,
        "updated"
    );
    assert_eq!(
        file.type_at(text.find("left::make()").unwrap()),
        Some(TypeId::Host(id))
    );
}

#[test]
fn erroneous_host_calls_retain_return_types_and_member_facts() {
    let context = DefinitionContext::new().unwrap();
    let mut declarations = interface(&context);
    let parameter = HostParameter {
        name: "amount".into(),
        ty: HostValueType::I32,
        passing: HostPassingStyle::Owned,
    };
    declarations.functions[0].params.push(parameter.clone());
    let owner = &mut declarations.types[0];
    owner.ownership = HostTypeOwnership::HostRoot;
    owner.path_access = PathAccess::ReadOnly;
    owner.methods.push(
        HostMethodDeclaration::new(
            &context,
            owner.id,
            "next",
            vec![parameter],
            HostValueType::Opaque(owner.id),
        )
        .unwrap(),
    );
    let mut field =
        HostFieldDeclaration::new(&context, owner.id, "score", HostValueType::I32).unwrap();
    field.path_access = PathAccess::ReadOnly;
    owner.fields.push(field.clone());
    declarations.paths.push(HostPathDeclaration {
        root: owner.id,
        segments: vec![HostPathSegmentDeclaration::Field(field.id)],
        access: PathAccess::ReadOnly,
        schema_epoch: 0,
    });
    let host_type = TypeId::Host(owner.id);
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
        db.set_host_declarations(
            HostDeclarations::new(metadata(&context, declarations.clone())).unwrap(),
        );
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
            imported_field_at(file, text.find("score").unwrap(), &context),
            Some(field.clone())
        );
        assert_eq!(
            imported_field_at(file, text.find(call).unwrap(), &context),
            None
        );
        assert_eq!(
            imported_field_at(file, text.find(".score").unwrap(), &context),
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
    let context = DefinitionContext::new().unwrap();
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
        db.set_host_declarations(
            HostDeclarations::new(metadata(&context, interface(&context))).unwrap(),
        );
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
                                .resolve(interface(&context).types[0].id)
                                .unwrap()
                                .to_path()
                        )
                        .unwrap()
                ))
            );
        }
        if let Some(offset) = text.find("Item<i32>") {
            assert_eq!(
                file.host_type_at(offset).unwrap().id,
                file.definitions()
                    .lookup(
                        &context
                            .snapshot()
                            .resolve(interface(&context).types[0].id)
                            .unwrap()
                            .to_path()
                    )
                    .unwrap()
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

/// Compare an offline query with its provider contract in the fixture scope.
/// Reference validation still uses the query's analysis table before import.
fn imported_field_at(
    file: &FileAnalysis,
    offset: usize,
    context: &DefinitionContext,
) -> Option<HostFieldDeclaration> {
    file.host_field_at(offset)
        .map(|field| import_facts(field, file.definitions(), context).into_records())
}
