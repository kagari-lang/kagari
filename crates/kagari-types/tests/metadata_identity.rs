use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity,
        map::{DefinitionContext, DefinitionMap},
        mapping::DefinitionMappingError,
        table::DefinitionTableError,
    },
};
use kagari_types::ty::Ty;

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
            Ty::Tuple(vec![Ty::Host(original), Ty::Host(original)]),
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
        Some(&Ty::Tuple(vec![Ty::Host(mapped), Ty::Host(mapped),]))
    );
    assert!(imported.get_id(source.ids().next().unwrap()).is_none());
    assert_eq!(
        target.snapshot().resolve(mapped).unwrap().to_path(),
        path("Player")
    );
    source.insert(path("invalid"), Ty::Host(mapped)).unwrap();
    assert!(matches!(
        source.import_records(&target, &cancel),
        Err(DefinitionMappingError::Identity(
            DefinitionTableError::ForeignTable
        ))
    ));
    cancel.cancel();
    let empty: DefinitionMap<Ty> = DefinitionMap::default();
    assert!(matches!(
        empty.import_records(&target, &cancel),
        Err(DefinitionMappingError::Cancelled)
    ));
}
