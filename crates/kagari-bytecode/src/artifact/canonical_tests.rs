use crate::{
    artifact::*,
    instruction::{BytecodeInstruction, PathId, Register},
    module::{BytecodeFunction, FunctionRecord},
    program::{BytecodeProgram, ModuleRef, verified::VerifiedBytecodeProgram},
};

use kagari_common::host_interface::path::HostPathSegmentDeclaration;

#[test]
fn consuming_artifact_verification_rejects_changed_code_and_envelope() {
    let artifact = KbcArtifact::from_program(
        BytecodeProgram {
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        },
        Default::default(),
    )
    .unwrap();
    let verified = artifact.clone().into_verified(&Default::default()).unwrap();
    assert_eq!(verified.bytecode().program().modules.len(), 1);
    assert!(verified.portable_mir().is_none());

    let mut changed = artifact.clone();
    changed.program.modules[0].source_name = "changed".into();
    assert!(matches!(
        changed.into_verified(&Default::default()),
        Err(ArtifactValidationError::ContentHashMismatch)
    ));
    let mut invalid = artifact.clone();
    invalid.program.root = ModuleRef::new(1);
    assert!(matches!(
        invalid.into_verified(&Default::default()),
        Err(ArtifactValidationError::Bytecode(_))
    ));
    let mut incompatible = artifact;
    incompatible.header.runtime_abi_version = "invalid".into();
    assert!(matches!(
        incompatible.into_verified(&Default::default()),
        Err(ArtifactValidationError::RuntimeAbiMismatch { .. })
    ));

    // Mutable extraction never carries the verification seal to the changed code.
    let mut extracted = verified.into_bytecode().into_unverified();
    extracted.root = ModuleRef::new(1);
    assert!(matches!(
        VerifiedBytecodeProgram::new(extracted),
        Err(ArtifactValidationError::Bytecode(_))
    ));
}

#[test]
fn bytecode_seal_rejects_in_memory_resource_exhaustion() {
    let oversized = BytecodeProgram {
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule::default(); MAX_ARTIFACT_MODULES + 1],
    };
    assert!(matches!(
        VerifiedBytecodeProgram::new(oversized),
        Err(ArtifactValidationError::ResourceLimit(_))
    ));
}

#[test]
fn memory_artifacts_reject_oversized_strings_before_fingerprinting() {
    let valid = BytecodeProgram {
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule::default()],
    };
    {
        let mut program = valid.clone();
        program.modules[0].source_name = "x".repeat(MAX_ARTIFACT_BYTES as usize);
        assert!(matches!(
            KbcArtifact::from_program(program, Default::default()),
            Err(ArtifactValidationError::ResourceLimit(
                "artifact encoded size limit exceeded"
            ))
        ));
    }
    let mut artifact = KbcArtifact::from_program(valid, Default::default()).unwrap();
    artifact.header.compiler_fingerprint = "x".repeat(MAX_ARTIFACT_BYTES as usize);
    assert!(matches!(
        artifact.validate_for_loader(&Default::default()),
        Err(ArtifactValidationError::ResourceLimit(
            "artifact encoded size limit exceeded"
        ))
    ));
    assert!(
        artifact
            .to_bytes()
            .unwrap_err()
            .message()
            .contains("artifact encoded size limit exceeded")
    );
}

#[test]
fn typed_path_operands_preflight_before_decoding_registers() {
    let instruction = BytecodeInstruction::ReadPath {
        dst: Register::new(0),
        root_or_view: Register::new(1),
        path: PathId::new(0),
        dynamic_args: vec![Register::new(2); MAX_ARTIFACT_NESTED_RECORDS + 1],
    };
    let bytes = codec().serialize(&instruction).unwrap();
    let error = codec()
        .deserialize::<BytecodeInstruction>(&bytes)
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("instruction operand count limit exceeded")
    );
}

