use crate::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, MAX_IDENTITY_PATH_SEGMENTS,
    ModuleIdentity, PackageId,
    table::{
        DefinitionId, DefinitionIndex, DefinitionTableBuilder, DefinitionTableError, TableData,
        module::ModuleHandle,
    },
};
use std::{
    cmp::Ordering as Comparison,
    collections::HashMap,
    mem::size_of,
    sync::{Arc, Weak},
};

#[test]
fn module_roots_preserve_scope_prefixes_and_explicit_import() {
    let identity = ModuleIdentity {
        package: PackageId("game".into()),
        path: vec!["combat".into(), "damage".into()],
    };
    let mut builder = DefinitionTableBuilder::new().unwrap();
    let empty = builder.freeze();
    let module = builder.intern_module(&identity).unwrap();
    let function = builder
        .intern_child(module.definition(), DefinitionKind::Function, "run", 0)
        .unwrap();
    let table = builder.freeze();
    assert_eq!(table.module_identity(module).unwrap(), &identity);
    assert_eq!(table.lookup_module(&identity), Some(module));
    assert_eq!(table.module_of(function), Ok(module));
    assert_eq!(
        table.module_handle(function),
        Err(DefinitionTableError::InvalidModuleRoot)
    );
    assert_eq!(
        empty.module_identity(module),
        Err(DefinitionTableError::InvalidIndex)
    );
    let parent = table.module_parent(module).unwrap().unwrap();
    assert_eq!(table.module_identity(parent).unwrap().path, ["combat"]);
    assert_eq!(table.module_child(parent, "damage"), Some(module));
    assert_eq!(table.parent(module.definition()).unwrap(), None);
    let package = table.module_parent(parent).unwrap().unwrap();
    assert_eq!(table.module_parent(package).unwrap(), None);
    let mut other = DefinitionTableBuilder::new().unwrap();
    assert_eq!(
        other.freeze().module_identity(module),
        Err(DefinitionTableError::ForeignTable)
    );
    let mapping = other.import(&table, [function]).unwrap();
    let imported = mapping.map_module(module).unwrap();
    assert_ne!(imported, module);
    assert_eq!(other.freeze().module_identity(imported).unwrap(), &identity);
    assert_eq!(
        other
            .freeze()
            .module_of(mapping.map(function).unwrap())
            .unwrap(),
        imported
    );
    let encoded = table.encode([function]).unwrap();
    let encoded_import = other
        .freeze()
        .encode([mapping.map(function).unwrap()])
        .unwrap();
    assert_eq!(encoded.table, encoded_import.table);
    assert_eq!(size_of::<ModuleHandle>(), size_of::<DefinitionId>());
}

#[test]
fn borrowed_identity_queries_require_the_explicit_scope() {
    let expected = path("game", "hp");
    let mut builder = DefinitionTableBuilder::new().unwrap();
    let id = builder.intern_path(&expected).unwrap();
    let table = builder.freeze();
    let parent = table.parent(id).unwrap().unwrap();
    assert_eq!(
        table.lookup_child(parent, DefinitionKind::Field, "hp", 0),
        Some(id)
    );
    assert_eq!(
        table.lookup_child(parent, DefinitionKind::Field, "hp", 1),
        None
    );
    assert_eq!(table.lookup(&expected), Some(id));
    let view = table.resolve(id).unwrap();
    assert_eq!(view.module(), &expected.module);
    assert_eq!(view.last().unwrap().name, "hp");
    let root = table.parent(parent).unwrap().unwrap();
    assert!(table.resolve(root).unwrap().last().is_none());
    assert_eq!(table.lookup_root(&expected.module), Some(root));
    assert_eq!(
        table.lookup_root(&ModuleIdentity::single_file("absent")),
        None
    );
    assert_eq!(table.contains(root, id), Ok(true));
    assert_eq!(table.contains(id, id), Ok(true));
    assert_eq!(table.contains(id, root), Ok(false));
    assert_eq!(table.compare(root, id), Ok(Comparison::Less));
    let foreign = DefinitionTableBuilder::new().unwrap().freeze();
    assert!(matches!(
        foreign.resolve(id),
        Err(DefinitionTableError::ForeignTable)
    ));
    assert_eq!(
        foreign.contains(id, id),
        Err(DefinitionTableError::ForeignTable)
    );
    assert_eq!(
        foreign.compare(id, id),
        Err(DefinitionTableError::ForeignTable)
    );
    assert!(matches!(
        table.parent(DefinitionId {
            table: table.id(),
            index: DefinitionIndex(u32::MAX)
        }),
        Err(DefinitionTableError::InvalidIndex)
    ));
}

