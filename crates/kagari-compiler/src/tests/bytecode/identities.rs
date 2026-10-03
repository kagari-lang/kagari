use crate::tests::bytecode::*;
use kagari_contract::types as abi;

#[test]
fn executable_function_identities_survive_lowering_and_reject_mismatched_records() {
    let module = common::bytecode_ok(
        "fn id<T>(value: T) -> T { value } fn other() -> i32 { 2 } fn main() -> i32 { id(1) + other() }",
    );
    assert!(
        module.modules[module.root.index()]
            .functions
            .iter()
            .all(|function| {
                function.identity.is_some()
                    && module.modules[module.root.index()].function_table[function.id.index()]
                        .identity
                        == function.identity
            })
    );
    let generic = module.modules[module.root.index()]
        .functions
        .iter()
        .position(|function| {
            function.identity.as_ref().is_some_and(|identity| {
                identity
                    .declaration
                    .path
                    .last()
                    .is_some_and(|part| part.name == "id")
            })
        })
        .expect("reachable generic instance");
    assert_eq!(
        module.modules[module.root.index()].functions[generic]
            .identity
            .as_ref()
            .unwrap()
            .arguments,
        [abi::Ty::Builtin(kagari_contract::scalar::BuiltinType::I32)]
    );
    let mut mismatched_record = module.clone();
    mismatched_record.modules[mismatched_record.root.index()].function_table[generic].identity =
        None;
    assert!(matches!(
        verify_program(&mismatched_record),
        Err(BytecodeVerificationError::FunctionRecordMismatch { .. })
    ));
    let mut duplicate = module.clone();
    let other = duplicate.modules[duplicate.root.index()]
        .functions
        .iter()
        .position(|function| {
            function.identity.as_ref().is_some_and(|identity| {
                identity
                    .declaration
                    .path
                    .last()
                    .is_some_and(|part| part.name == "other")
            })
        })
        .unwrap();
    duplicate.modules[duplicate.root.index()].functions[other].identity =
        duplicate.modules[duplicate.root.index()].functions[generic]
            .identity
            .clone();
    duplicate.modules[duplicate.root.index()].function_table[other].identity =
        duplicate.modules[duplicate.root.index()].functions[other]
            .identity
            .clone();
    assert!(matches!(
        verify_program(&duplicate),
        Err(BytecodeVerificationError::InvalidFunctionIdentity { .. })
    ));
    let mut foreign = module.clone();
    foreign.modules[foreign.root.index()].functions[generic]
        .identity
        .as_mut()
        .unwrap()
        .declaration
        .module
        .package
        .0 = "foreign".into();
    foreign.modules[foreign.root.index()].function_table[generic].identity =
        foreign.modules[foreign.root.index()].functions[generic]
            .identity
            .clone();
    assert!(matches!(
        verify_program(&foreign),
        Err(BytecodeVerificationError::InvalidFunctionIdentity { .. })
    ));
    let mut wrong_kind = module.clone();
    wrong_kind.modules[wrong_kind.root.index()].functions[generic]
        .identity
        .as_mut()
        .unwrap()
        .declaration
        .path
        .last_mut()
        .unwrap()
        .kind = kagari_common::identity::DefinitionKind::Struct;
    wrong_kind.modules[wrong_kind.root.index()].function_table[generic].identity =
        wrong_kind.modules[wrong_kind.root.index()].functions[generic]
            .identity
            .clone();
    assert!(matches!(
        verify_program(&wrong_kind),
        Err(BytecodeVerificationError::InvalidFunctionIdentity { .. })
    ));
    let mut oversized = module.clone();
    oversized.modules[oversized.root.index()].functions[generic]
        .identity
        .as_mut()
        .unwrap()
        .arguments = vec![
        abi::Ty::Builtin(kagari_contract::scalar::BuiltinType::I32);
        kagari_contract::decode_limits::MAX_NESTED_RECORDS + 1
    ];
    oversized.modules[oversized.root.index()].function_table[generic].identity =
        oversized.modules[oversized.root.index()].functions[generic]
            .identity
            .clone();
    assert!(matches!(
        KbcArtifact::from_program(oversized, Default::default(),),
        Err(ArtifactValidationError::ResourceLimit(
            "nested module record limit exceeded"
        ))
    ));
    let mut unresolved = module;
    unresolved.modules[unresolved.root.index()].functions[generic]
        .identity
        .as_mut()
        .unwrap()
        .arguments[0] = kagari_contract::types::Ty::Parameter {
        owner: unresolved.modules[unresolved.root.index()].function_table[generic]
            .identity
            .as_ref()
            .unwrap()
            .declaration
            .clone(),
        position: 0,
    };
    unresolved.modules[unresolved.root.index()].function_table[generic].identity =
        unresolved.modules[unresolved.root.index()].functions[generic]
            .identity
            .clone();
    assert!(matches!(
        verify_program(&unresolved),
        Err(BytecodeVerificationError::InvalidFunctionIdentity { .. })
    ));
}

