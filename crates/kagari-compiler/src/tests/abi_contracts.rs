use crate::source::types::lower_type;
use crate::tests::common::bytecode_ok;
use bincode::Options;
use kagari_abi::scalar::BuiltinType;
use kagari_abi::types::{
    AbiType, GenericParameterAbi, NominalAbiType, PublicAbiItem, StandardEnumKind,
};
use kagari_bytecode::{BytecodeVerificationError, verify_module};
use kagari_common::collection::CollectionAccess;
use kagari_common::identity::{
    DefinitionId, DefinitionKind, DefinitionPathSegment, ModuleIdentity,
};

fn codec() -> impl Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
}
fn owner(
    module: &ModuleIdentity,
    parent: &[DefinitionPathSegment],
    kind: DefinitionKind,
    name: &str,
) -> DefinitionId {
    let mut path = parent.to_vec();
    path.push(DefinitionPathSegment {
        kind,
        name: name.into(),
        occurrence: 0,
    });
    DefinitionId {
        module: module.clone(),
        path,
    }
}
#[test]
fn interface_tables_require_distinct_local_impl_identities() {
    let original = bytecode_ok(
        "struct Player { val value: i32 } pub trait Display { fn show(self) -> i32; } impl Display for Player { fn show(self) -> i32 { self.value } } fn main() -> i32 { 1 }",
    );
    let table_index = original
        .public_items
        .iter()
        .position(|item| matches!(item, PublicAbiItem::InterfaceTable(_)))
        .expect("checked interface table");
    let PublicAbiItem::InterfaceTable(table) = &original.public_items[table_index] else {
        unreachable!()
    };
    assert_eq!(table.declaration.module, original.identity);
    assert_eq!(table.declaration.path.len(), 1);
    assert_eq!(table.declaration.path[0].kind, DefinitionKind::Impl);
    assert!(table.declaration.path[0].name.is_empty());
    for corruption in 0..3 {
        let mut module = original.clone();
        let PublicAbiItem::InterfaceTable(table) = &mut module.public_items[table_index] else {
            unreachable!()
        };
        match corruption {
            0 => table.declaration.module.package.0 = "foreign".into(),
            1 => table.declaration.path[0].kind = DefinitionKind::Trait,
            _ => table.declaration.path[0].name = "fabricated".into(),
        }
        assert!(matches!(
            verify_module(&module),
            Err(BytecodeVerificationError::InvalidPublicAbi)
        ));
    }
    for corruption in 0..3 {
        let mut module = original.clone();
        let PublicAbiItem::InterfaceTable(table) = &mut module.public_items[table_index] else {
            unreachable!()
        };
        match corruption {
            0 => table.methods[0].return_type = AbiType::Builtin(BuiltinType::Bool),
            1 => table.methods[0].params[0].mutable = true,
            _ => table.methods[0].params[0].ty = AbiType::Builtin(BuiltinType::I32),
        }
        assert!(matches!(
            verify_module(&module),
            Err(BytecodeVerificationError::InvalidPublicAbi)
        ));
    }
    let mut wrong_trait = original.clone();
    let PublicAbiItem::InterfaceTable(table) = &mut wrong_trait.public_items[table_index] else {
        unreachable!()
    };
    let AbiType::Trait(reference) = &mut table.trait_type else {
        unreachable!()
    };
    reference.declaration.path[0].occurrence = 1;
    assert!(matches!(
        verify_module(&wrong_trait),
        Err(BytecodeVerificationError::InvalidPublicAbi)
    ));
    let mut duplicate = original.clone();
    duplicate
        .public_items
        .push(duplicate.public_items[table_index].clone());
    assert!(matches!(
        verify_module(&duplicate),
        Err(BytecodeVerificationError::InvalidPublicAbi)
    ));
    for corruption in 0..3 {
        let mut module = original.clone();
        let PublicAbiItem::InterfaceTable(table) = &mut module.public_items[table_index] else {
            unreachable!()
        };
        match corruption {
            0 => table.methods.clear(),
            1 => table.methods[0].name = "other".into(),
            _ => table.methods.push(table.methods[0].clone()),
        }
        assert!(matches!(
            verify_module(&module),
            Err(BytecodeVerificationError::InvalidPublicAbi)
        ));
    }
}

