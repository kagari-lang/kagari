use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        map::DefinitionContext, mapping::DefinitionMappingError, metadata::DefinitionMetadata,
        table::DefinitionTableError,
    },
};
use kagari_types::{
    declaration::host::HostPassingStyle,
    host_interface::{
        HostFunctionDeclaration, HostInterface, HostParameter,
        type_declaration::HostTypeDeclaration, value_type::HostValueType,
    },
};

fn interface() -> DefinitionMetadata<HostInterface> {
    let context = DefinitionContext::new().unwrap();
    let player = HostTypeDeclaration::new(&context, "game.Player").unwrap();
    let function = HostFunctionDeclaration::new(
        &context,
        "game.exchange",
        vec![HostParameter {
            name: "player".into(),
            ty: HostValueType::Opaque(player.id),
            passing: HostPassingStyle::Owned,
        }],
        HostValueType::Tuple(vec![HostValueType::Opaque(player.id), HostValueType::I32]),
    )
    .unwrap();
    let interface = HostInterface {
        types: vec![player],
        functions: vec![function],
        ..Default::default()
    };
    interface.validate(&context.snapshot()).unwrap();
    DefinitionMetadata::checked(context.snapshot(), interface, &Default::default()).unwrap()
}

#[test]
fn record_roundtrip_preserves_contracts_and_imports_without_cross_scope_capture() {
    let cancel = CancellationToken::default();
    let scoped = interface();
    let original_id = scoped.records().functions[0].id;

    let context = DefinitionContext::new().unwrap();
    HostFunctionDeclaration::new(&context, "other.first", vec![], HostValueType::Unit).unwrap();
    let imported = scoped.import_into(&context, &cancel).unwrap();
    assert_ne!(original_id, imported.records().functions[0].id);
    assert!(matches!(
        scoped
            .definitions()
            .resolve(imported.records().functions[0].id),
        Err(DefinitionTableError::ForeignTable)
    ));
    imported.records().validate(imported.definitions()).unwrap();
    assert_eq!(
        imported.records(),
        scoped.import_into(&context, &cancel).unwrap().records()
    );
    assert_eq!(
        scoped.records().to_bytes(scoped.definitions()).unwrap(),
        imported.records().to_bytes(imported.definitions()).unwrap()
    );

    let encoded = scoped.records().to_bytes(scoped.definitions()).unwrap();
    let decoded = HostInterface::from_bytes(&encoded).unwrap();
    assert_ne!(original_id, decoded.records().functions[0].id);
    assert_eq!(
        decoded.import_into(&context, &cancel).unwrap().records(),
        imported.records()
    );
    drop(context);
    imported.records().validate(imported.definitions()).unwrap();
}

#[test]
fn foreign_records_and_cancelled_adoption_are_rejected() {
    let cancel = CancellationToken::default();
    let scoped = interface();
    let other = interface();
    assert!(matches!(
        DefinitionMetadata::checked(scoped.definitions().clone(), other.into_records(), &cancel),
        Err(DefinitionMappingError::Identity(
            DefinitionTableError::ForeignTable
        ))
    ));

    cancel.cancel();
    assert!(matches!(
        scoped.import_into(&DefinitionContext::new().unwrap(), &cancel),
        Err(DefinitionMappingError::Cancelled)
    ));
}
