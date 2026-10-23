use super::*;
use bincode::Options;
use kagari_common::identity::{
    DefinitionKind,
    map::DefinitionContext,
    mapping::DefinitionMapper,
    scoped::{DefinitionReferences, RemapDefinitions},
    table::{DefinitionId, DefinitionTable, DefinitionTableError},
};
use kagari_types::{
    decode_limits::MAX_NESTED_RECORDS,
    scalar::BuiltinType,
    ty::{
        NominalTy, Ty,
        substitution::{MAX_TYPE_DEPTH, MAX_TYPE_NODES},
    },
};
use std::collections::{BTreeMap, HashSet};

#[test]
fn wire_rejects_duplicate_export_records_and_foreign_references() {
    let context = DefinitionContext::new().unwrap();
    let root = ModuleIdentity::single_file("exports");
    let declaration = context
        .intern_child(
            context.intern_root(&root).unwrap(),
            DefinitionKind::Function,
            "f",
            0,
        )
        .unwrap();
    let encoding = context.snapshot().encode([declaration]).unwrap();
    let decoding = encoding.table.clone().decode().unwrap();
    let reference = encoding.reference(declaration).unwrap();
    for corruption in 0..8 {
        let namespace = || NamespaceRecord {
            identity: root.clone(),
            modules: vec![],
            functions: vec![("f".into(), reference)],
            types: vec![("T".into(), TypeExportRecord::OpenAlias { arity: 1 })],
        };
        let mut record = ProgramExportsRecord {
            namespaces: vec![namespace()],
            entry: None,
        };
        match corruption {
            0 => record.namespaces.push(namespace()),
            1 => record.namespaces[0].functions.push(("f".into(), reference)),
            2 => record.namespaces[0].modules = vec![("same".into(), root.clone()); 2],
            3 => {
                record.entry = Some(bincode::deserialize(&u32::MAX.to_le_bytes()).unwrap());
            }
            4 => record.namespaces[0]
                .types
                .push(("T".into(), TypeExportRecord::OpenAlias { arity: 2 })),
            5 => record.namespaces[0]
                .modules
                .push(("T".into(), root.clone())),
            6 => {
                record.namespaces[0].types[0].1 = TypeExportRecord::Declaration(
                    bincode::deserialize(&u32::MAX.to_le_bytes()).unwrap(),
                )
            }
            7 => record.namespaces[0].types[0].0 = "invalid::leaf".into(),
            _ => unreachable!(),
        }
        let bytes = bincode::serialize(&record).unwrap();
        let decoded: ProgramExportsRecord = bincode::deserialize(&bytes).unwrap();
        assert!(
            decoded.decode(&decoding, &Default::default()).is_err(),
            "corruption {corruption}"
        );
    }
    // Reject an oversized namespace sequence before reading/allocating its elements.
    let error = bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .deserialize::<ProgramExportsRecord>(&((MAX_EXPORT_RECORDS + 1) as u64).to_le_bytes())
        .err()
        .unwrap();
    assert!(error.to_string().contains("export record"), "{error}");
}

fn graph(root: &ModuleIdentity, export: TypeExport) -> ProgramExports {
    ProgramExports {
        namespaces: BTreeMap::from([(
            root.clone(),
            ExportNamespace {
                types: BTreeMap::from([("T".into(), export)]),
                ..Default::default()
            },
        )]),
        entry: None,
    }
}

fn nominal(declaration: DefinitionId) -> NominalTy {
    NominalTy {
        declaration,
        arguments: vec![],
        associated_types: BTreeMap::new(),
    }
}

fn references(exports: &ProgramExports) -> HashSet<DefinitionId> {
    let mut ids = HashSet::new();
    exports
        .visit_definition_ids(
            &mut |id| {
                ids.insert(*id);
                Ok(())
            },
            &Default::default(),
        )
        .unwrap();
    ids
}

fn encoded(exports: &ProgramExports, table: &DefinitionTable) -> (DefinitionEncoding, Vec<u8>) {
    let encoding = table.encode(references(exports)).unwrap();
    let record = ProgramExportsRecord::encode(exports, &encoding, &Default::default()).unwrap();
    (encoding, bincode::serialize(&record).unwrap())
}

