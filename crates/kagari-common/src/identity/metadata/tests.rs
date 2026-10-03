use super::*;
use crate::{
    host_interface::{
        HostFunctionDeclaration, HostInterface, HostParameter, HostPassingStyle,
        value_type::HostValueType,
    },
    identity::table::DefinitionTableError,
};
use bincode::{DefaultOptions, Options};

fn interface() -> HostInterface {
    HostInterface {
        functions: vec![HostFunctionDeclaration::new(
            "game.exchange",
            vec![HostParameter {
                name: "player".into(),
                ty: HostValueType::opaque("game.Player"),
                passing: HostPassingStyle::Owned,
            }],
            HostValueType::Tuple(vec![
                HostValueType::opaque("game.Player"),
                HostValueType::I32,
            ]),
        )],
        ..Default::default()
    }
}

fn bytes<T: Serialize>(value: &T) -> Vec<u8> {
    DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .serialize(value)
        .unwrap()
}

#[test]
fn record_roundtrip_preserves_contracts_and_imports_without_cross_scope_capture() {
    let cancel = CancellationToken::default();
    let authoring = interface();
    let scoped = scope_record(&authoring, &cancel).unwrap();
    let original_id = scoped.records().functions[0].id;
    assert_eq!(scoped.to_paths(&cancel).unwrap(), authoring);

    let context = DefinitionContext::new().unwrap();
    let unrelated = HostFunctionDeclaration::new("other.first", vec![], HostValueType::Unit).id;
    context.intern(&unrelated).unwrap();
    let imported = scoped.import_into(&context, &cancel).unwrap();
    assert_ne!(original_id, imported.records().functions[0].id);
    assert!(matches!(
        scoped
            .definitions()
            .resolve(imported.records().functions[0].id),
        Err(DefinitionTableError::ForeignTable)
    ));
    assert_eq!(imported.to_paths(&cancel).unwrap(), authoring);
    assert_eq!(
        imported.records(),
        scoped.import_into(&context, &cancel).unwrap().records()
    );
    assert_eq!(
        bytes(&scoped.to_portable(&cancel).unwrap()),
        bytes(&imported.to_portable(&cancel).unwrap())
    );

    let encoded = bytes(&scoped.to_portable(&cancel).unwrap());
    let portable: PortableMetadata<HostInterface<PortableDefinitionRef>> = DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .reject_trailing_bytes()
        .deserialize(&encoded)
        .unwrap();
    let decoded = portable.decode(&cancel).unwrap();
    assert_ne!(original_id, decoded.records().functions[0].id);
    assert_eq!(decoded.to_paths(&cancel).unwrap(), authoring);
    drop(context);
    assert_eq!(imported.to_paths(&cancel).unwrap(), authoring);
}

#[test]
fn foreign_records_and_out_of_range_portable_references_are_rejected() {
    let cancel = CancellationToken::default();
    let scoped = scope_record(&interface(), &cancel).unwrap();
    let other = scope_record(&interface(), &cancel).unwrap();
    assert!(matches!(
        DefinitionMetadata::checked(scoped.definitions().clone(), other.into_records(), &cancel),
        Err(DefinitionMappingError::Identity(
            DefinitionTableError::ForeignTable
        ))
    ));

    let mut portable = scoped.to_portable(&cancel).unwrap();
    portable.records.functions[0].id = DefaultOptions::new()
        .with_fixint_encoding()
        .deserialize(&u32::MAX.to_le_bytes())
        .unwrap();
    assert!(matches!(
        portable.decode(&cancel),
        Err(DefinitionMappingError::Identity(_))
    ));
}

#[test]
fn resolved_host_types_still_validate_identity_kind_and_mapping_observes_cancellation() {
    let cancel = CancellationToken::default();
    let scoped = scope_record(&interface(), &cancel).unwrap();
    let mut portable = scoped.to_portable(&cancel).unwrap();
    portable.records.functions[0].return_type =
        HostValueType::Opaque(portable.records.functions[0].id);
    let decoded = portable.decode(&cancel).unwrap();
    assert!(matches!(
        decoded.to_paths(&cancel),
        Err(DefinitionMappingError::InvalidContract)
    ));

    cancel.cancel();
    assert!(matches!(
        scope_record(&interface(), &cancel),
        Err(DefinitionMappingError::Cancelled)
    ));
    assert!(matches!(
        scoped.to_portable(&cancel),
        Err(DefinitionMappingError::Cancelled)
    ));
}