#[test]
fn artifact_loader_rejects_invalid_struct_layouts_slots_and_initializers() {
    let original = common::bytecode_ok(
        "struct P { var x: i32, val fixed: bool } fn main() -> i32 { val p = P { fixed: true, x: 1 }; p.x = 2; p.x }",
    );
    let valid = KbcArtifact::from_program(original.clone(), Default::default()).unwrap();
    for corruption in 0..7 {
        let mut module = original.clone();
        match corruption {
            0 => {
                let member = &mut module.modules[module.root.index()];
                member.structures.push(member.structures[0].clone());
            }
            1 => module.modules[module.root.index()].structures[0].fields[0]
                .declaration
                .module
                .path
                .push("foreign".into()),
            2 => module.modules[module.root.index()].structures[0].fields[0].mutable = false,
            _ => {
                for instruction in module.modules[module.root.index()]
                    .functions
                    .iter_mut()
                    .flat_map(|function| &mut function.instructions)
                {
                    match instruction {
                        BytecodeInstruction::MakeStruct {
                            structure, fields, ..
                        } => match corruption {
                            3 => *structure = StructId::new(999),
                            4 => {
                                fields.pop();
                            }
                            5 => fields.swap(0, 1),
                            _ => {}
                        },
                        BytecodeInstruction::ReadAggregateField { field, .. }
                            if corruption == 6 =>
                        {
                            field.slot = u32::MAX
                        }
                        _ => {}
                    }
                }
            }
        }
        assert!(
            KbcArtifact::from_program(module.clone(), ArtifactBuildOptions::default(),).is_err()
        );
        let mut corrupted = valid.clone();
        corrupted.program = module;
        let bytes = corrupted.to_bytes().unwrap();
        let artifact = KbcArtifact::from_bytes(&bytes).unwrap();
        assert!(
            matches!(
                artifact.validate_for_loader(&ArtifactCompatibility::default()),
                Err(ArtifactValidationError::Bytecode(_))
            ),
            "corruption {corruption}"
        );
    }
}

#[test]
fn executable_struct_fields_require_concrete_resolved_types() {
    use kagari_contract::{
        scalar::BuiltinType,
        types::{NominalTy, Ty},
    };
    let module = common::bytecode_ok(
        "struct Box<T> { val value: T } fn main() -> i32 { Box<i32> { value: 42 }.value }",
    );
    let declaration = module.modules[module.root.index()].structures[0]
        .declaration
        .clone();
    for ty in [
        Ty::Parameter {
            owner: declaration.clone(),
            position: 0,
        },
        Ty::Struct(NominalTy {
            associated_types: Default::default(),
            declaration,
            arguments: vec![Ty::Builtin(BuiltinType::Bool)],
        }),
        Ty::Builtin(BuiltinType::Bool),
    ] {
        let mut invalid = module.clone();
        invalid.modules[invalid.root.index()].structures[0].fields[0].ty = ty;
        assert!(verify_program(&invalid).is_err());
    }
}