#[test]
fn type_exports_round_trip_remap_and_encode_canonically() {
    let root = ModuleIdentity::single_file("exports");
    let context = DefinitionContext::new().unwrap();
    let module = context.intern_root(&root).unwrap();
    let cell = context
        .intern_child(module, DefinitionKind::Struct, "Cell", 0)
        .unwrap();
    let interface = context
        .intern_child(module, DefinitionKind::Trait, "View", 0)
        .unwrap();
    let first = context
        .intern_child(interface, DefinitionKind::AssociatedType, "A", 0)
        .unwrap();
    let second = context
        .intern_child(interface, DefinitionKind::AssociatedType, "Z", 0)
        .unwrap();
    let native = context
        .intern_child(module, DefinitionKind::AssociatedType, "Native", 0)
        .unwrap();
    let function = context
        .intern_child(module, DefinitionKind::Function, "f", 0)
        .unwrap();
    let int = Ty::Builtin(BuiltinType::I32);
    let mut exports = graph(
        &root,
        TypeExport::Closed(Ty::Trait(NominalTy {
            declaration: interface,
            arguments: vec![Ty::Struct(NominalTy {
                declaration: cell,
                arguments: vec![int.clone()],
                associated_types: BTreeMap::new(),
            })],
            associated_types: BTreeMap::from([
                (first, int),
                (second, Ty::NativeObject(nominal(native))),
            ]),
        })),
    );
    let namespace = exports.namespaces.get_mut(&root).unwrap();
    namespace
        .types
        .insert("Cell".into(), TypeExport::Declaration(cell));
    namespace.types.insert(
        "Open".into(),
        TypeExport::OpenAlias {
            arity: MAX_NESTED_RECORDS as u32,
        },
    );
    // Function names are independent of type/module names, including cyclic aliases.
    namespace.functions.insert("T".into(), function);
    namespace.functions.insert("again".into(), function);
    namespace.modules.insert("again".into(), root.clone());
    let source = context.snapshot();
    assert_eq!(
        references(&exports),
        HashSet::from([cell, interface, first, second, native, function])
    );
    assert_eq!(exports.function(&root, "again::T"), Some(function));
    exports
        .validate(&root, &source, &Default::default(), |_, _| true, |_| true)
        .unwrap();
    let (encoding, bytes) = encoded(&exports, &source);
    let decoding = encoding.table.clone().decode().unwrap();
    let decoded = bincode::deserialize::<ProgramExportsRecord>(&bytes)
        .unwrap()
        .decode(&decoding, &Default::default())
        .unwrap();
    let (decoded_encoding, decoded_bytes) = encoded(&decoded, &decoding.table);
    assert_eq!(encoding.table, decoded_encoding.table);
    assert_eq!(bytes, decoded_bytes);

    let destination = DefinitionContext::new().unwrap();
    // Reverse allocation, including associated binding keys, must not affect bytes.
    for id in [function, native, second, first, cell, interface] {
        destination
            .intern(&source.resolve(id).unwrap().to_path())
            .unwrap();
    }
    let target = destination.snapshot();
    let mut convert = |id: &DefinitionId| Ok(destination.intern(&source.resolve(*id)?.to_path())?);
    let cancel = CancellationToken::default();
    let remapped = exports
        .remap_definitions(&mut DefinitionMapper::new(&mut convert, &target, &cancel))
        .unwrap();
    assert!(
        references(&remapped)
            .iter()
            .all(|id| target.resolve(*id).is_ok() && source.resolve(*id).is_err())
    );
    let (remapped_encoding, remapped_bytes) = encoded(&remapped, &target);
    assert_eq!(encoding.table, remapped_encoding.table);
    assert_eq!(bytes, remapped_bytes);
    assert!(ProgramExportsRecord::encode(&exports, &remapped_encoding, &cancel).is_err());
    let empty_decoding = source.encode([]).unwrap().table.decode().unwrap();
    let nested_reference = TypeExportRecord::encode(
        exports.namespaces[&root].types.get("T").unwrap(),
        &encoding,
        &cancel,
    )
    .unwrap();
    assert!(matches!(
        nested_reference.decode(&empty_decoding, &cancel),
        Err(DefinitionMappingError::Identity(_))
    ));
    // The converter owns scope validation; an unchecked identity closure would
    // bypass that contract rather than exercise rejection of a foreign target.
    for rejected in [cell, interface, first, second, native, function] {
        assert_eq!(
            target.resolve(rejected).err(),
            Some(DefinitionTableError::ForeignTable)
        );
        let mut reached = false;
        let mut foreign = |id: &DefinitionId| {
            let path = source.resolve(*id)?.to_path();
            let mapped = if *id == rejected {
                reached = true;
                *id
            } else {
                destination.intern(&path)?
            };
            target.resolve(mapped)?;
            Ok(mapped)
        };
        assert_eq!(
            exports.remap_definitions(&mut DefinitionMapper::new(&mut foreign, &target, &cancel)),
            Err(DefinitionMappingError::Identity(
                DefinitionTableError::ForeignTable
            ))
        );
        assert!(reached);
    }
}

