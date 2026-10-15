//! Host schema references are remapped through scoped definition identities.
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, PackageId,
        map::{DefinitionContext, DefinitionMap},
        mapping::DefinitionMappingError,
        metadata::DefinitionMetadata,
        table::DefinitionTableError,
    },
};
use kagari_types::{
    declaration::{host::HostPassingStyle, logging},
    host_interface::{
        HostFunctionDeclaration, HostInterface, HostInterfaceError, host_type_identity,
        type_declaration::{HostFieldDeclaration, HostMethodDeclaration, HostTypeDeclaration},
        value_type::HostValueType,
    },
    scalar::BuiltinType,
    ty::Ty,
};

#[test]
fn host_construction_owns_members_and_preserves_publication_validation() {
    let context = DefinitionContext::new().unwrap();
    let mut owner = HostTypeDeclaration::new(&context, "game.Player").unwrap();
    let old_prefix = context.snapshot();
    let field = HostFieldDeclaration::new(&context, owner.id, "score", HostValueType::I32).unwrap();
    let method =
        HostMethodDeclaration::new(&context, owner.id, "read", vec![], HostValueType::I32).unwrap();
    assert!(old_prefix.resolve(field.id).is_err());
    assert_eq!(context.snapshot().parent(field.id).unwrap(), Some(owner.id));
    assert_eq!(
        context.lookup(&host_type_identity("game.Player")),
        Some(owner.id)
    );
    let foreign = DefinitionContext::new().unwrap();
    assert_eq!(
        HostFieldDeclaration::new(&foreign, owner.id, "score", HostValueType::I32),
        Err(DefinitionTableError::ForeignTable)
    );
    assert_eq!(
        HostMethodDeclaration::new(&foreign, owner.id, "read", vec![], HostValueType::I32),
        Err(DefinitionTableError::ForeignTable)
    );
    owner.fields.push(field);
    owner.methods.push(method);
    let make = HostFunctionDeclaration::new(
        &context,
        "game.make",
        vec![],
        HostValueType::Opaque(owner.id),
    )
    .unwrap();
    let interface = HostInterface {
        types: vec![owner],
        functions: vec![make],
        ..Default::default()
    };
    let metadata =
        DefinitionMetadata::checked(context.snapshot(), interface, &Default::default()).unwrap();
    metadata.records().validate(metadata.definitions()).unwrap();
    let callable = metadata.records().types[0]
        .method_contract(
            &metadata.records().types[0].methods[0].id,
            metadata.definitions(),
        )
        .unwrap();
    callable.validate(metadata.definitions()).unwrap();
    assert_eq!(
        callable.params[0].ty,
        HostValueType::Opaque(metadata.records().types[0].id)
    );

    // Identity construction does not turn invalid symbols or foreign signature
    // references into verified Host contracts.
    let invalid =
        HostFunctionDeclaration::new(&context, "game..bad", vec![], HostValueType::Unit).unwrap();
    assert_eq!(
        invalid.validate(&context.snapshot()),
        Err(HostInterfaceError::InvalidDeclaration)
    );
    let foreign_owner = HostTypeDeclaration::new(&foreign, "game.Player").unwrap();
    let invalid = HostFunctionDeclaration::new(
        &context,
        "game.foreign",
        vec![],
        HostValueType::Opaque(foreign_owner.id),
    )
    .unwrap();
    assert!(matches!(
        DefinitionMetadata::checked(context.snapshot(), invalid, &Default::default()),
        Err(DefinitionMappingError::Identity(
            DefinitionTableError::ForeignTable
        ))
    ));
}

