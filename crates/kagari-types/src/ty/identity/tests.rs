use super::*;
use crate::{
    scalar::BuiltinType,
    ty::{GenericParam, substitution::TypeSubstitution},
};
use bincode::{DefaultOptions, Options};
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, associated_type_id,
};
use std::collections::BTreeSet;

fn path(kind: DefinitionKind, name: &str) -> DefinitionPath {
    DefinitionPath {
        module: ModuleIdentity::single_file("types.kgr"),
        path: vec![DefinitionPathSegment {
            kind,
            name: name.into(),
            occurrence: 0,
        }],
    }
}

fn fixture(builder: &mut DefinitionTableBuilder) -> Vec<Ty> {
    let owner_path = path(DefinitionKind::Trait, "Collection");
    let owner = builder.intern_path(&owner_path).unwrap();
    let item = builder
        .intern_path(&associated_type_id(&owner_path, "Item"))
        .unwrap();
    let iter = builder
        .intern_path(&associated_type_id(&owner_path, "Iter"))
        .unwrap();
    let host = builder
        .intern_path(&path(DefinitionKind::Struct, "Host"))
        .unwrap();
    let function = builder
        .intern_path(&path(DefinitionKind::Function, "run"))
        .unwrap();
    let option = builder
        .intern_path(&crate::language::binding::option_declaration())
        .unwrap();
    let nominal = NominalTy {
        declaration: owner,
        arguments: vec![Ty::Parameter { owner, position: 0 }],
        associated_types: [
            (item, Ty::Builtin(BuiltinType::I32)),
            (iter, Ty::Iter(Box::new(Ty::Builtin(BuiltinType::I32)))),
        ]
        .into(),
    };
    let mut plain = nominal.clone();
    plain.associated_types.clear();
    vec![
        Ty::Trait(nominal.clone()),
        Ty::Projection {
            arguments: vec![Ty::Builtin(BuiltinType::Bool)],
            receiver: Box::new(Ty::SelfType(owner)),
            interface: Box::new(nominal),
            member: item,
        },
        Ty::Host(host),
        Ty::Struct(plain.clone()),
        Ty::Enum(plain.clone()),
        Ty::NativeObject(plain),
        Ty::Tuple(vec![
            Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))),
            Ty::NativeObject(NominalTy {
                declaration: owner,
                arguments: vec![
                    Ty::Builtin(BuiltinType::String),
                    Ty::NativeObject(NominalTy {
                        declaration: host,
                        arguments: vec![Ty::Builtin(BuiltinType::I64)],
                        associated_types: Default::default(),
                    }),
                ],
                associated_types: Default::default(),
            }),
        ]),
        Ty::Function {
            params: vec![Ty::Parameter {
                owner: function,
                position: 0,
            }],
            result: Box::new(Ty::Enum(crate::ty::NominalTy {
                declaration: option,
                arguments: vec![Ty::Builtin(BuiltinType::Bool)],
                associated_types: Default::default(),
            })),
        },
    ]
}

fn codec() -> impl Options {
    DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
}

#[test]
fn one_type_model_preserves_every_reference_across_portable_and_runtime_contexts() {
    let mut source = DefinitionTableBuilder::new().unwrap();
    let types = fixture(&mut source);
    let encoded = PortableTypes::encode(&source.freeze(), &types, &Default::default()).unwrap();
    let bytes = codec().serialize(&encoded).unwrap();
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    assert_eq!(
        encoded.clone().decode(&cancelled).unwrap_err(),
        IdentityTransformError::Type(TypeTransformError::Cancelled)
    );
    let decoded = codec()
        .deserialize::<PortableTypes>(&bytes)
        .unwrap()
        .decode(&Default::default())
        .unwrap();
    assert_eq!(
        decoded
            .import_into(&mut source, &Default::default())
            .unwrap(),
        types
    );
    assert_ne!(decoded.definitions.id(), source.freeze().id());
    let mut runtime_a = DefinitionTableBuilder::new().unwrap();
    let mut runtime_b = DefinitionTableBuilder::new().unwrap();
    let first = decoded
        .import_into(&mut runtime_a, &Default::default())
        .unwrap();
    let second = decoded
        .import_into(&mut runtime_b, &Default::default())
        .unwrap();
    for (definitions, imported) in [
        (runtime_a.freeze(), first.clone()),
        (runtime_b.freeze(), second.clone()),
    ] {
        assert_eq!(
            DecodedTypes {
                definitions,
                types: imported
            }
            .import_into(&mut source, &Default::default())
            .unwrap(),
            types
        );
    }
    assert_ne!(first, second);
    assert_eq!(
        decoded
            .import_into(&mut runtime_a, &Default::default())
            .unwrap(),
        first
    );
}