#[test]
fn instruction_operand_vectors_are_bounded_before_verification() {
    use crate::{
        instruction::{BytecodeInstruction, Register},
        module::BytecodeFunction,
    };

    let valid = BytecodeProgram {
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule::default()],
    };
    let make_tuple = |count| BytecodeInstruction::MakeTuple {
        dst: Register::new(0),
        elements: vec![Register::new(0); count],
    };
    let mut oversized = valid.clone();
    let mut function = BytecodeFunction::default();
    function
        .instructions
        .push(make_tuple(MAX_ARTIFACT_NESTED_RECORDS + 1));
    oversized.modules[0].functions.push(function);
    assert!(matches!(
        KbcArtifact::from_program(oversized.clone(), Default::default()),
        Err(ArtifactValidationError::ResourceLimit(
            "instruction operand record limit exceeded"
        ))
    ));
    let mut artifact = KbcArtifact::from_program(valid.clone(), Default::default()).unwrap();
    artifact.program = oversized;
    assert!(matches!(
        artifact.validate_for_loader(&Default::default()),
        Err(ArtifactValidationError::ResourceLimit(
            "instruction operand record limit exceeded"
        ))
    ));
    assert!(artifact.to_bytes().is_err());
    let crafted = codec().serialize(&artifact).unwrap();
    assert!(
        KbcArtifact::from_bytes(&crafted)
            .unwrap_err()
            .message()
            .contains("instruction operand count limit exceeded")
    );

    let mut aggregate = valid;
    let function = BytecodeFunction {
        instructions: vec![
            make_tuple(MAX_ARTIFACT_NESTED_RECORDS);
            MAX_ARTIFACT_TABLE_RECORDS / MAX_ARTIFACT_NESTED_RECORDS + 1
        ],
        ..Default::default()
    };
    aggregate.modules[0].functions.push(function);
    assert!(matches!(
        KbcArtifact::from_program(aggregate, Default::default()),
        Err(ArtifactValidationError::ResourceLimit(
            "instruction operand aggregate limit exceeded"
        ))
    ));
}

#[test]
fn oversized_memory_identity_paths_reject_before_fingerprinting() {
    use kagari_abi::types::AbiType;
    use kagari_common::{
        host_interface::{
            HostInterface, host_type_identity, type_declaration::HostTypeDeclaration,
        },
        identity::MAX_IDENTITY_PATH_SEGMENTS,
    };

    let valid = BytecodeProgram {
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule::default()],
    };
    for case in 0..3 {
        let mut program = valid.clone();
        let reason = if case == 2 {
            "ABI type resource limit exceeded"
        } else {
            "identity path segment limit exceeded"
        };
        match case {
            0 => {
                program.modules[0].identity.path =
                    vec!["part".into(); MAX_IDENTITY_PATH_SEGMENTS + 1];
            }
            1 => {
                let mut host = HostTypeDeclaration::new("demo.Player");
                host.id.module.path = vec!["part".into(); MAX_IDENTITY_PATH_SEGMENTS + 1];
                program.modules[0].host_interface = HostInterface {
                    types: vec![host],
                    ..Default::default()
                };
            }
            _ => {
                let mut id = host_type_identity("demo.Player");
                id.module.path = vec!["part".into(); MAX_IDENTITY_PATH_SEGMENTS + 1];
                program.modules[0]
                    .public_items
                    .push(kagari_abi::types::PublicAbiItem::Const(
                        kagari_abi::types::ConstAbi {
                            name: "bad".into(),
                            ty: AbiType::Host(id),
                            value: "0".into(),
                        },
                    ));
            }
        }
        assert!(matches!(
            KbcArtifact::from_program(program.clone(), Default::default()),
            Err(ArtifactValidationError::ResourceLimit(found)) if found == reason
        ));
        let mut artifact = KbcArtifact::from_program(valid.clone(), Default::default()).unwrap();
        artifact.program = program;
        assert!(matches!(
            artifact.validate_for_loader(&Default::default()),
            Err(ArtifactValidationError::ResourceLimit(found)) if found == reason
        ));
        assert!(artifact.to_bytes().is_err());
    }
    let mut artifact = KbcArtifact::from_program(valid, Default::default()).unwrap();
    artifact.header.module_identity.path = vec!["part".into(); MAX_IDENTITY_PATH_SEGMENTS + 1];
    assert!(matches!(
        artifact.validate_for_loader(&Default::default()),
        Err(ArtifactValidationError::ResourceLimit(
            "identity path segment limit exceeded"
        ))
    ));
    assert!(artifact.to_bytes().is_err());
}

