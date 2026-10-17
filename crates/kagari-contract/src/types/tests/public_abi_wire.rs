//! Public ABI codec ownership, canonical references and untrusted file limits.
use crate::types::{InterfaceTable, PublicItem, inherent::InherentTable, wire::PublicItemRecord};
use bincode::{DefaultOptions, Options};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, ModuleIdentity, map::DefinitionContext, mapping::DefinitionMappingError,
        metadata::DefinitionMetadata, scoped::DefinitionReferences,
        table::wire::PortableDefinitionTable,
    },
};
use kagari_types::{
    callable::{CallableImplementation, MethodPolicy, NativeDefaultApplication},
    declaration::{
        AssociatedConstDef, AssociatedTypeDef, AssociatedTypeFamily, ConstDef, FieldDef, FnDecl,
        Param, TraitDef, TypeDef, TypeDefKind, VariantDef, conversion::ConversionAdapter,
        host::HostCallPolicy,
    },
    scalar::BuiltinType,
    ty::{GenericBound, GenericParam, NominalTy, Ty},
    visibility::Visibility,
};
use std::collections::{BTreeMap, HashSet};

fn codec() -> impl Options {
    DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .reject_trailing_bytes()
}

fn items(context: &DefinitionContext, reverse: bool) -> Vec<PublicItem> {
    let root = context
        .intern_root(&ModuleIdentity::single_file("abi.kgr"))
        .unwrap();
    let mut declarations = vec![
        (DefinitionKind::Trait, "View"),
        (DefinitionKind::Trait, "Parent"),
        (DefinitionKind::Trait, "Key"),
        (DefinitionKind::Function, "factory"),
        (DefinitionKind::Impl, "Impl"),
    ];
    if reverse {
        declarations.reverse();
    }
    let ids = declarations
        .into_iter()
        .map(|(kind, name)| (name, context.intern_child(root, kind, name, 0).unwrap()))
        .collect::<BTreeMap<_, _>>();
    let view = ids["View"];
    let parent = ids["Parent"];
    let mut members = vec!["Input", "Output"];
    if reverse {
        members.reverse();
    }
    let outputs = members
        .into_iter()
        .map(|name| {
            (
                name,
                context
                    .intern_child(parent, DefinitionKind::AssociatedType, name, 0)
                    .unwrap(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let method = context
        .intern_child(view, DefinitionKind::Method, "read", 0)
        .unwrap();
    let item = context
        .intern_child(view, DefinitionKind::AssociatedType, "Item", 0)
        .unwrap();
    let constant = context
        .intern_child(view, DefinitionKind::Const, "DEFAULT", 0)
        .unwrap();
    let integer = Ty::Builtin(BuiltinType::I32);
    let text = Ty::Builtin(BuiltinType::String);
    let nominal = NominalTy {
        declaration: parent,
        arguments: vec![integer.clone()],
        associated_types: BTreeMap::from([
            (outputs["Input"], integer.clone()),
            (outputs["Output"], text.clone()),
        ]),
    };
    let parameter = GenericParam {
        owner: method,
        position: 0,
    };
    let bound = GenericBound {
        ty: Ty::Parameter {
            owner: method,
            position: 0,
        },
        constraints: vec![
            NominalTy {
                declaration: ids["Key"],
                arguments: vec![],
                associated_types: BTreeMap::new(),
            },
            nominal.clone(),
        ],
    };
    let function = FnDecl {
        host_policy: None,
        method_policy: Default::default(),
        name: "read".into(),
        implementation: CallableImplementation::NativeDefault(NativeDefaultApplication {
            declaration: ids["factory"],
            arguments: vec![Ty::Trait(nominal.clone())],
        }),
        generic_params: vec![parameter.clone()],
        bounds: vec![bound.clone()],
        params: vec![Param {
            name: "value".into(),
            ty: integer.clone(),
            mutable: false,
        }],
        return_type: Ty::Trait(nominal.clone()),
    };
    let associated = AssociatedTypeDef {
        generic_params: vec![],
        parameter_bounds: vec![],
        declaration: item,
        bounds: vec![nominal.clone()],
    };
    let trait_def = TraitDef {
        associated_consts: vec![AssociatedConstDef {
            declaration: constant,
            ty: integer.clone(),
            default_value: Some("7".into()),
        }],
        name: "View".into(),
        supertraits: vec![nominal],
        generic_params: vec![],
        bounds: vec![],
        methods: vec![function.clone()],
        associated_types: vec![associated],
        conversion_adapter: Some(ConversionAdapter::Reverse {
            origin: parent,
            method,
            error: Some((outputs["Input"], item)),
            result: Some(view),
        }),
    };
    let value = ConstDef {
        name: "DEFAULT".into(),
        ty: integer.clone(),
        value: "7".into(),
    };
    vec![
        PublicItem::Function(function.clone()),
        PublicItem::Const(value.clone()),
        PublicItem::Type(TypeDef {
            name: "Data".into(),
            kind: TypeDefKind::Enum,
            generic_params: vec![],
            bounds: vec![],
            fields: vec![],
            variants: vec![VariantDef {
                reports_failure: true,
                name: "Value".into(),
                payload: vec![integer.clone()],
            }],
        }),
        PublicItem::Type(TypeDef {
            name: "State".into(),
            kind: TypeDefKind::Struct,
            generic_params: vec![],
            bounds: vec![],
            fields: vec![FieldDef {
                host_policy: None,
                visibility: Visibility::Public,
                name: "key".into(),
                ty: text,
                mutable: false,
            }],
            variants: vec![],
        }),
        PublicItem::Trait(trait_def),
        PublicItem::InterfaceTable(Box::new(InterfaceTable {
            associated_type_families: vec![AssociatedTypeFamily {
                declaration: item,
                generic_params: vec![],
                bounds: vec![],
                value: integer.clone(),
            }],
            associated_consts: vec![value],
            bridge: None,
            declaration: ids["Impl"],
            name: "Impl".into(),
            generic_params: vec![parameter.clone()],
            bounds: vec![bound.clone()],
            trait_type: Ty::Trait(NominalTy {
                declaration: view,
                arguments: vec![],
                associated_types: BTreeMap::new(),
            }),
            for_type: integer.clone(),
            methods: vec![function.clone()],
        })),
        PublicItem::InherentTable(Box::new(InherentTable {
            declaration: ids["Impl"],
            name: "Impl".into(),
            generic_params: vec![parameter],
            bounds: vec![bound],
            for_type: integer,
            methods: vec![function],
        })),
    ]
}

fn encode(item: &PublicItem, context: &DefinitionContext) -> Vec<u8> {
    let cancel = CancellationToken::default();
    let mut ids = HashSet::new();
    item.visit_definition_ids(
        &mut |id| {
            ids.insert(*id);
            Ok(())
        },
        &cancel,
    )
    .unwrap();
    let encoding = context.snapshot().encode(ids).unwrap();
    let record = PublicItemRecord::encode(item, &encoding, &cancel).unwrap();
    codec().serialize(&(record, encoding.table)).unwrap()
}

#[test]
fn public_abi_records_roundtrip_independent_scopes_and_preserve_binding_pairs() {
    let context = DefinitionContext::new().unwrap();
    let independent = DefinitionContext::new().unwrap();
    independent
        .intern_root(&ModuleIdentity::single_file("unrelated"))
        .unwrap();
    let values = items(&context, false);
    let reordered = items(&independent, true);
    let cancel = CancellationToken::default();
    // Reference-free constants may use any empty file table. Exercise a record
    // with an actual NativeDefault/binder reference at this scope boundary.
    let foreign = independent.snapshot().encode([]).unwrap();
    assert!(PublicItemRecord::encode(&values[0], &foreign, &cancel).is_err());
    for (item, other) in values.iter().zip(&reordered) {
        let bytes = encode(item, &context);
        assert_eq!(bytes, encode(other, &independent));
        let (wire, table): (PublicItemRecord, PortableDefinitionTable) =
            codec().deserialize(&bytes).unwrap();
        let decoding = table.decode().unwrap();
        let value = wire.decode(&decoding, &cancel).unwrap();
        let scoped = DefinitionMetadata::checked(decoding.table, value, &cancel).unwrap();
        let imported = scoped.import_into(&context, &cancel).unwrap();
        assert_eq!(imported.records(), item);
        let (wire, table): (PublicItemRecord, PortableDefinitionTable) =
            codec().deserialize(&bytes).unwrap();
        let cancelled = CancellationToken::default();
        cancelled.cancel();
        assert!(matches!(
            wire.decode(&table.decode().unwrap(), &cancelled),
            Err(DefinitionMappingError::Cancelled)
        ));
    }
}

#[test]
fn public_abi_reader_bounds_declarations_before_reading_elements() {
    // Function tag, absent host policy, full method policy, and name. Only the
    // following vector count is malformed, so rejection must reach its reader.
    let header = (
        0u32,
        None::<HostCallPolicy>,
        MethodPolicy::default(),
        "read",
    );
    let generics = codec().serialize(&(&header, 0u32, u64::MAX)).unwrap();
    // The same reader owns NativeDefault arguments: implementation tag 3,
    // file-local declaration reference, then the oversized argument count.
    let default_arguments = codec().serialize(&(&header, 3u32, 0u32, 4097u64)).unwrap();
    for bytes in [generics, default_arguments] {
        let error = match codec().deserialize::<PublicItemRecord>(&bytes) {
            Ok(_) => panic!("accepted an unbounded signature vector"),
            Err(error) => error,
        };
        assert!(
            error
                .to_string()
                .contains("nested declaration count limit exceeded"),
            "{error}"
        );
    }
}
