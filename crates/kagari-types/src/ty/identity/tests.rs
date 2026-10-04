use super::*;
use crate::{
    collection::CollectionAccess,
    scalar::BuiltinType,
    surface::StandardEnum,
    ty::{GenericParam, substitution::TypeSubstitution},
};
use bincode::{DefaultOptions, Options};
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, associated_type_id,
};

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

fn fixture() -> Vec<Ty> {
    let owner = path(DefinitionKind::Trait, "Collection");
    let nominal = NominalTy {
        declaration: owner.clone(),
        arguments: vec![Ty::Parameter {
            owner: owner.clone(),
            position: 0,
        }],
        associated_types: [
            (
                associated_type_id(&owner, "Item"),
                Ty::Builtin(BuiltinType::I32),
            ),
            (
                associated_type_id(&owner, "Iter"),
                Ty::Iter(Box::new(Ty::Builtin(BuiltinType::I32))),
            ),
        ]
        .into(),
    };
    let mut plain = nominal.clone();
    plain.associated_types.clear();
    vec![
        Ty::Trait(nominal.clone()),
        Ty::Projection {
            arguments: vec![Ty::Builtin(BuiltinType::Bool)],
            receiver: Box::new(Ty::SelfType(owner.clone())),
            interface: Box::new(nominal),
            member: associated_type_id(&owner, "Item"),
        },
        Ty::Host(path(DefinitionKind::Struct, "Host")),
        Ty::Struct(plain.clone()),
        Ty::Enum(plain.clone()),
        Ty::NativeObject(plain),
        Ty::Tuple(vec![
            Ty::Array(
                Box::new(Ty::Builtin(BuiltinType::I32)),
                CollectionAccess::ReadOnly,
            ),
            Ty::Map {
                key: Box::new(Ty::Builtin(BuiltinType::String)),
                value: Box::new(Ty::Set(
                    Box::new(Ty::Builtin(BuiltinType::I64)),
                    CollectionAccess::Mutable,
                )),
                access: CollectionAccess::Mutable,
            },
        ]),
        Ty::Function {
            params: vec![Ty::Parameter {
                owner: path(DefinitionKind::Function, "run"),
                position: 0,
            }],
            result: Box::new(Ty::StandardEnum {
                kind: StandardEnum::Option,
                args: vec![Ty::Builtin(BuiltinType::Bool)],
            }),
        },
    ]
}

fn intern(builder: &mut DefinitionTableBuilder, types: &[Ty]) -> Vec<Ty<DefinitionId>> {
    types
        .iter()
        .map(|ty| {
            ty.map_definitions(
                &mut |path| builder.intern_path(path).map_err(Into::into),
                &Default::default(),
            )
            .unwrap()
        })
        .collect()
}

fn paths(definitions: &DefinitionTable, types: &[Ty<DefinitionId>]) -> Vec<Ty> {
    types
        .iter()
        .map(|ty| {
            ty.map_definitions(
                &mut |id| Ok(definitions.resolve(*id)?.to_path()),
                &Default::default(),
            )
            .unwrap()
        })
        .collect()
}

fn codec() -> impl Options {
    DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
}

#[test]
fn one_type_model_preserves_every_reference_across_portable_and_runtime_contexts() {
    let authoring = fixture();
    let mut source = DefinitionTableBuilder::new().unwrap();
    let types = intern(&mut source, &authoring);
    let encoded = PortableTypes::encode(&source.freeze(), &types, &Default::default()).unwrap();
    let bytes = codec().serialize(&encoded).unwrap();
    let decoded = codec()
        .deserialize::<PortableTypes>(&bytes)
        .unwrap()
        .decode(&Default::default())
        .unwrap();
    assert_eq!(paths(&decoded.definitions, &decoded.types), authoring);
    assert_ne!(decoded.definitions.id(), source.freeze().id());
    let mut runtime_a = DefinitionTableBuilder::new().unwrap();
    let mut runtime_b = DefinitionTableBuilder::new().unwrap();
    let first = decoded
        .import_into(&mut runtime_a, &Default::default())
        .unwrap();
    let second = decoded
        .import_into(&mut runtime_b, &Default::default())
        .unwrap();
    assert_eq!(paths(&runtime_a.freeze(), &first), authoring);
    assert_eq!(paths(&runtime_b.freeze(), &second), authoring);
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
    let authoring = fixture();
    let mut first = DefinitionTableBuilder::new().unwrap();
    let first_types = intern(&mut first, &authoring);
    let mut second = DefinitionTableBuilder::new().unwrap();
    second
        .intern_path(&path(DefinitionKind::Function, "unrelated"))
        .unwrap();
    let reversed: Vec<_> = authoring.iter().rev().cloned().collect();
    intern(&mut second, &reversed);
    let second_types = intern(&mut second, &authoring);
    let encode = |builder: &DefinitionTableBuilder, types: &[Ty<DefinitionId>]| {
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
    let types = intern(
        &mut source,
        &[Ty::Host(path(DefinitionKind::Struct, "Host"))],
    );
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
    portable.types[0] = Ty::Host(encoding.reference(id).unwrap());
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
    let only = target
        .intern_path(&path(DefinitionKind::Trait, "Only"))
        .unwrap();
    assert_eq!(
        fixture()[0]
            .map_definitions(&mut |_| Ok(only), &Default::default())
            .unwrap_err(),
        IdentityTransformError::Type(TypeTransformError::InvalidContract)
    );
    let cancel = CancellationToken::default();
    cancel.cancel();
    let mut visits = 0;
    assert_eq!(
        fixture()[0]
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
    assert_eq!(size_of::<GenericParam<DefinitionId>>(), 16);
    let mut definitions = DefinitionTableBuilder::new().unwrap();
    let owner = definitions
        .intern_path(&path(DefinitionKind::Function, "run"))
        .unwrap();
    let binder = GenericParam { owner, position: 0 };
    let arguments: [Ty<DefinitionId>; 1] = [Ty::Builtin(BuiltinType::I64)];
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
