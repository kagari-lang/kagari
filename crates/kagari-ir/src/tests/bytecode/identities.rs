use super::*;

#[test]
fn executable_function_identities_survive_lowering_and_reject_mismatched_records() {
    let module = common::bytecode_ok(
        "fn id<T>(value: T) -> T { value } fn other() -> i32 { 2 } fn main() -> i32 { id(1) + other() }",
    );
    assert!(module.functions.iter().all(|function| {
        function.identity.is_some()
            && module.function_table[function.id.index()].identity == function.identity
    }));
    let generic = module
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
        module.functions[generic]
            .identity
            .as_ref()
            .unwrap()
            .arguments,
        [crate::module::abi::AbiType::Builtin(
            kagari_abi::scalar::BuiltinType::I32
        )]
    );
    let mut mismatched_record = module.clone();
    mismatched_record.function_table[generic].identity = None;
    assert!(matches!(
        verify_module(&mismatched_record),
        Err(BytecodeVerificationError::FunctionRecordMismatch { .. })
    ));
    let mut duplicate = module.clone();
    let other = duplicate
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
    duplicate.functions[other].identity = duplicate.functions[generic].identity.clone();
    duplicate.function_table[other].identity = duplicate.functions[other].identity.clone();
    assert!(matches!(
        verify_module(&duplicate),
        Err(BytecodeVerificationError::InvalidFunctionIdentity { .. })
    ));
    let mut foreign = module.clone();
    foreign.functions[generic]
        .identity
        .as_mut()
        .unwrap()
        .declaration
        .module
        .package
        .0 = "foreign".into();
    foreign.function_table[generic].identity = foreign.functions[generic].identity.clone();
    assert!(matches!(
        verify_module(&foreign),
        Err(BytecodeVerificationError::InvalidFunctionIdentity { .. })
    ));
    let mut wrong_kind = module.clone();
    wrong_kind.functions[generic]
        .identity
        .as_mut()
        .unwrap()
        .declaration
        .path
        .last_mut()
        .unwrap()
        .kind = kagari_common::identity::DefinitionKind::Struct;
    wrong_kind.function_table[generic].identity = wrong_kind.functions[generic].identity.clone();
    assert!(matches!(
        verify_module(&wrong_kind),
        Err(BytecodeVerificationError::InvalidFunctionIdentity { .. })
    ));
    let mut oversized = module.clone();
    oversized.functions[generic]
        .identity
        .as_mut()
        .unwrap()
        .arguments = vec![
        crate::module::abi::AbiType::Builtin(kagari_abi::scalar::BuiltinType::I32);
        kagari_abi::decode_limits::MAX_NESTED_RECORDS + 1
    ];
    oversized.function_table[generic].identity = oversized.functions[generic].identity.clone();
    assert!(matches!(
        KbcArtifact::from_program(
            crate::bytecode::BytecodeProgram {
                root: crate::bytecode::ModuleRef::new(0),
                modules: vec![oversized],
            },
            Default::default(),
        ),
        Err(ArtifactValidationError::ResourceLimit(
            "nested module record limit exceeded"
        ))
    ));
    let mut unresolved = module;
    unresolved.functions[generic]
        .identity
        .as_mut()
        .unwrap()
        .arguments[0] = crate::module::abi::AbiType::Parameter {
        owner: unresolved.function_table[generic]
            .identity
            .as_ref()
            .unwrap()
            .declaration
            .clone(),
        position: 0,
    };
    unresolved.function_table[generic].identity = unresolved.functions[generic].identity.clone();
    assert!(matches!(
        verify_module(&unresolved),
        Err(BytecodeVerificationError::InvalidFunctionIdentity { .. })
    ));
}

