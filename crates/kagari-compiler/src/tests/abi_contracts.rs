use crate::tests::common;
use bincode::Options;
use kagari_bytecode::{program::verify_program, verifier::BytecodeVerificationError};
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, map::DefinitionContext,
    table::DefinitionId,
};
use kagari_contract::types::PublicItem;
use kagari_hir::types::semantic::{lower_type, raise_type};
use kagari_types::{
    collection::CollectionAccess::{Mutable, ReadOnly},
    scalar::BuiltinType,
    ty::{GenericParam, NominalTy, Ty, identity::PortableTypes},
};
use std::slice;

fn codec() -> impl Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
}

fn owner(
    context: &DefinitionContext,
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
    context
        .intern(&DefinitionPath {
            module: module.clone(),
            path,
        })
        .unwrap()
}

#[test]
fn interface_tables_require_distinct_local_impl_identities() {
    let context = DefinitionContext::new().unwrap();
    let original = common::adopt_bytecode(
        &context,
        common::bytecode_ok(
            "struct Player { val value: i32 } pub trait Display { fn show(self) -> i32; } impl Display for Player { fn show(self) -> i32 { self.value } } fn main() -> i32 { 1 }",
        ),
    );
    let table_index = original.modules[original.root.index()]
        .public_items
        .iter()
        .position(|item| matches!(item, PublicItem::InterfaceTable(_)))
        .expect("checked interface table");
    let PublicItem::InterfaceTable(table) =
        &original.modules[original.root.index()].public_items[table_index]
    else {
        unreachable!()
    };
    assert_eq!(
        *context
            .snapshot()
            .resolve(table.declaration)
            .unwrap()
            .module(),
        original.modules[original.root.index()].identity
    );
    assert_eq!(
        context
            .snapshot()
            .resolve(table.declaration)
            .unwrap()
            .segments()
            .count(),
        1
    );
    assert_eq!(
        context
            .snapshot()
            .resolve(table.declaration)
            .unwrap()
            .last()
            .unwrap()
            .kind,
        DefinitionKind::Impl
    );
    assert!(
        context
            .snapshot()
            .resolve(table.declaration)
            .unwrap()
            .last()
            .unwrap()
            .name
            .is_empty()
    );
    for corruption in 0..3 {
        let mut module = original.clone();
        let PublicItem::InterfaceTable(table) =
            &mut module.modules[module.root.index()].public_items[table_index]
        else {
            unreachable!()
        };
        common::edit_definition(&context, &mut table.declaration, |path| match corruption {
            0 => path.module.package.0 = "foreign".into(),
            1 => path.path[0].kind = DefinitionKind::Trait,
            _ => path.path[0].name = "fabricated".into(),
        });
        assert!(matches!(
            verify_program(&module, &context.snapshot(), &Default::default()),
            Err(BytecodeVerificationError::InvalidPublicAbi)
        ));
    }
    for corruption in 0..3 {
        let mut module = original.clone();
        let PublicItem::InterfaceTable(table) =
            &mut module.modules[module.root.index()].public_items[table_index]
        else {
            unreachable!()
        };
        match corruption {
            0 => table.methods[0].return_type = Ty::Builtin(BuiltinType::Bool),
            1 => table.methods[0].params[0].mutable = true,
            _ => table.methods[0].params[0].ty = Ty::Builtin(BuiltinType::I32),
        }
        assert!(matches!(
            verify_program(&module, &context.snapshot(), &Default::default()),
            Err(BytecodeVerificationError::InvalidPublicAbi)
        ));
    }
    let mut wrong_trait = original.clone();
    let PublicItem::InterfaceTable(table) =
        &mut wrong_trait.modules[wrong_trait.root.index()].public_items[table_index]
    else {
        unreachable!()
    };
    let Ty::Trait(reference) = &mut table.trait_type else {
        unreachable!()
    };
    common::edit_definition(&context, &mut reference.declaration, |path| {
        path.path[0].occurrence = 1
    });
    assert!(matches!(
        verify_program(&wrong_trait, &context.snapshot(), &Default::default()),
        Err(BytecodeVerificationError::InvalidPublicAbi)
    ));
    let mut duplicate = original.clone();
    let duplicate_table =
        duplicate.modules[duplicate.root.index()].public_items[table_index].clone();
    duplicate.modules[duplicate.root.index()]
        .public_items
        .push(duplicate_table);
    assert!(matches!(
        verify_program(&duplicate, &context.snapshot(), &Default::default()),
        Err(BytecodeVerificationError::InvalidPublicAbi)
    ));
    for corruption in 0..3 {
        let mut module = original.clone();
        let PublicItem::InterfaceTable(table) =
            &mut module.modules[module.root.index()].public_items[table_index]
        else {
            unreachable!()
        };
        match corruption {
            0 => table.methods.clear(),
            1 => table.methods[0].name = "other".into(),
            _ => table.methods.push(table.methods[0].clone()),
        }
        assert!(matches!(
            verify_program(&module, &context.snapshot(), &Default::default()),
            Err(BytecodeVerificationError::InvalidPublicAbi)
        ));
    }
}