#[test]
fn public_signatures_reject_foreign_parameters_invalid_arity_and_escaped_self() {
    let original = bytecode_ok(
        "pub fn plain() -> i32 { 1 } pub trait Identity { fn same<T: Eq + Hash + PartialEq>(self, value: T) -> T; }",
    );
    for corruption in 0..8 {
        let mut module = original.clone();
        let (functions, traits) = module.public_items.split_at_mut(1);
        let PublicAbiItem::Function(function) = &mut functions[0] else {
            panic!("public function")
        };
        let PublicAbiItem::Trait(interface) = &mut traits[0] else {
            panic!("public trait")
        };
        match corruption {
            0 => {
                interface.methods[0].generic_params[0]
                    .owner
                    .module
                    .package
                    .0 = "foreign".into()
            }
            1 => {
                if let AbiType::Parameter { position, .. } = &mut interface.methods[0].params[1].ty
                {
                    *position = 99;
                }
            }
            2 => {
                function.return_type = AbiType::SelfType(owner(
                    &module.identity,
                    &[],
                    DefinitionKind::Trait,
                    "Identity",
                ))
            }
            3 => {
                function.return_type = AbiType::StandardEnum {
                    kind: StandardEnumKind::Result,
                    args: vec![AbiType::Builtin(BuiltinType::I32)],
                }
            }
            4 => {
                function.return_type = AbiType::Struct(NominalAbiType {
                    associated_types: Default::default(),
                    declaration: owner(&module.identity, &[], DefinitionKind::Trait, "Identity"),
                    arguments: vec![],
                })
            }
            5 => function.generic_params.push(GenericParameterAbi {
                owner: owner(&module.identity, &[], DefinitionKind::Function, "plain"),
                position: 0,
            }),
            6 => interface.methods[0].bounds[0].constraints.reverse(),
            _ => {
                let constraint = interface.methods[0].bounds[0].constraints[0].clone();
                interface.methods[0].bounds[0]
                    .constraints
                    .insert(0, constraint);
            }
        }
        assert!(
            matches!(
                verify_module(&module),
                Err(BytecodeVerificationError::InvalidPublicAbi)
            ),
            "corruption {corruption}"
        );
    }
    let mut module = original;
    let PublicAbiItem::Trait(interface) = &mut module.public_items[1] else {
        panic!("public trait")
    };
    interface.methods[0].return_type =
        AbiType::SelfType(owner(&module.identity, &[], DefinitionKind::Trait, "Other"));
    assert!(matches!(
        verify_module(&module),
        Err(BytecodeVerificationError::InvalidPublicAbi)
    ));
}

#[test]
fn collection_access_survives_checked_host_and_wire_conversions() {
    use CollectionAccess::{Mutable, ReadOnly};
    use kagari_common::host_interface::HostValueType;
    let integer = AbiType::Builtin(BuiltinType::I32);
    for access in [ReadOnly, Mutable] {
        for ty in [
            AbiType::Array(Box::new(integer.clone()), access),
            AbiType::Set(Box::new(integer.clone()), access),
            AbiType::Map {
                key: Box::new(integer.clone()),
                value: Box::new(AbiType::Array(Box::new(integer.clone()), ReadOnly)),
                access,
            },
        ] {
            let checked = ty.to_checked_type();
            assert_eq!(checked.collection_access(), Some(access));
            assert_eq!(lower_type(&checked), ty);
            let bytes = codec().serialize(&ty).unwrap();
            assert_eq!(codec().deserialize::<AbiType>(&bytes).unwrap(), ty);
        }
        for host in [
            HostValueType::Array(Box::new(HostValueType::I32), access),
            HostValueType::Set(Box::new(HostValueType::String), access),
            HostValueType::Map {
                key: Box::new(HostValueType::String),
                value: Box::new(HostValueType::Array(Box::new(HostValueType::I32), ReadOnly)),
                access,
            },
        ] {
            assert_eq!(
                AbiType::from_host_type(&host)
                    .to_checked_type()
                    .collection_access(),
                Some(access)
            );
        }
    }
    let readonly = AbiType::Array(Box::new(integer.clone()), ReadOnly);
    let mutable = AbiType::Array(Box::new(integer), Mutable);
    assert_ne!(
        codec().serialize(&readonly).unwrap(),
        codec().serialize(&mutable).unwrap()
    );
}