#[test]
fn log_declaration_retains_scoped_contract_and_canonical_binding_after_adoption() {
    let log = logging::declaration().unwrap();
    let repeated = logging::declaration().unwrap();
    assert_eq!(log.records().functions, repeated.records().functions);
    assert_eq!(log.records().identity, repeated.records().identity);
    assert_eq!(log.definitions().id(), repeated.definitions().id());
    log.records()
        .validate(&|_| None, log.definitions())
        .unwrap();
    let contract = log
        .records()
        .native_declarations(log.definitions())
        .unwrap()
        .remove(0);
    let function = &contract.function;
    let policy = function.host_policy.as_ref().unwrap();
    assert_eq!(function.name, "log");
    assert_eq!(function.params.len(), 1);
    assert_eq!(function.params[0].name, "message");
    assert_eq!(function.params[0].ty, Ty::Builtin(BuiltinType::String));
    assert_eq!(policy.passing, [HostPassingStyle::SharedBorrow]);
    assert_eq!(function.return_type, Ty::Builtin(BuiltinType::Unit));
    assert!(policy.effects.may_trap);
    assert!(policy.effects.may_call_host_services);
    assert!(policy.effects.may_mutate_host_state);
    let expected = DefinitionPath {
        module: ModuleIdentity {
            package: PackageId("host".into()),
            path: vec![],
        },
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Function,
            name: "log".into(),
            occurrence: 0,
        }],
    };
    assert_eq!(
        log.definitions()
            .resolve(contract.declaration)
            .unwrap()
            .to_path(),
        expected
    );
    let target = DefinitionContext::new().unwrap();
    target.intern(&path("unrelated")).unwrap();
    let retained = target.snapshot();
    let imported = log.import_into(&target, &Default::default()).unwrap();
    let adopted = imported
        .records()
        .native_declarations(imported.definitions())
        .unwrap()
        .remove(0);
    assert_ne!(contract.declaration, adopted.declaration);
    assert!(retained.resolve(adopted.declaration).is_err());
    imported
        .records()
        .validate(&|_| None, imported.definitions())
        .unwrap();
    assert_eq!(
        imported
            .definitions()
            .resolve(adopted.declaration)
            .unwrap()
            .to_path(),
        expected
    );
    let comparison = DefinitionContext::new().unwrap();
    assert_eq!(
        log.import_into(&comparison, &Default::default())
            .unwrap()
            .records()
            .native_declarations(&comparison.snapshot())
            .unwrap(),
        imported
            .import_into(&comparison, &Default::default())
            .unwrap()
            .records()
            .native_declarations(&comparison.snapshot())
            .unwrap(),
    );
}

fn path(name: &str) -> DefinitionPath {
    DefinitionPath {
        module: ModuleIdentity::single_file("index.kgr"),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Function,
            name: name.into(),
            occurrence: 0,
        }],
    }
}

#[test]
fn record_import_remaps_values_and_rejects_foreign_references_and_cancellation() {
    let mut source = DefinitionMap::default();
    let original = source.context().intern(&path("Player")).unwrap();
    source
        .insert(
            path("run"),
            HostValueType::Tuple(vec![
                HostValueType::Opaque(original),
                HostValueType::Opaque(original),
            ]),
        )
        .unwrap();
    let target = DefinitionContext::new().unwrap();
    target.intern(&path("unrelated")).unwrap();
    let cancel = CancellationToken::default();
    let imported = source.import_records(&target, &cancel).unwrap();
    let mapped = target.lookup(&path("Player")).unwrap();
    assert_ne!(original, mapped);
    assert_eq!(
        imported.get(&path("run")),
        Some(&HostValueType::Tuple(vec![
            HostValueType::Opaque(mapped),
            HostValueType::Opaque(mapped),
        ]))
    );
    assert!(imported.get_id(source.ids().next().unwrap()).is_none());
    assert_eq!(
        target.snapshot().resolve(mapped).unwrap().to_path(),
        path("Player")
    );
    source
        .insert(path("invalid"), HostValueType::Opaque(mapped))
        .unwrap();
    assert!(matches!(
        source.import_records(&target, &cancel),
        Err(DefinitionMappingError::Identity(
            DefinitionTableError::ForeignTable
        ))
    ));
    cancel.cancel();
    let empty: DefinitionMap<HostValueType> = DefinitionMap::default();
    assert!(matches!(
        empty.import_records(&target, &cancel),
        Err(DefinitionMappingError::Cancelled)
    ));
}