#[test]
fn type_leaf_validation_precedes_backend_authority() {
    let root = ModuleIdentity::single_file("exports");
    let context = DefinitionContext::new().unwrap();
    let module = context.intern_root(&root).unwrap();
    let cell = context
        .intern_child(module, DefinitionKind::Struct, "Cell", 0)
        .unwrap();
    let interface = context
        .intern_child(module, DefinitionKind::Trait, "View", 0)
        .unwrap();
    let member = context
        .intern_child(interface, DefinitionKind::AssociatedType, "Item", 0)
        .unwrap();
    let function = context
        .intern_child(module, DefinitionKind::Function, "f", 0)
        .unwrap();
    let duplicate = context
        .intern_child(module, DefinitionKind::Struct, "Cell", 1)
        .unwrap();
    let nested = context
        .intern_child(cell, DefinitionKind::Struct, "Nested", 0)
        .unwrap();
    let table = context.snapshot();
    let foreign = DefinitionContext::new()
        .unwrap()
        .intern(&table.resolve(cell).unwrap().to_path())
        .unwrap();
    let invalid = [
        TypeExport::Declaration(function),
        TypeExport::Declaration(module),
        TypeExport::Declaration(duplicate),
        TypeExport::Declaration(nested),
        TypeExport::Declaration(member),
        TypeExport::Declaration(foreign),
        TypeExport::OpenAlias { arity: 0 },
        TypeExport::OpenAlias {
            arity: MAX_NESTED_RECORDS as u32 + 1,
        },
        TypeExport::Closed(Ty::Tuple(vec![Ty::Parameter {
            owner: cell,
            position: 0,
        }])),
        TypeExport::Closed(Ty::SelfType(interface)),
        TypeExport::Closed(Ty::Projection {
            receiver: Box::new(Ty::Struct(nominal(cell))),
            interface: Box::new(nominal(interface)),
            member,
            arguments: vec![],
        }),
        TypeExport::Closed(Ty::Enum(nominal(cell))),
        TypeExport::Closed(Ty::Struct(nominal(foreign))),
        TypeExport::Closed(Ty::Trait(NominalTy {
            declaration: interface,
            arguments: vec![],
            associated_types: BTreeMap::from([(cell, Ty::Builtin(BuiltinType::I32))]),
        })),
        TypeExport::Closed(Ty::Struct(NominalTy {
            declaration: cell,
            arguments: vec![],
            associated_types: BTreeMap::from([(member, Ty::Builtin(BuiltinType::I32))]),
        })),
    ];
    for export in invalid {
        let exports = graph(&root, export);
        assert!(
            exports
                .validate(
                    &root,
                    &table,
                    &Default::default(),
                    |_, _| panic!("no functions"),
                    |_| panic!("invalid leaf reached authority callback")
                )
                .is_err()
        );
    }

    for kind in [
        DefinitionKind::Struct,
        DefinitionKind::Enum,
        DefinitionKind::Trait,
        DefinitionKind::AssociatedType,
    ] {
        let id = context.intern_child(module, kind, "Public", 0).unwrap();
        let table = context.snapshot();
        let export = TypeExport::Declaration(id);
        let exports = graph(&root, export.clone());
        let mut seen = vec![];
        exports
            .validate(
                &root,
                &table,
                &Default::default(),
                |_, _| false,
                |leaf| {
                    seen.push(leaf.clone());
                    true
                },
            )
            .unwrap();
        assert_eq!(seen, vec![export]);
        assert_eq!(
            exports.validate(&root, &table, &Default::default(), |_, _| true, |_| false),
            Err(DefinitionMappingError::InvalidContract)
        );
    }
    for export in [
        TypeExport::Closed(Ty::Struct(nominal(cell))),
        TypeExport::Closed(Ty::Builtin(BuiltinType::I32)),
        TypeExport::OpenAlias { arity: 1 },
    ] {
        let exports = graph(&root, export.clone());
        exports
            .validate(
                &root,
                &table,
                &Default::default(),
                |_, _| false,
                |leaf| leaf == &export,
            )
            .unwrap();
        assert_eq!(
            exports.validate(&root, &table, &Default::default(), |_, _| true, |_| false),
            Err(DefinitionMappingError::InvalidContract)
        );
    }
}