#[test]
fn nested_function_layout_tables_are_bounded_on_memory_and_wire_routes() {
    let valid = BytecodeProgram {
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule::default()],
    };
    let mut function_table = valid.clone();
    function_table.modules[0]
        .function_table
        .push(FunctionRecord {
            id: FunctionRef::new(0),
            identity: None,
            name: "oversized".into(),
            params: vec![ValueType::Unit; MAX_ARTIFACT_TABLE_RECORDS + 1],
            return_type: ValueType::Unit,
            effects: Default::default(),
        });
    let mut debug_frame = valid.clone();
    let mut function = BytecodeFunction::default();
    function.metadata.debug.frame_layout.params =
        vec![ValueType::Unit; MAX_ARTIFACT_TABLE_RECORDS + 1];
    debug_frame.modules[0].functions.push(function);
    for (program, reason) in [
        (
            function_table,
            "function table parameter record limit exceeded",
        ),
        (debug_frame, "function metadata record limit exceeded"),
    ] {
        assert!(matches!(
            KbcArtifact::from_program(program.clone(), Default::default()),
            Err(ArtifactValidationError::ResourceLimit(found)) if found == reason
        ));
        let mut artifact = KbcArtifact::from_program(valid.clone(), Default::default()).unwrap();
        artifact.program = program;
        assert!(matches!(
            artifact.validate_for_loader(&Default::default()),
            Err(ArtifactValidationError::ResourceLimit(found)) if found == reason
        ));
        assert!(artifact.to_bytes().is_err());
        let crafted = codec().serialize(&artifact).unwrap();
        assert!(
            KbcArtifact::from_bytes(&crafted)
                .unwrap_err()
                .message()
                .contains("artifact table count limit exceeded")
        );
    }
}

#[test]
fn detached_debug_frame_layout_is_bounded_before_fingerprinting() {
    let valid = BytecodeProgram {
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule::default()],
    };
    let mut debug = BytecodeDebugMetadata::default();
    debug.frame_layout.locals = vec![ValueType::Unit; MAX_ARTIFACT_TABLE_RECORDS + 1];
    let metadata = DebugMetadata {
        stripped: false,
        source_files: Vec::new(),
        debug_names: Vec::new(),
        functions: vec![debug],
    };
    assert!(matches!(
        KbcArtifact::from_program(
            valid.clone(),
            ArtifactBuildOptions {
                debug: Some(metadata.clone()),
                ..Default::default()
            }
        ),
        Err(ArtifactValidationError::ResourceLimit(
            "debug record limit exceeded"
        ))
    ));
    let mut artifact = KbcArtifact::from_program(valid, Default::default()).unwrap();
    artifact.debug = Some(metadata);
    assert!(matches!(
        artifact.validate_for_loader(&Default::default()),
        Err(ArtifactValidationError::ResourceLimit(
            "debug record limit exceeded"
        ))
    ));
    assert!(artifact.to_bytes().is_err());
    let crafted = codec().serialize(&artifact).unwrap();
    assert!(
        KbcArtifact::from_bytes(&crafted)
            .unwrap_err()
            .message()
            .contains("artifact table count limit exceeded")
    );
}

#[test]
fn decoder_rejects_forged_header_identity_path_length() {
    let artifact = KbcArtifact::from_program(
        BytecodeProgram {
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        },
        Default::default(),
    )
    .unwrap();
    let mut bytes = artifact.to_bytes().unwrap();
    let header = &artifact.header;
    let offset = codec()
        .serialized_size(&(
            &header.magic,
            &header.format_version,
            &header.language_version,
            &header.compiler_fingerprint,
            &header.runtime_abi_version,
            &header.runtime_helper_abi_version,
            &header.encoding,
            &header.module_identity.package,
        ))
        .unwrap() as usize;
    bytes[offset..offset + 8].copy_from_slice(&u64::MAX.to_le_bytes());
    let error = KbcArtifact::from_bytes(&bytes).unwrap_err();
    assert!(
        error
            .message()
            .contains("module identity path segment count limit exceeded")
    );
}

#[test]
fn debug_table_length_is_rejected_before_decoding_source_names() {
    let debug = DebugMetadata {
        stripped: false,
        source_files: Vec::new(),
        debug_names: Vec::new(),
        functions: Vec::new(),
    };
    let mut bytes = codec().serialize(&debug).unwrap();
    bytes[1..9].copy_from_slice(&u64::MAX.to_le_bytes());
    let error = codec().deserialize::<DebugMetadata>(&bytes).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("artifact table count limit exceeded")
    );
}