#[test]
fn struct_instances_must_match_public_templates_locally_and_across_modules() {
    use kagari_bytecode::program::{BytecodeProgram, ModuleRef, verify_program};
    use kagari_contract::{scalar::BuiltinType, types::Ty};
    let owner = common::bytecode_ok(
        "pub struct Box<T> { var values: ArrayList<T> } fn main() -> i32 { Box<i32> { values: [42] }.values[0] }",
    );
    let mut importer = BytecodeModule {
        identity: ModuleIdentity::single_file("importer.kgr"),
        structures: owner.modules[owner.root.index()].structures.clone(),
        dependencies: vec![ModuleRef::new(0)],
        ..Default::default()
    };
    let program = |importer| {
        let declaration_owner = BytecodeModule {
            identity: owner.modules[owner.root.index()].identity.clone(),
            public_items: owner.modules[owner.root.index()].public_items.clone(),
            ..Default::default()
        };
        BytecodeProgram {
            root: ModuleRef::new(1),
            modules: vec![declaration_owner, importer],
        }
    };
    verify_program(&program(importer.clone())).unwrap();
    for mutation in 0..4 {
        let mut invalid = owner.clone();
        match mutation {
            0 => {
                invalid.modules[invalid.root.index()].structures[0].fields[0].ty = Ty::Array(
                    Box::new(Ty::Builtin(BuiltinType::Bool)),
                    CollectionAccess::Mutable,
                )
            }
            1 => invalid.modules[invalid.root.index()].structures[0].fields[0].mutable = false,
            2 => {
                invalid.modules[invalid.root.index()].structures[0].fields[0].name = "other".into();
                invalid.modules[invalid.root.index()].structures[0].fields[0]
                    .declaration
                    .path
                    .last_mut()
                    .unwrap()
                    .name = "other".into();
            }
            _ => invalid.modules[invalid.root.index()].structures[0]
                .fields
                .clear(),
        }
        assert_eq!(
            verify_program(&invalid),
            Err(BytecodeVerificationError::InvalidStructLayout)
        );
        importer.structures = invalid.modules[invalid.root.index()].structures.clone();
        // An imported layout can be internally valid without matching its owner.
        let mut standalone = importer.clone();
        standalone.dependencies.clear();
        verify_module(&standalone).unwrap();
        assert_eq!(
            verify_program(&program(importer.clone())),
            Err(BytecodeVerificationError::InvalidStructLayout)
        );
    }
}

#[test]
fn executable_layouts_reject_noncanonical_declaration_and_member_identities() {
    let module = common::bytecode_ok(
        "pub struct Item { val value: i32 } pub enum Token { Data(i32) } fn main() -> i32 { val token = Token::Data(1); Item { value: 42 }.value }",
    );
    let enumeration = module.modules[module.root.index()]
        .enumerations
        .iter()
        .position(|layout| {
            layout.declaration.module == module.modules[module.root.index()].identity
        })
        .unwrap();
    for mutation in 0..6 {
        let mut invalid = module.clone();
        match mutation {
            0 => {
                invalid.modules[invalid.root.index()].structures[0]
                    .declaration
                    .path[0]
                    .occurrence = 1;
                invalid.modules[invalid.root.index()].structures[0].fields[0]
                    .declaration
                    .path[0]
                    .occurrence = 1;
            }
            1 => {
                invalid.modules[invalid.root.index()].structures[0].fields[0]
                    .declaration
                    .path[1]
                    .occurrence = 1
            }
            2 => {
                let parent = invalid.modules[invalid.root.index()].structures[0]
                    .declaration
                    .path[0]
                    .clone();
                invalid.modules[invalid.root.index()].structures[0]
                    .declaration
                    .path
                    .insert(0, parent.clone());
                invalid.modules[invalid.root.index()].structures[0].fields[0]
                    .declaration
                    .path
                    .insert(0, parent);
            }
            3 => {
                invalid.modules[invalid.root.index()].enumerations[enumeration]
                    .declaration
                    .path[0]
                    .occurrence = 1;
                invalid.modules[invalid.root.index()].enumerations[enumeration].variants[0]
                    .declaration
                    .path[0]
                    .occurrence = 1;
            }
            4 => {
                invalid.modules[invalid.root.index()].enumerations[enumeration].variants[0]
                    .declaration
                    .path[1]
                    .occurrence = 1
            }
            _ => {
                let parent = invalid.modules[invalid.root.index()].enumerations[enumeration]
                    .declaration
                    .path[0]
                    .clone();
                invalid.modules[invalid.root.index()].enumerations[enumeration]
                    .declaration
                    .path
                    .insert(0, parent.clone());
                invalid.modules[invalid.root.index()].enumerations[enumeration].variants[0]
                    .declaration
                    .path
                    .insert(0, parent);
            }
        }
        assert_eq!(
            verify_program(&invalid),
            Err(if mutation < 3 {
                BytecodeVerificationError::InvalidStructLayout
            } else {
                BytecodeVerificationError::InvalidEnumLayout
            })
        );
    }
}