#[test]
fn artifact_loader_rejects_invalid_struct_layouts_slots_and_initializers() {
    let original = common::bytecode_ok(
        "struct P { var x: i32, val fixed: bool } fn main() -> i32 { val p = P { fixed: true, x: 1 }; p.x = 2; p.x }",
    );
    let valid = KbcArtifact::from_program(
        crate::bytecode::BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![original.clone()],
        },
        Default::default(),
    )
    .unwrap();
    for corruption in 0..7 {
        let mut module = original.clone();
        match corruption {
            0 => module.structures.push(module.structures[0].clone()),
            1 => module.structures[0].fields[0]
                .declaration
                .module
                .path
                .push("foreign".into()),
            2 => module.structures[0].fields[0].mutable = false,
            _ => {
                for instruction in module
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
            KbcArtifact::from_program(
                crate::bytecode::BytecodeProgram {
                    root: crate::bytecode::ModuleRef::new(0),
                    modules: vec![module.clone()],
                },
                ArtifactBuildOptions::default(),
            )
            .is_err()
        );
        let mut corrupted = valid.clone();
        corrupted.program.modules[0] = module;
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
    use crate::module::abi::AbiType;
    use crate::module::abi::NominalAbiType;
    use kagari_abi::scalar::BuiltinType;
    let module = common::bytecode_ok(
        "struct Box<T> { val value: T } fn main() -> i32 { Box<i32> { value: 42 }.value }",
    );
    let declaration = module.structures[0].declaration.clone();
    for ty in [
        AbiType::Parameter {
            owner: declaration.clone(),
            position: 0,
        },
        AbiType::Struct(NominalAbiType {
            associated_types: Default::default(),
            declaration,
            arguments: vec![AbiType::Builtin(BuiltinType::Bool)],
        }),
        AbiType::Builtin(BuiltinType::Bool),
    ] {
        let mut invalid = module.clone();
        invalid.structures[0].fields[0].ty = ty;
        assert!(verify_module(&invalid).is_err());
    }
}

#[test]
fn struct_instances_must_match_public_templates_locally_and_across_modules() {
    use crate::bytecode::BytecodeProgram;
    use crate::bytecode::ModuleRef;
    use crate::bytecode::verify_program;
    use crate::module::abi::AbiType;
    use kagari_abi::scalar::BuiltinType;
    let owner = common::bytecode_ok(
        "pub struct Box<T> { var values: ArrayList<T> } fn main() -> i32 { Box<i32> { values: [42] }.values[0] }",
    );
    let mut importer = BytecodeModule {
        identity: ModuleIdentity::single_file("importer.kgr"),
        structures: owner.structures.clone(),
        dependencies: vec![ModuleRef::new(0)],
        ..Default::default()
    };
    let program = |importer| {
        let declaration_owner = BytecodeModule {
            identity: owner.identity.clone(),
            public_items: owner.public_items.clone(),
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
                invalid.structures[0].fields[0].ty = AbiType::Array(
                    Box::new(AbiType::Builtin(BuiltinType::Bool)),
                    CollectionAccess::Mutable,
                )
            }
            1 => invalid.structures[0].fields[0].mutable = false,
            2 => {
                invalid.structures[0].fields[0].name = "other".into();
                invalid.structures[0].fields[0]
                    .declaration
                    .path
                    .last_mut()
                    .unwrap()
                    .name = "other".into();
            }
            _ => invalid.structures[0].fields.clear(),
        }
        assert_eq!(
            verify_module(&invalid),
            Err(BytecodeVerificationError::InvalidStructLayout)
        );
        importer.structures = invalid.structures;
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
    for mutation in 0..6 {
        let mut invalid = module.clone();
        match mutation {
            0 => {
                invalid.structures[0].declaration.path[0].occurrence = 1;
                invalid.structures[0].fields[0].declaration.path[0].occurrence = 1;
            }
            1 => invalid.structures[0].fields[0].declaration.path[1].occurrence = 1,
            2 => {
                let parent = invalid.structures[0].declaration.path[0].clone();
                invalid.structures[0]
                    .declaration
                    .path
                    .insert(0, parent.clone());
                invalid.structures[0].fields[0]
                    .declaration
                    .path
                    .insert(0, parent);
            }
            3 => {
                invalid.enumerations[0].declaration.path[0].occurrence = 1;
                invalid.enumerations[0].variants[0].declaration.path[0].occurrence = 1;
            }
            4 => invalid.enumerations[0].variants[0].declaration.path[1].occurrence = 1,
            _ => {
                let parent = invalid.enumerations[0].declaration.path[0].clone();
                invalid.enumerations[0]
                    .declaration
                    .path
                    .insert(0, parent.clone());
                invalid.enumerations[0].variants[0]
                    .declaration
                    .path
                    .insert(0, parent);
            }
        }
        assert_eq!(
            verify_module(&invalid),
            Err(if mutation < 3 {
                BytecodeVerificationError::InvalidStructLayout
            } else {
                BytecodeVerificationError::InvalidEnumLayout
            })
        );
    }
}