#[test]
fn decoder_rejects_huge_module_count_before_reading_module_data() {
    let artifact = KbcArtifact::from_program(
        BytecodeProgram {
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        },
        Default::default(),
    )
    .unwrap();
    let mut bytes = artifact.to_bytes().unwrap();
    let offset = (codec().serialized_size(&artifact.header).unwrap()
        + codec().serialized_size(&artifact.program.root).unwrap()) as usize;
    bytes[offset..offset + 8].copy_from_slice(&u64::MAX.to_le_bytes());
    let error = KbcArtifact::from_bytes(&bytes).unwrap_err();
    assert!(error.message().contains("module count limit exceeded"));
}

#[test]
fn deep_abi_types_are_rejected_before_artifact_fingerprinting() {
    use kagari_abi::{scalar::BuiltinType, types::AbiType};
    let valid = BytecodeProgram {
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule::default()],
    };
    let mut deep = AbiType::Builtin(BuiltinType::I32);
    for _ in 0..64 {
        deep = AbiType::Array(Box::new(deep), CollectionAccess::Mutable);
    }
    let mut program = valid.clone();
    program.modules[0]
        .public_items
        .push(kagari_abi::types::PublicAbiItem::Const(
            kagari_abi::types::ConstAbi {
                name: "deep".into(),
                ty: deep,
                value: "0".into(),
            },
        ));
    assert!(matches!(
        KbcArtifact::from_program(program.clone(), Default::default()),
        Err(ArtifactValidationError::ResourceLimit(
            "ABI type resource limit exceeded"
        ))
    ));
    let mut artifact = KbcArtifact::from_program(valid, Default::default()).unwrap();
    artifact.program = program;
    assert!(matches!(
        artifact.validate_for_loader(&Default::default()),
        Err(ArtifactValidationError::ResourceLimit(
            "ABI type resource limit exceeded"
        ))
    ));
    assert!(artifact.to_bytes().is_err());
}

#[test]
fn nested_layout_and_host_path_counts_are_bounded_on_all_artifact_routes() {
    use kagari_abi::{
        layout::{EnumLayout, EnumVariantLayout, StructFieldLayout, StructLayout},
        scalar::BuiltinType,
        types::{AbiType, FieldAbi},
    };
    use kagari_common::{
        host_interface::{path::HostPathDeclaration, type_declaration::PathAccess},
        identity::{DefinitionId, DefinitionKind, DefinitionPathSegment},
    };

    let valid = BytecodeProgram {
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule::default()],
    };
    let id = |kind| DefinitionId {
        module: valid.modules[0].identity.clone(),
        path: vec![DefinitionPathSegment {
            kind,
            name: "item".into(),
            occurrence: 0,
        }],
    };
    let field_id = id(DefinitionKind::Field);
    let mut cases = Vec::new();

    let mut structure = valid.clone();
    structure.modules[0].structures.push(StructLayout {
        declaration: id(DefinitionKind::Struct),
        arguments: Vec::new(),
        fields: vec![
            StructFieldLayout {
                declaration: field_id.clone(),
                name: "field".into(),
                ty: AbiType::Builtin(BuiltinType::I32),
                mutable: false,
            };
            MAX_ARTIFACT_NESTED_RECORDS + 1
        ],
    });
    cases.push(structure);

    let mut enumeration = valid.clone();
    enumeration.modules[0].enumerations.push(EnumLayout {
        declaration: id(DefinitionKind::Enum),
        arguments: Vec::new(),
        variants: vec![EnumVariantLayout {
            declaration: id(DefinitionKind::Variant),
            payload: vec![AbiType::Builtin(BuiltinType::I32); MAX_ARTIFACT_NESTED_RECORDS + 1],
        }],
    });
    cases.push(enumeration);

    let mut host_path = valid.clone();
    host_path.modules[0]
        .host_interface
        .paths
        .push(HostPathDeclaration {
            root: id(DefinitionKind::Struct),
            segments: vec![
                HostPathSegmentDeclaration::Field(field_id.clone());
                MAX_ARTIFACT_NESTED_RECORDS + 1
            ],
            access: PathAccess::ReadOnly,
            schema_epoch: 0,
        });
    cases.push(host_path);

    let mut public_abi = valid.clone();
    public_abi.modules[0]
        .public_items
        .push(kagari_abi::types::PublicAbiItem::Type(
            kagari_abi::types::TypeAbi {
                name: "item".into(),
                kind: kagari_abi::types::TypeAbiKind::Struct,
                generic_params: Vec::new(),
                bounds: Vec::new(),
                fields: vec![
                    FieldAbi {
                        name: "field".into(),
                        ty: AbiType::Builtin(BuiltinType::I32),
                        mutable: false,
                    };
                    MAX_ARTIFACT_NESTED_RECORDS + 1
                ],
                variants: Vec::new(),
            },
        ));
    cases.push(public_abi);

    for (index, program) in cases.into_iter().enumerate() {
        assert!(matches!(
            KbcArtifact::from_program(program.clone(), Default::default()),
            Err(ArtifactValidationError::ResourceLimit(
                "nested module record limit exceeded"
            ))
        ));
        let mut artifact = KbcArtifact::from_program(valid.clone(), Default::default()).unwrap();
        artifact.program = program;
        assert!(matches!(
            artifact.validate_for_loader(&Default::default()),
            Err(ArtifactValidationError::ResourceLimit(
                "nested module record limit exceeded"
            ))
        ));
        assert!(artifact.to_bytes().is_err());
        let crafted = codec().serialize(&artifact).unwrap();
        let error = KbcArtifact::from_bytes(&crafted).unwrap_err();
        let expected = if index == 2 {
            "host member count limit exceeded"
        } else {
            "nested declaration count limit exceeded"
        };
        assert!(error.message().contains(expected), "{index}: {error}");
    }
}