#[test]
fn canonical_bytes_ignore_process_scopes_insertion_order_and_unrelated_definitions() {
    let mut first = DefinitionTableBuilder::new().unwrap();
    let first_types = fixture(&mut first);
    let mut second = DefinitionTableBuilder::new().unwrap();
    second
        .intern_path(&path(DefinitionKind::Function, "unrelated"))
        .unwrap();
    // Reverse identity allocation too, including associated keys within one type.
    let mut references = BTreeSet::new();
    let first_table = first.freeze();
    for ty in &first_types {
        references.extend(
            ty.definition_references(&Default::default())
                .unwrap()
                .into_iter()
                .map(|id| first_table.resolve(*id).unwrap().to_path()),
        );
    }
    for path in references.into_iter().rev() {
        second.intern_path(&path).unwrap();
    }
    let second_types = fixture(&mut second);
    let encode = |builder: &DefinitionTableBuilder, types: &[Ty]| {
        codec()
            .serialize(
                &PortableTypes::encode(&builder.freeze(), types, &Default::default()).unwrap(),
            )
            .unwrap()
    };
    assert_ne!(first_types, second_types);
    assert_eq!(encode(&first, &first_types), encode(&second, &second_types));
}

#[test]
fn foreign_handles_and_out_of_range_wire_references_fail_before_adoption() {
    let mut source = DefinitionTableBuilder::new().unwrap();
    let host = source
        .intern_path(&path(DefinitionKind::Struct, "Host"))
        .unwrap();
    let types = vec![Ty::Host(host)];
    let other = DefinitionTableBuilder::new().unwrap();
    assert_eq!(
        PortableTypes::encode(&other.freeze(), &types, &Default::default()).unwrap_err(),
        IdentityTransformError::Identity(DefinitionTableError::ForeignTable)
    );
    let mut portable =
        PortableTypes::encode(&source.freeze(), &types, &Default::default()).unwrap();
    let mut deep = path(DefinitionKind::Struct, "Deep");
    deep.path.extend((0..20).map(|index| DefinitionPathSegment {
        kind: DefinitionKind::Field,
        name: index.to_string(),
        occurrence: 0,
    }));
    let id = source.intern_path(&deep).unwrap();
    let encoding = source.freeze().encode([id]).unwrap();
    portable.types[0] = WireType::encode(&Ty::Host(id), &encoding, &Default::default()).unwrap();
    let portable = codec()
        .deserialize::<PortableTypes>(&codec().serialize(&portable).unwrap())
        .unwrap();
    assert_eq!(
        portable.decode(&Default::default()).unwrap_err(),
        IdentityTransformError::Identity(DefinitionTableError::InvalidIndex)
    );
}

#[test]
fn identity_conversion_rejects_collapsed_associated_keys_and_honors_cancellation() {
    let mut target = DefinitionTableBuilder::new().unwrap();
    let source_types = fixture(&mut DefinitionTableBuilder::new().unwrap());
    let only = target
        .intern_path(&path(DefinitionKind::Trait, "Only"))
        .unwrap();
    assert_eq!(
        source_types[0]
            .map_definitions(&mut |_| Ok(only), &Default::default())
            .unwrap_err(),
        IdentityTransformError::Type(TypeTransformError::InvalidContract)
    );
    let cancel = CancellationToken::default();
    cancel.cancel();
    let mut visits = 0;
    assert_eq!(
        source_types[0]
            .map_definitions(
                &mut |_| {
                    visits += 1;
                    Ok(only)
                },
                &cancel
            )
            .unwrap_err(),
        IdentityTransformError::Type(TypeTransformError::Cancelled)
    );
    assert_eq!(visits, 0);
}

#[test]
fn scoped_binders_share_the_existing_checked_substitution_semantics() {
    assert_eq!(size_of::<GenericParam>(), 16);
    let mut definitions = DefinitionTableBuilder::new().unwrap();
    let owner = definitions
        .intern_path(&path(DefinitionKind::Function, "run"))
        .unwrap();
    let binder = GenericParam { owner, position: 0 };
    let arguments: [Ty; 1] = [Ty::Builtin(BuiltinType::I64)];
    let substitution = TypeSubstitution::for_owner(&owner, &arguments);
    assert_eq!(
        substitution
            .apply(&Ty::Tuple(vec![binder.as_type()]), &Default::default())
            .unwrap(),
        Ty::Tuple(vec![arguments[0].clone()])
    );
    let mut foreign = DefinitionTableBuilder::new().unwrap();
    let other_owner = foreign
        .intern_path(&path(DefinitionKind::Function, "run"))
        .unwrap();
    let other = GenericParam {
        owner: other_owner,
        position: 0,
    };
    assert_eq!(
        substitution
            .apply(&other.as_type(), &Default::default())
            .unwrap(),
        other.as_type()
    );
}