fn path(module: &str, name: &str) -> DefinitionPath {
    DefinitionPath {
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
    assert_eq!(size_of::<DefinitionId>(), 8);
    assert_eq!(size_of::<Option<DefinitionId>>(), 8);
    let mut table = DefinitionTableBuilder::new().unwrap();
    let expected = path("game", "hp");
    let hp = table.intern_path(&expected).unwrap();
    assert_eq!(hp, table.intern_path(&expected).unwrap());
    let mp = table.intern_path(&path("game", "mp")).unwrap();
    let frozen = table.freeze();
    assert_eq!(frozen.len(), 4);
    // The package root and the three declaration spellings are each interned once.
    assert_eq!(frozen.symbol_count(), 4);
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
    let absent = DefinitionId {
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
fn rl03_imported_snapshots_release_source_and_intermediate_backings() {
    let expected = path("game", "hp");
    let omitted = path("game", "mp");
    let mut source_builder = DefinitionTableBuilder::new().unwrap();
    let source_hp = source_builder.intern_path(&expected).unwrap();
    let source_mp = source_builder.intern_path(&omitted).unwrap();
    let source = source_builder.freeze();
    let source_control = source.clone();
    let source_backing = Arc::downgrade(&source.data);

    let mut a_builder = DefinitionTableBuilder::new().unwrap();
    a_builder.intern_path(&path("a_local", "noise")).unwrap();
    let source_to_a = a_builder.import(&source, [source_hp]).unwrap();
    let a_hp = source_to_a.map(source_hp).unwrap();
    assert_ne!(source_hp.index(), a_hp.index());
    assert_eq!(
        source_to_a.map(source_mp),
        Err(DefinitionTableError::UnmappedDefinition)
    );
    let a = a_builder.freeze();
    assert_eq!(a.lookup(&expected), Some(a_hp));
    assert_eq!(a.lookup(&omitted), None);
    assert_eq!(a.resolve(a_hp).unwrap().to_path(), expected);
    assert_eq!(
        a.resolve(source_hp).unwrap_err(),
        DefinitionTableError::ForeignTable
    );
    drop(source_builder);
    drop(source);
    assert!(source_backing.upgrade().is_some());
    assert_eq!(
        source_control.resolve(source_hp).unwrap().to_path(),
        expected
    );
    drop(source_control);
    assert!(source_backing.upgrade().is_none());

    // Keep the accepted prefix strong across append: a Weak alone could be
    // disassociated by make_mut without dropping the table's contents.
    let a_control = a.clone();
    let a_backing = Arc::downgrade(&a.data);
    let a_later = a_builder.intern_path(&path("game", "later")).unwrap();
    assert!(!Arc::ptr_eq(&a.data, &a_builder.data));
    assert_eq!(a.resolve(a_hp).unwrap().to_path(), expected);
    assert_eq!(
        a.resolve(a_later).unwrap_err(),
        DefinitionTableError::InvalidIndex
    );
    // This probe is captured after the writer's final mutation.
    let a_writer_backing = Arc::downgrade(&a_builder.data);
    drop(a_builder);
    assert!(a_writer_backing.upgrade().is_none());
    assert!(a_backing.upgrade().is_some());

    let mut b_builder = DefinitionTableBuilder::new().unwrap();
    b_builder.intern_path(&path("b_local", "one")).unwrap();
    b_builder.intern_path(&path("b_local", "two")).unwrap();
    let b_prefix = b_builder.freeze();
    assert_eq!(
        b_builder.import(&a, [source_hp]).unwrap_err(),
        DefinitionTableError::ForeignTable
    );
    assert_eq!(
        b_builder.import(&a, [a_later]).unwrap_err(),
        DefinitionTableError::InvalidIndex
    );
    let a_to_b = b_builder.import(&a, [a_hp]).unwrap();
    let b_hp = a_to_b.map(a_hp).unwrap();
    assert_ne!(a_hp.index(), b_hp.index());
    assert_eq!(
        a_to_b.map(source_hp),
        Err(DefinitionTableError::ForeignTable)
    );
    let b = b_builder.freeze();
    assert_eq!(
        b_prefix.resolve(b_hp).unwrap_err(),
        DefinitionTableError::InvalidIndex
    );
    assert_eq!(b.lookup(&expected), Some(b_hp));
    assert_eq!(b.lookup(&omitted), None);
    assert_eq!(b.lookup(&path("game", "later")), None);
    assert_eq!(b.lookup(&path("a_local", "noise")), None);
    let b_backing = Arc::downgrade(&b.data);
    drop(b_builder);
    drop(b_prefix);
    drop(a);
    assert!(a_backing.upgrade().is_some());
    assert_eq!(a_control.resolve(a_hp).unwrap().to_path(), expected);
    drop(a_control);
    assert!(a_backing.upgrade().is_none());
    assert!(source_backing.upgrade().is_none());
    assert_eq!(b.resolve(b_hp).unwrap().to_path(), expected);
    assert_eq!(
        b.resolve(a_hp).unwrap_err(),
        DefinitionTableError::ForeignTable
    );
    assert!(b_backing.upgrade().is_some());
    drop(b);
    assert!(b_backing.upgrade().is_none());
    // Remaps and IDs remain alive through all release assertions; neither owns
    // the source/current backing it names.
    assert_eq!(source_to_a.map(source_hp).unwrap(), a_hp);
    assert_eq!(a_to_b.map(a_hp).unwrap(), b_hp);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ContainerCapacity {
    name: &'static str,
    len: usize,
    capacity: usize,
    slot_bytes: usize,
}

impl ContainerCapacity {
    fn vector<T>(name: &'static str, values: &Vec<T>) -> Self {
        Self {
            name,
            len: values.len(),
            capacity: values.capacity(),
            slot_bytes: size_of::<T>(),
        }
    }

    fn map<K, V>(name: &'static str, values: &HashMap<K, V>) -> Self {
        Self {
            name,
            len: values.len(),
            capacity: values.capacity(),
            slot_bytes: size_of::<(K, V)>(),
        }
    }
}

fn table_capacities(data: &TableData) -> [ContainerCapacity; 9] {
    [
        ContainerCapacity::vector("modules", &data.modules),
        ContainerCapacity::vector("symbols", &data.symbols),
        ContainerCapacity::vector("nodes", &data.nodes),
        ContainerCapacity::vector("depths", &data.depths),
        ContainerCapacity::map("module_indices", &data.module_indices),
        ContainerCapacity::map("package_roots", &data.package_roots),
        ContainerCapacity::map("module_children", &data.module_children),
        ContainerCapacity::map("symbol_indices", &data.symbol_indices),
        ContainerCapacity::map("node_indices", &data.node_indices),
    ]
}

fn rl03_probe_snapshot_capacity(cycles: usize, distinct: bool, retain: bool) {
    let mut builder = DefinitionTableBuilder::new().unwrap();
    // A two-component module exercises both module lookup maps. The seed makes
    // the repeated-identity control mutation-free, including the first cycle.
    let seed = DefinitionPath {
        module: ModuleIdentity {
            package: PackageId("probe".into()),
            path: vec!["nested".into()],
        },
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Struct,
            name: "Type0".into(),
            occurrence: 0,
        }],
    };
    let seed_id = builder.intern_path(&seed).unwrap();
    let baseline = table_capacities(&builder.data);
    let mut retained = Vec::new();
    let mut copied_backings = 0;
    for cycle in 1..=cycles {
        let before = builder.freeze();
        assert!(Arc::ptr_eq(&before.data, &builder.data));
        if retain {
            retained.push(before);
        } else {
            drop(before);
            assert_eq!(Arc::strong_count(&builder.data), 1);
            assert_eq!(Arc::weak_count(&builder.data), 0);
        }
        let mut next = seed.clone();
        if distinct {
            next.path[0].name = format!("Type{cycle}");
        }
        let id = builder.intern_path(&next).unwrap();
        assert_eq!(builder.resolve(id).unwrap().to_path(), next);
        if let Some(before) = retained.last() {
            let shared = Arc::ptr_eq(&before.data, &builder.data);
            assert_eq!(shared, !distinct);
            copied_backings += usize::from(!shared);
            assert_eq!(before.resolve(seed_id).unwrap().to_path(), seed);
            if distinct {
                assert_eq!(
                    before.resolve(id).unwrap_err(),
                    DefinitionTableError::InvalidIndex
                );
            } else {
                assert_eq!(id, seed_id);
            }
        }
    }
    let current = builder.freeze();
    let current_capacity = table_capacities(&current.data);
    assert_eq!(current.len(), 3 + if distinct { cycles } else { 0 });
    assert_eq!(current.symbol_count(), current.len());
    if !distinct {
        assert_eq!(current_capacity, baseline);
    }
    assert_eq!(copied_backings, if distinct && retain { cycles } else { 0 });

    // Count each backing only once, not each snapshot handle. These references
    // do not add strong owners; all Weak probes are made after final mutation.
    let mut old_unique: Vec<&Arc<TableData>> = Vec::new();
    let mut shared_with_current = 0;
    let mut old_capacity = baseline;
    for entry in &mut old_capacity {
        entry.len = 0;
        entry.capacity = 0;
    }
    for snapshot in &retained {
        if Arc::ptr_eq(&snapshot.data, &current.data) {
            shared_with_current += 1;
        } else if !old_unique
            .iter()
            .any(|old| Arc::ptr_eq(old, &snapshot.data))
        {
            old_unique.push(&snapshot.data);
            for (total, sample) in old_capacity
                .iter_mut()
                .zip(table_capacities(&snapshot.data))
            {
                total.len += sample.len;
                total.capacity += sample.capacity;
            }
            // COW duplicates the containers, not the shared identity/string
            // payload allocations already present in their immutable prefix.
            assert!(Arc::ptr_eq(
                &snapshot.data.modules[0],
                &current.data.modules[0]
            ));
            assert!(Arc::ptr_eq(
                &snapshot.data.symbols[0],
                &current.data.symbols[0]
            ));
        }
    }
    assert_eq!(old_unique.len(), copied_backings);
    assert_eq!(
        shared_with_current,
        if retain && !distinct { cycles } else { 0 }
    );
    let old_backings: Vec<Weak<TableData>> =
        old_unique.iter().map(|data| Arc::downgrade(data)).collect();
    drop(old_unique);
    let current_backing = Arc::downgrade(&current.data);
    let old_vector_slot_bytes: usize = old_capacity[..4]
        .iter()
        .map(|entry| entry.capacity * entry.slot_bytes)
        .sum();
    let old_map_entry_slot_bytes: usize = old_capacity[4..]
        .iter()
        .map(|entry| entry.capacity * entry.slot_bytes)
        .sum();
    // Gross extra capacity alongside current, NOT allocator bytes, transient
    // append peak or RSS. Map capacity omits control/bucket overhead; all counts
    // omit nested payloads, Arc headers and the probe's own bookkeeping.
    println!(
        "RL03 cycles={cycles} distinct={distinct} retain={retain} \
         copied_backings={copied_backings} shared_with_current={shared_with_current} \
         old_unique_backings={} old_vector_slot_bytes={old_vector_slot_bytes} \
         old_map_entry_slot_bytes={old_map_entry_slot_bytes} \
         current={current_capacity:?} old_unique_sum={old_capacity:?}",
        old_backings.len()
    );

    drop(builder);
    assert!(current_backing.upgrade().is_some());
    assert!(
        old_backings
            .iter()
            .all(|backing| backing.upgrade().is_some())
    );
    drop(current);
    assert_eq!(current_backing.upgrade().is_some(), retain && !distinct);
    drop(retained);
    assert!(current_backing.upgrade().is_none());
    assert!(
        old_backings
            .iter()
            .all(|backing| backing.upgrade().is_none())
    );
}

#[test]
fn rl03_snapshot_capacity_matrix() {
    for cycles in [1, 16, 128] {
        for distinct in [false, true] {
            for retain in [false, true] {
                rl03_probe_snapshot_capacity(cycles, distinct, retain);
            }
        }
    }
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