#[test]
fn module_count_limit_rejects_memory_and_encoded_artifacts_before_verification() {
    let valid = BytecodeProgram {
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule::default()],
    };
    let mut excessive = valid.clone();
    excessive
        .modules
        .resize(MAX_ARTIFACT_MODULES + 1, BytecodeModule::default());
    assert!(matches!(
        KbcArtifact::from_program(excessive.clone(), Default::default()),
        Err(ArtifactValidationError::ResourceLimit("too many modules"))
    ));
    let mut artifact = KbcArtifact::from_program(valid, Default::default()).unwrap();
    artifact.program = excessive;
    assert!(matches!(
        artifact.validate_for_loader(&Default::default()),
        Err(ArtifactValidationError::ResourceLimit("too many modules"))
    ));
    assert!(
        artifact
            .to_bytes()
            .unwrap_err()
            .message()
            .contains("too many modules")
    );
    let crafted = codec().serialize(&artifact).unwrap();
    assert!(
        KbcArtifact::from_bytes(&crafted)
            .unwrap_err()
            .message()
            .contains("module count limit exceeded")
    );
}

#[test]
fn declared_section_counts_are_bounded_independently_of_payload_size() {
    let mut artifact = KbcArtifact::from_program(
        BytecodeProgram {
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        },
        Default::default(),
    )
    .unwrap();
    artifact.tables.sections[0].record_count = MAX_ARTIFACT_TABLE_RECORDS + 1;
    assert!(matches!(
        artifact.validate_for_loader(&Default::default()),
        Err(ArtifactValidationError::ResourceLimit(
            "artifact metadata record limit exceeded"
        ))
    ));
    assert!(artifact.to_bytes().is_err());
    let crafted = codec().serialize(&artifact).unwrap();
    assert!(KbcArtifact::from_bytes(&crafted).is_err());
}

#[test]
fn recomputed_hash_cannot_hide_inconsistent_artifact_tables() {
    let artifact = KbcArtifact::from_program(
        BytecodeProgram {
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        },
        Default::default(),
    )
    .unwrap();
    assert!(artifact.validate_for_loader(&Default::default()).is_ok());
    for change in 0..5 {
        let mut changed = artifact.clone();
        match change {
            0 => changed.tables.sections[0].record_count += 1,
            1 => changed.tables.sections[0].fingerprint = ArtifactFingerprint(0),
            2 => changed.tables.sections.swap(0, 1),
            3 => {
                changed.tables.sections.pop();
            }
            _ => changed.tables.source_files.push("forged.kgr".into()),
        }
        changed.header.content_hash = changed.compute_content_hash();
        assert!(
            matches!(
                changed.validate_for_loader(&Default::default()),
                Err(ArtifactValidationError::TableMismatch)
            ),
            "change {change}"
        );
    }
}

