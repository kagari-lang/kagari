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
use kagari_types::host_interface::{
    HostFunctionDeclaration, HostInterface, HostInterfaceError, HostPassingStyle,
    host_type_identity, standard_log,
    type_declaration::{HostFieldDeclaration, HostMethodDeclaration, HostTypeDeclaration},
    value_type::HostValueType,
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
fn standard_log_retains_scoped_contract_and_canonical_binding_after_adoption() {
    let log = standard_log().unwrap();
    let repeated = standard_log().unwrap();
    assert_eq!(log.records(), repeated.records());
    assert_eq!(log.definitions().id(), repeated.definitions().id());
    let contract = log.records();
    contract.validate(log.definitions()).unwrap();
    assert_eq!(contract.symbol, "host.log");
    assert_eq!(contract.params.len(), 1);
    assert_eq!(contract.params[0].name, "message");
    assert_eq!(contract.params[0].ty, HostValueType::String);
    assert_eq!(contract.params[0].passing, HostPassingStyle::SharedBorrow);
    assert_eq!(contract.return_type, HostValueType::Unit);
    assert!(contract.effects.may_trap);
    assert!(contract.effects.may_call_host_services);
    assert!(contract.effects.may_mutate_host_state);
    assert_eq!(
        log.definitions().resolve(contract.id).unwrap().to_path(),
        DefinitionPath {
            module: ModuleIdentity {
                package: PackageId("host".into()),
                path: vec!["host".into()]
            },
            path: vec![DefinitionPathSegment {
                kind: DefinitionKind::Function,
                name: "log".into(),
                occurrence: 0
            }],
        },
    );
    let target = DefinitionContext::new().unwrap();
    target.intern(&path("unrelated")).unwrap();
    let retained = target.snapshot();
    let imported = log.import_into(&target, &Default::default()).unwrap();
    assert_ne!(contract.id, imported.records().id);
    assert!(retained.resolve(imported.records().id).is_err());
    imported.records().validate(imported.definitions()).unwrap();
    assert_eq!(
        contract.fingerprint(log.definitions()).unwrap(),
        imported
            .records()
            .fingerprint(imported.definitions())
            .unwrap(),
    );
    assert_eq!(
        log.definitions().resolve(contract.id).unwrap().to_path(),
        imported
            .definitions()
            .resolve(imported.records().id)
            .unwrap()
            .to_path(),
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
