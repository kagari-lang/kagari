use crate::{BytecodeArtifact, engine::KagariEngine};
use kagari_bytecode::program::{identity, verify_program};
use kagari_common::identity::{DefinitionKind, map::DefinitionContext};
use kagari_contract::types::PublicItem;
use kagari_source::source::SourceFile;
use kagari_types::{
    declaration::{AssociatedTypeDef, AssociatedTypeFamily},
    scalar::BuiltinType,
    ty::{GenericBound, GenericParam, NominalTy, Ty},
};

#[test]
fn unused_family_metadata_is_verified_before_loading() {
    let engine = KagariEngine::default();
    let artifact = engine.compile_to_artifact(SourceFile::new("families.kgr", "pub trait Extra {} pub trait Family { type Item<T: PartialEq>: PartialEq; fn make<T: PartialEq>(self, value:T)->Self::Item<T>; } struct N {} impl Family for N { type Item<U> = U; fn make<V: PartialEq>(self, value:V)->V { value } } fn main()->i32 { 42 }"),  Default::default()).unwrap();
    let context = DefinitionContext::new().unwrap();
    let artifact = artifact
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let table = identity::collect(
        &artifact.program.modules.iter().collect::<Vec<_>>(),
        &context,
        &Default::default(),
    )
    .unwrap();
    verify_program(&artifact.program, &table, &Default::default()).unwrap();
    let root = table
        .lookup_root(&artifact.program.modules[artifact.program.root.index()].identity)
        .unwrap();
    // Use a carried ordinary trait, so rejection tests a stronger family
    // requirement rather than an unknown or incorrectly scoped declaration.
    let stronger = NominalTy {
        declaration: table
            .lookup_child(root, DefinitionKind::Trait, "Extra", 0)
            .unwrap(),
        arguments: vec![],
        associated_types: Default::default(),
    };
    for mutation in 0..10 {
        let mut program = artifact.program.clone();
        let module = &mut program.modules[program.root.index()];
        if mutation < 3 {
            let record = module
                .public_items
                .iter_mut()
                .find_map(|item| {
                    if let PublicItem::Trait(record) = item {
                        (record.name == "Family").then_some(record)
                    } else {
                        None
                    }
                })
                .unwrap();
            match mutation {
                0 => {
                    let owner = &mut record.associated_types[0].generic_params[0].owner;
                    *owner = context.snapshot().parent(*owner).unwrap().unwrap();
                }
                1 => record.associated_types[0].generic_params[0].position = 1,
                _ => {
                    let Ty::Projection { arguments, .. } = &mut record.methods[0].return_type
                    else {
                        panic!("projection")
                    };
                    arguments.clear();
                }
            }
        } else {
            let table = module
                .public_items
                .iter_mut()
                .find_map(|item| {
                    if let PublicItem::InterfaceTable(table) = item {
                        Some(table)
                    } else {
                        None
                    }
                })
                .unwrap();
            let family = &mut table.associated_type_families[0];
            match mutation {
                3 => family.generic_params[0].owner = table.declaration,
                4 => family.generic_params[0].position = 1,
                5 => family.generic_params.clear(),
                6 => family.value = Ty::Builtin(BuiltinType::Unit),
                7 => family.bounds.push(GenericBound {
                    ty: Ty::Parameter {
                        owner: family.generic_params[0].owner,
                        position: 0,
                    },
                    constraints: vec![stronger.clone()],
                }),
                8 => {
                    let definitions = context.snapshot();
                    let parent = definitions.parent(family.declaration).unwrap().unwrap();
                    let segment = definitions
                        .resolve(family.declaration)
                        .unwrap()
                        .last()
                        .unwrap();
                    family.declaration = context
                        .intern_child(parent, segment.kind, "Unknown", segment.occurrence)
                        .unwrap();
                }
                _ => {
                    family.value = Ty::Projection {
                        receiver: Box::new(table.for_type.clone()),
                        interface: Box::new(match &table.trait_type {
                            Ty::Trait(v) => v.clone(),
                            _ => unreachable!(),
                        }),
                        member: family.declaration,
                        arguments: vec![Ty::Parameter {
                            owner: family.generic_params[0].owner,
                            position: 0,
                        }],
                    }
                }
            }
        }
        assert!(
            BytecodeArtifact::from_program(
                program,
                Default::default(),
                &context,
                &Default::default()
            )
            .is_err(),
            "accepted mutation {mutation}"
        );
    }
}

#[test]
fn complete_family_metadata_cannot_make_a_dynamic_interface() {
    let engine = KagariEngine::default();
    let artifact = engine.compile_to_artifact(SourceFile::new("dynamic.kgr", "pub trait Read { fn read(self)->i32; } struct N {} impl Read for N { fn read(self)->i32 { 42 } } fn main()->i32 { val x: Read = N {}; x.read() }"),  Default::default()).unwrap();
    let context = DefinitionContext::new().unwrap();
    let artifact = artifact
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let table = identity::collect(
        &artifact.program.modules.iter().collect::<Vec<_>>(),
        &context,
        &Default::default(),
    )
    .unwrap();
    verify_program(&artifact.program, &table, &Default::default()).unwrap();
    let mut program = artifact.program.clone();
    let module = &mut program.modules[program.root.index()];
    let (interface, implementation) = module
        .public_items
        .iter()
        .find_map(|item| {
            let PublicItem::InterfaceTable(table) = item else {
                return None;
            };
            let Ty::Trait(interface) = &table.trait_type else {
                return None;
            };
            Some((interface.declaration, table.declaration))
        })
        .unwrap();
    let member = context
        .intern_child(interface, DefinitionKind::AssociatedType, "Item", 0)
        .unwrap();
    let binder = context
        .intern_child(implementation, DefinitionKind::AssociatedType, "Item", 0)
        .unwrap();
    for item in &mut module.public_items {
        match item {
            PublicItem::Trait(record) => record.associated_types.push(AssociatedTypeDef {
                declaration: member,
                generic_params: vec![GenericParam {
                    owner: member,
                    position: 0,
                }],
                parameter_bounds: Vec::new(),
                bounds: Vec::new(),
            }),
            PublicItem::InterfaceTable(table) => {
                table.associated_type_families.push(AssociatedTypeFamily {
                    declaration: member,
                    generic_params: vec![GenericParam {
                        owner: binder,
                        position: 0,
                    }],
                    bounds: Vec::new(),
                    value: Ty::Parameter {
                        owner: binder,
                        position: 0,
                    },
                })
            }
            _ => {}
        }
    }
    assert!(
        BytecodeArtifact::from_program(program, Default::default(), &context, &Default::default())
            .is_err()
    );
}