#[test]
fn invalid_host_types_are_rejected_before_fingerprinting_memory_artifacts() {
    use kagari_common::host_interface::{HostFunctionDeclaration, value_type::HostValueType};
    let valid = BytecodeProgram {
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule::default()],
    };
    let mut deep = HostValueType::I32;
    for _ in 0..64 {
        deep = HostValueType::Array(Box::new(deep), CollectionAccess::Mutable);
    }
    for ty in [
        HostValueType::Set(Box::new(HostValueType::F32), CollectionAccess::Mutable),
        deep,
    ] {
        let mut program = valid.clone();
        program.modules[0]
            .host_interface
            .functions
            .push(HostFunctionDeclaration::new("host.bad", vec![], ty));
        assert!(matches!(
            VerificationMetadata::from_program(&program, &Default::default()),
            Err(ArtifactValidationError::Bytecode(_))
        ));
        assert!(matches!(
            KbcArtifact::from_program(program.clone(), Default::default()),
            Err(ArtifactValidationError::Bytecode(_))
        ));
        let mut artifact = KbcArtifact::from_program(valid.clone(), Default::default()).unwrap();
        artifact.program = program;
        assert!(matches!(
            artifact.validate_for_loader(&Default::default()),
            Err(ArtifactValidationError::Bytecode(_))
        ));
    }
}

#[test]
fn bytecode_cannot_claim_a_different_identity_from_its_header() {
    let mut artifact = KbcArtifact::from_program(
        BytecodeProgram {
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        },
        Default::default(),
    )
    .unwrap();
    artifact.program.modules[artifact.program.root.index()].identity =
        ModuleIdentity::single_file("forged.kgr");
    artifact.header.content_hash = artifact.compute_content_hash();
    assert!(matches!(
        artifact.validate_for_loader(&Default::default()),
        Err(ArtifactValidationError::ModuleIdentityMismatch { .. })
    ));
}

#[test]
fn legacy_language_semantics_cannot_be_opted_into() {
    let mut artifact = KbcArtifact::from_program(
        BytecodeProgram {
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        },
        Default::default(),
    )
    .unwrap();
    artifact.header.language_version = "0.1.0".into();
    let requirements = ArtifactCompatibility {
        language_version: "0.1.0".into(),
        ..Default::default()
    };
    assert!(matches!(
        artifact.validate_for_loader(&requirements),
        Err(ArtifactValidationError::LanguageVersionMismatch { .. })
    ));
}

#[test]
fn fingerprints_depend_on_serialized_values_not_rust_debug_names() {
    #[derive(Debug, Serialize)]
    struct First(u32);
    #[derive(Debug, Serialize)]
    struct Renamed(u32);
    assert_eq!(
        ArtifactFingerprint::of_serialized(&First(42)),
        ArtifactFingerprint::of_serialized(&Renamed(42))
    );
    assert_ne!(
        ArtifactFingerprint::of_serialized(&First(42)),
        ArtifactFingerprint::of_serialized(&First(43))
    );
}