#[test]
fn public_signatures_reject_foreign_parameters_invalid_arity_and_escaped_self() {
    let context = DefinitionContext::new().unwrap();
    let original = common::adopt_bytecode(
        &context,
        common::bytecode_ok(
            "use std::hash::{Hash};\npub fn plain() -> i32 { 1 } pub trait Identity { fn same<T: Eq + Hash + PartialEq>(self, value: T) -> T; }",
        ),
    );
    for corruption in 0..8 {
        let mut module = original.clone();
        let identity = module.modules[module.root.index()].identity.clone();
        let (functions, traits) = module.modules[module.root.index()]
            .public_items
            .split_at_mut(1);
        let PublicItem::Function(function) = &mut functions[0] else {
            panic!("public function")
        };
        let PublicItem::Trait(interface) = &mut traits[0] else {
            panic!("public trait")
        };
        match corruption {
            0 => common::edit_definition(
                &context,
                &mut interface.methods[0].generic_params[0].owner,
                |path| path.module.package.0 = "foreign".into(),
            ),
            1 => {
                if let Ty::Parameter { position, .. } = &mut interface.methods[0].params[1].ty {
                    *position = 99;
                }
            }
            2 => {
                function.return_type = Ty::SelfType(owner(
                    &context,
                    &identity,
                    &[],
                    DefinitionKind::Trait,
                    "Identity",
                ))
            }
            3 => {
                function.return_type = Ty::Enum(kagari_types::ty::NominalTy {
                    declaration: context
                        .intern(&kagari_types::language::binding::result_declaration())
                        .unwrap(),
                    arguments: vec![Ty::Builtin(BuiltinType::I32)],
                    associated_types: Default::default(),
                })
            }
            4 => {
                function.return_type = Ty::Struct(NominalTy {
                    associated_types: Default::default(),
                    declaration: owner(&context, &identity, &[], DefinitionKind::Trait, "Identity"),
                    arguments: vec![],
                })
            }
            5 => function.generic_params.push(GenericParam {
                owner: owner(&context, &identity, &[], DefinitionKind::Function, "plain"),
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
        let error = verify_program(&module, &context.snapshot(), &Default::default()).unwrap_err();
        if corruption == 5 {
            // A generic declaration cannot back the retained concrete export.
            // The package export contract rejects this before public ABI checks.
            assert_eq!(error, BytecodeVerificationError::InvalidProgramExports);
        } else if corruption == 3 {
            assert!(
                matches!(
                    error,
                    BytecodeVerificationError::InvalidOperation {
                        reason: "invalid collection access flow",
                        ..
                    }
                ),
                "{error:?}"
            );
        } else {
            assert_eq!(
                error,
                BytecodeVerificationError::InvalidPublicAbi,
                "corruption {corruption}"
            );
        }
    }
    let mut module = original;
    let identity = module.modules[module.root.index()].identity.clone();
    let PublicItem::Trait(interface) = &mut module.modules[module.root.index()].public_items[1]
    else {
        panic!("public trait")
    };
    interface.methods[0].return_type = Ty::SelfType(owner(
        &context,
        &identity,
        &[],
        DefinitionKind::Trait,
        "Other",
    ));
    assert!(matches!(
        verify_program(&module, &context.snapshot(), &Default::default()),
        Err(BytecodeVerificationError::InvalidPublicAbi)
    ));
}

#[test]
fn collection_access_survives_checked_and_wire_conversions() {
    let context = DefinitionContext::new().unwrap();

    let integer = Ty::Builtin(BuiltinType::I32);
    for access in [ReadOnly, Mutable] {
        for ty in [
            Ty::Set(Box::new(integer.clone()), access),
            Ty::Map {
                key: Box::new(integer.clone()),
                value: Box::new(Ty::Array(Box::new(integer.clone()))),
                access,
            },
        ] {
            let checked = raise_type(&ty);
            assert_eq!(checked.collection_access(), Some(access));
            assert_eq!(lower_type(&checked), ty);
            let portable = PortableTypes::encode(
                &context.snapshot(),
                slice::from_ref(&ty),
                &Default::default(),
            )
            .unwrap();
            let bytes = codec().serialize(&portable).unwrap();
            let decoded = codec()
                .deserialize::<PortableTypes>(&bytes)
                .unwrap()
                .decode(&Default::default())
                .unwrap();
            assert_eq!(decoded.types, [ty]);
        }
    }
    let array = Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32)));
    assert_eq!(raise_type(&array).collection_access(), None);
    assert_eq!(lower_type(&raise_type(&array)), array);
    assert_eq!(
        codec()
            .deserialize::<PortableTypes>(
                &codec()
                    .serialize(
                        &PortableTypes::encode(
                            &context.snapshot(),
                            slice::from_ref(&array),
                            &Default::default()
                        )
                        .unwrap()
                    )
                    .unwrap()
            )
            .unwrap()
            .decode(&Default::default())
            .unwrap()
            .types,
        [array]
    );
    let readonly = Ty::Set(Box::new(integer.clone()), ReadOnly);
    let mutable = Ty::Set(Box::new(integer), Mutable);
    assert_ne!(
        codec()
            .serialize(
                &PortableTypes::encode(&context.snapshot(), &[readonly], &Default::default())
                    .unwrap()
            )
            .unwrap(),
        codec()
            .serialize(
                &PortableTypes::encode(&context.snapshot(), &[mutable], &Default::default())
                    .unwrap()
            )
            .unwrap()
    );
}
