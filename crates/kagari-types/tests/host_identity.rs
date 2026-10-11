//! Host schema references are remapped through scoped definition identities.
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, PackageId,
        map::{DefinitionContext, DefinitionMap},
        mapping::DefinitionMappingError,
        table::{DefinitionId, DefinitionTableError},
    },
};
use kagari_types::host_interface::{HostPassingStyle, standard_log, value_type::HostValueType};

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
    let empty: DefinitionMap<HostValueType<DefinitionId>> = DefinitionMap::default();
    assert!(matches!(
        empty.import_records(&target, &cancel),
        Err(DefinitionMappingError::Cancelled)
    ));
}