#[test]
fn decoder_rejects_old_versions_trailing_bytes_and_oversized_lengths() {
    let artifact = KbcArtifact::from_program(
        BytecodeProgram {
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    let bytes = artifact.to_bytes().unwrap();
    assert!(KbcArtifact::from_bytes(&bytes).is_ok());
    for version in 1..KBC_ARTIFACT_FORMAT_VERSION {
        let mut old = bytes.clone();
        old[4..6].copy_from_slice(&version.to_le_bytes());
        assert!(KbcArtifact::from_bytes(&old).is_err());
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(KbcArtifact::from_bytes(&trailing).is_err());
    let mut enormous_string = bytes;
    enormous_string[6..14].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(KbcArtifact::from_bytes(&enormous_string).is_err());
}

#[test]
fn artifact_preserves_portable_virtual_path_declarations() {
    use kagari_common::host_interface::{
        path::{HostPathDeclaration, HostPathSegmentDeclaration, HostVirtualSegmentDeclaration},
        type_declaration::{HostTypeDeclaration, HostTypeOwnership, PathAccess},
        value_type::HostValueType,
    };
    let mut root = HostTypeDeclaration::new("game.Player");
    root.ownership = HostTypeOwnership::HostRoot;
    root.path_access = PathAccess::ReadOnly;
    let path = HostPathDeclaration {
        root: root.id.clone(),
        segments: vec![HostPathSegmentDeclaration::Virtual(
            HostVirtualSegmentDeclaration {
                name: "preview".into(),
                result: HostValueType::I32,
                access: PathAccess::ReadOnly,
            },
        )],
        access: PathAccess::ReadOnly,
        schema_epoch: 1,
    };
    let mut module = BytecodeModule::default();
    module.host_interface.types.push(root);
    module.host_interface.paths.push(path.clone());
    let artifact = KbcArtifact::from_program(
        BytecodeProgram {
            root: ModuleRef::new(0),
            modules: vec![module],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    assert_eq!(decoded.program.modules[0].host_interface.paths, vec![path]);
}

#[test]
fn required_host_fingerprint_is_derived_and_independent_of_docs_and_order() {
    use kagari_common::host_interface::{
        HostFunctionDeclaration, HostInterface, standard_log, value_type::HostValueType,
    };
    let interface = HostInterface {
        paths: vec![],
        types: Vec::new(),
        functions: vec![
            standard_log(),
            HostFunctionDeclaration::new("host.other", vec![], HostValueType::Unit),
        ],
    };
    let fingerprint = ArtifactFingerprint::of_host_interface(&interface);
    let mut reordered = interface.clone();
    reordered.functions.reverse();
    reordered.functions[0].documentation = "different docs".into();
    assert_eq!(
        fingerprint,
        ArtifactFingerprint::of_host_interface(&reordered)
    );
    reordered.functions[0].effects.may_trap = true;
    assert_ne!(
        fingerprint,
        ArtifactFingerprint::of_host_interface(&reordered)
    );
    let mut artifact = KbcArtifact::from_program(
        BytecodeProgram {
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule {
                host_interface: interface,
                ..Default::default()
            }],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    assert_eq!(
        artifact.verification.host_interface_fingerprint,
        ArtifactFingerprint::of_program_hosts(&artifact.program)
    );
    artifact.verification.host_interface_fingerprint = ArtifactFingerprint::empty();
    artifact.header.content_hash = artifact.compute_content_hash();
    assert!(matches!(
        artifact.validate_for_loader(&ArtifactCompatibility::default()),
        Err(ArtifactValidationError::HostInterfaceFingerprintMismatch { .. })
    ));
}

#[test]
fn header_metadata_is_covered_and_old_format_cannot_be_opted_into() {
    let mut artifact = KbcArtifact::from_program(
        BytecodeProgram {
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    artifact
        .header
        .module_identity
        .package
        .0
        .push_str("changed");
    assert!(matches!(
        artifact.validate_for_loader(&ArtifactCompatibility::default()),
        Err(ArtifactValidationError::ContentHashMismatch)
    ));
    artifact.header.format_version = 1;
    let requirements = ArtifactCompatibility {
        format_version: 1,
        ..ArtifactCompatibility::default()
    };
    assert!(matches!(
        artifact.validate_for_loader(&requirements),
        Err(ArtifactValidationError::FormatVersionMismatch { .. })
    ));
}

#[test]
fn recomputed_checksum_cannot_hide_dependency_or_verification_metadata_changes() {
    use crate::program::ModuleRef;
    let program = BytecodeProgram {
        root: ModuleRef::new(1),
        modules: vec![
            BytecodeModule {
                identity: ModuleIdentity::single_file("dependency"),
                ..Default::default()
            },
            BytecodeModule {
                identity: ModuleIdentity::single_file("root"),
                dependencies: vec![ModuleRef::new(0)],
                ..Default::default()
            },
        ],
    };
    let original = KbcArtifact::from_program(program, Default::default()).unwrap();
    original.validate_for_loader(&Default::default()).unwrap();
    let mut artifact = original.clone();
    artifact.program.modules[0].source_name.push_str("changed");
    artifact.header.content_hash = artifact.compute_content_hash();
    assert!(matches!(
        artifact.validate_for_loader(&Default::default()),
        Err(ArtifactValidationError::DependencyFingerprintMismatch)
    ));
    let mut artifact = original;
    artifact
        .verification
        .public_abi_fingerprints
        .push(PublicAbiFingerprint {
            name: "invented".into(),
            fingerprint: ArtifactFingerprint::empty(),
        });
    artifact.verification.loader.public_abi_fingerprints =
        artifact.verification.public_abi_fingerprints.clone();
    artifact.header.content_hash = artifact.compute_content_hash();
    assert!(matches!(
        artifact.validate_for_loader(&Default::default()),
        Err(ArtifactValidationError::VerificationMetadataMismatch)
    ));
}

#[test]
fn portable_mir_is_opaque_but_integrity_and_manifest_bound() {
    let program = BytecodeProgram {
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule::default()],
    };
    let plain = KbcArtifact::from_program(program.clone(), Default::default()).unwrap();
    let artifact = KbcArtifact::from_program(
        program,
        ArtifactBuildOptions {
            portable_mir: Some(PortableMir {
                // Bytecode loading deliberately does not decode compiler input.
                bytes: vec![1, 2, 3],
            }),
            ..Default::default()
        },
    )
    .unwrap();
    assert_ne!(plain.header.content_hash, artifact.header.content_hash);
    assert!(plain.portable_mir.is_none());
    assert!(
        !plain
            .tables
            .sections
            .iter()
            .any(|item| item.id == ArtifactSectionId::PortableMir)
    );
    let bytes = artifact.to_bytes().unwrap();
    let decoded = KbcArtifact::from_bytes(&bytes).unwrap();
    decoded.validate_for_loader(&Default::default()).unwrap();
    assert_eq!(decoded.to_bytes().unwrap(), bytes);
    assert_eq!(decoded.portable_mir.unwrap().bytes, [1, 2, 3]);

    let mut changed = artifact.clone();
    changed.portable_mir.as_mut().unwrap().bytes[0] = 4;
    assert!(matches!(
        changed.validate_for_loader(&Default::default()),
        Err(ArtifactValidationError::ContentHashMismatch)
    ));
    // Even a refreshed outer checksum cannot hide a stale section fingerprint.
    changed.header.content_hash = changed.compute_content_hash();
    assert!(matches!(
        changed.validate_for_loader(&Default::default()),
        Err(ArtifactValidationError::TableMismatch)
    ));
    changed.portable_mir = None;
    changed.header.content_hash = changed.compute_content_hash();
    assert!(matches!(
        changed.validate_for_loader(&Default::default()),
        Err(ArtifactValidationError::TableMismatch)
    ));

    let mut previous = bytes;
    previous[4..6].copy_from_slice(&102u16.to_le_bytes());
    assert!(
        KbcArtifact::from_bytes(&previous)
            .unwrap_err()
            .message()
            .contains("format version")
    );
}

#[test]
fn portable_mir_bounds_apply_before_decoding_and_to_combined_envelope_size() {
    let error = codec()
        .deserialize::<PortableMir>(&u64::MAX.to_le_bytes())
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("portable MIR byte limit exceeded")
    );
    let program = BytecodeProgram {
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule::default()],
    };
    let mut artifact = KbcArtifact::from_program(program.clone(), Default::default()).unwrap();
    artifact.portable_mir = Some(PortableMir {
        bytes: vec![0; MAX_ARTIFACT_BYTES as usize + 1],
    });
    assert!(matches!(
        artifact.validate_for_loader(&Default::default()),
        Err(ArtifactValidationError::ResourceLimit(
            "portable MIR byte limit exceeded"
        ))
    ));
    assert!(
        artifact
            .to_bytes()
            .unwrap_err()
            .message()
            .contains("portable MIR byte limit exceeded")
    );
    let mut payload = artifact.portable_mir.take().unwrap();
    assert!(matches!(
        KbcArtifact::from_program(
            program.clone(),
            ArtifactBuildOptions {
                portable_mir: Some(payload.clone()),
                ..Default::default()
            }
        ),
        Err(ArtifactValidationError::ResourceLimit(
            "portable MIR byte limit exceeded"
        ))
    ));
    payload.bytes.pop();
    // The payload alone fits; the complete envelope must also fit the same budget.
    assert!(matches!(
        KbcArtifact::from_program(
            program,
            ArtifactBuildOptions {
                portable_mir: Some(payload),
                ..Default::default()
            }
        ),
        Err(ArtifactValidationError::ResourceLimit(
            "artifact encoded size limit exceeded"
        ))
    ));
}