#[test]
fn type_export_wire_bounds_and_cancellation() {
    let root = ModuleIdentity::single_file("exports");
    let table = DefinitionContext::new().unwrap().snapshot();
    let encoding = table.encode([]).unwrap();
    let decoding = encoding.table.clone().decode().unwrap();
    for arity in [0, MAX_NESTED_RECORDS as u32 + 1, u32::MAX] {
        let record = ProgramExportsRecord {
            namespaces: vec![NamespaceRecord {
                identity: root.clone(),
                modules: vec![],
                functions: vec![],
                types: vec![("T".into(), TypeExportRecord::OpenAlias { arity })],
            }],
            entry: None,
        };
        assert_eq!(
            record.decode(&decoding, &Default::default()),
            Err(DefinitionMappingError::InvalidContract)
        );
    }
    assert!(bincode::deserialize::<TypeExportRecord>(&3u32.to_le_bytes()).is_err());
    let mut oversized_type = 1u32.to_le_bytes().to_vec(); // Closed tag followed by flat node count.
    oversized_type.extend_from_slice(&((MAX_NESTED_RECORDS + 1) as u64).to_le_bytes());
    let error = bincode::deserialize::<TypeExportRecord>(&oversized_type)
        .err()
        .unwrap();
    assert!(error.to_string().contains("nested declaration"), "{error}");

    let mut too_deep = Ty::Builtin(BuiltinType::I32);
    for _ in 0..MAX_TYPE_DEPTH {
        too_deep = Ty::Array(Box::new(too_deep));
    }
    for ty in [
        too_deep,
        Ty::Tuple(vec![Ty::Builtin(BuiltinType::I32); MAX_TYPE_NODES]),
    ] {
        let exports = graph(&root, TypeExport::Closed(ty));
        assert!(matches!(
            ProgramExportsRecord::encode(&exports, &encoding, &Default::default()),
            Err(DefinitionMappingError::LimitExceeded)
        ));
        assert_eq!(
            exports.visit_definition_ids(&mut |_| Ok(()), &Default::default()),
            Err(DefinitionMappingError::LimitExceeded)
        );
    }

    let mut exports = graph(&root, TypeExport::OpenAlias { arity: 1 });
    exports.namespaces.get_mut(&root).unwrap().types = (0..MAX_EXPORT_RECORDS)
        .map(|index| (format!("T{index}"), TypeExport::OpenAlias { arity: 1 }))
        .collect();
    assert!(matches!(
        ProgramExportsRecord::encode(&exports, &encoding, &Default::default()),
        Err(DefinitionMappingError::LimitExceeded)
    ));
    let record = ProgramExportsRecord {
        namespaces: vec![NamespaceRecord {
            identity: root.clone(),
            modules: vec![],
            functions: vec![],
            types: (0..MAX_EXPORT_RECORDS)
                .map(|index| {
                    (
                        format!("T{index}"),
                        TypeExportRecord::OpenAlias { arity: 1 },
                    )
                })
                .collect(),
        }],
        entry: None,
    };
    assert_eq!(
        record.decode(&decoding, &Default::default()),
        Err(DefinitionMappingError::LimitExceeded)
    );

    let exports = graph(&root, TypeExport::Closed(Ty::Builtin(BuiltinType::I32)));
    let record = ProgramExportsRecord::encode(&exports, &encoding, &Default::default()).unwrap();
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert_eq!(
        exports.validate(&root, &table, &cancel, |_, _| true, |_| true),
        Err(DefinitionMappingError::Cancelled)
    );
    assert_eq!(
        exports.visit_definition_ids(&mut |_| Ok(()), &cancel),
        Err(DefinitionMappingError::Cancelled)
    );
    assert!(matches!(
        ProgramExportsRecord::encode(&exports, &encoding, &cancel),
        Err(DefinitionMappingError::Cancelled)
    ));
    assert_eq!(
        record.decode(&decoding, &cancel),
        Err(DefinitionMappingError::Cancelled)
    );
}
