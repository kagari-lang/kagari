use super::*;
use std::mem::size_of;

fn path(module: &str, name: &str) -> DefinitionId {
    DefinitionId {
        module: ModuleIdentity::single_file(module),
        path: vec![
            DefinitionPathSegment {
                kind: DefinitionKind::Struct,
                name: "Player".into(),
                occurrence: 0,
            },
            DefinitionPathSegment {
                kind: DefinitionKind::Field,
                name: name.into(),
                occurrence: 0,
            },
        ],
    }
}

#[test]
fn short_ids_share_paths_and_names_without_deep_copy() {
    assert_eq!(size_of::<ScopedDefinitionId>(), 8);
    let mut table = DefinitionTableBuilder::new().unwrap();
    let expected = path("game", "hp");
    let hp = table.intern_path(&expected).unwrap();
    assert_eq!(hp, table.intern_path(&expected).unwrap());
    let mp = table.intern_path(&path("game", "mp")).unwrap();
    let frozen = table.freeze();
    assert_eq!(frozen.len(), 4);
    assert_eq!(frozen.symbol_count(), 3);
    assert_eq!(frozen.resolve(hp).unwrap().to_path(), expected);
    assert_eq!(
        frozen
            .resolve(mp)
            .unwrap()
            .segments()
            .map(|part| part.name)
            .collect::<Vec<_>>(),
        ["Player", "mp"]
    );
}

#[test]
fn foreign_tables_and_absent_indices_do_not_resolve() {
    let mut first = DefinitionTableBuilder::new().unwrap();
    let mut second = DefinitionTableBuilder::new().unwrap();
    let hp = first.intern_path(&path("game", "hp")).unwrap();
    let other = second.intern_path(&path("game", "hp")).unwrap();
    assert_eq!(hp.index(), other.index());
    assert_ne!(hp, other);
    assert_eq!(
        second.resolve(hp).unwrap_err(),
        DefinitionTableError::ForeignTable
    );
    assert_eq!(
        second.intern_child(hp, DefinitionKind::Field, "mp", 0),
        Err(DefinitionTableError::ForeignTable)
    );
    let absent = ScopedDefinitionId {
        table: hp.table,
        index: DefinitionIndex(u32::MAX),
    };
    assert_eq!(
        first.resolve(absent).unwrap_err(),
        DefinitionTableError::InvalidIndex
    );
}

#[test]
fn immutable_snapshots_retain_valid_prefixes() {
    let mut builder = DefinitionTableBuilder::new().unwrap();
    let hp = builder.intern_path(&path("game", "hp")).unwrap();
    let before = builder.freeze();
    let retained = before.clone();
    assert!(Arc::ptr_eq(&before.data, &retained.data));
    let mp = builder.intern_path(&path("game", "mp")).unwrap();
    let after = builder.freeze();
    assert_eq!(before.id(), after.id());
    assert_eq!(before.len(), 3);
    assert_eq!(after.len(), 4);
    assert_eq!(
        before.resolve(mp).unwrap_err(),
        DefinitionTableError::InvalidIndex
    );
    assert_eq!(
        before.resolve(hp).unwrap().to_path(),
        after.resolve(hp).unwrap().to_path()
    );
    drop(builder);
    assert_eq!(retained.resolve(hp).unwrap().to_path(), path("game", "hp"));
}

#[test]
fn identity_kinds_occurrences_and_modules_remain_distinct() {
    let mut builder = DefinitionTableBuilder::new().unwrap();
    let base = path("game", "hp");
    let id = builder.intern_path(&base).unwrap();
    let mut changed = base.clone();
    changed.path[1].occurrence = 1;
    assert_ne!(id, builder.intern_path(&changed).unwrap());
    changed = base.clone();
    changed.path[1].kind = DefinitionKind::Method;
    assert_ne!(id, builder.intern_path(&changed).unwrap());
    changed = base;
    changed.module = ModuleIdentity::single_file("other");
    assert_ne!(id, builder.intern_path(&changed).unwrap());
}

#[test]
fn importing_is_explicit_deduplicated_and_covers_ancestors() {
    let mut source = DefinitionTableBuilder::new().unwrap();
    let hp = source.intern_path(&path("game", "hp")).unwrap();
    let mp = source.intern_path(&path("game", "mp")).unwrap();
    let source = source.freeze();
    let mut target = DefinitionTableBuilder::new().unwrap();
    let existing = target.intern_path(&path("game", "hp")).unwrap();
    let mapping = target.import(&source, [hp, hp]).unwrap();
    assert_eq!(mapping.map(hp).unwrap(), existing);
    assert_eq!(
        mapping.map(mp),
        Err(DefinitionTableError::UnmappedDefinition)
    );
    assert_eq!(
        mapping.map(existing),
        Err(DefinitionTableError::ForeignTable)
    );
    assert_eq!(target.freeze().len(), 3);
    assert_eq!(mapping.target(), existing.table());
    let mapping = target.import(&source, [mp]).unwrap();
    assert_eq!(
        target.resolve(mapping.map(mp).unwrap()).unwrap().to_path(),
        path("game", "mp")
    );
    let self_mapping = target.import(&target.freeze(), [existing]).unwrap();
    assert_eq!(self_mapping.map(existing).unwrap(), existing);
}

#[test]
fn canonical_encoding_ignores_insertion_order_and_unrelated_definitions() {
    let mut first = DefinitionTableBuilder::new().unwrap();
    let hp = first.intern_path(&path("game", "hp")).unwrap();
    let mp = first.intern_path(&path("game", "mp")).unwrap();
    first.intern_path(&path("other", "noise")).unwrap();
    let first = first.freeze().encode([hp, mp]).unwrap();
    let mut second = DefinitionTableBuilder::new().unwrap();
    let mp2 = second.intern_path(&path("game", "mp")).unwrap();
    let hp2 = second.intern_path(&path("game", "hp")).unwrap();
    let second = second.freeze().encode([mp2, hp2, hp2]).unwrap();
    assert_eq!(first.table, second.table);
    assert_eq!(
        bincode::serialize(&first.table).unwrap(),
        bincode::serialize(&second.table).unwrap()
    );
    assert_eq!(first.reference(hp).unwrap(), second.reference(hp2).unwrap());
    assert_eq!(first.reference(mp).unwrap(), second.reference(mp2).unwrap());
    assert_eq!(
        first.reference(hp2),
        Err(DefinitionTableError::ForeignTable)
    );
}

#[test]
fn deep_paths_are_rejected_before_inserting_additional_nodes() {
    let mut builder = DefinitionTableBuilder::new().unwrap();
    let mut id = builder
        .intern_root(&ModuleIdentity::single_file("game"))
        .unwrap();
    for _ in 0..MAX_IDENTITY_PATH_SEGMENTS {
        id = builder
            .intern_child(id, DefinitionKind::Module, "nested", 0)
            .unwrap();
    }
    assert_eq!(
        builder.resolve(id).unwrap().segments().count(),
        MAX_IDENTITY_PATH_SEGMENTS
    );
    let before = builder.freeze();
    assert_eq!(
        builder.intern_child(id, DefinitionKind::Field, "too_deep", 0),
        Err(DefinitionTableError::PathLimit)
    );
    assert_eq!(builder.freeze().len(), before.len());
}
