use crate::{
    artifact::*,
    instruction::{BytecodeInstruction, PathId, Register},
    module::{BytecodeFunction, FunctionRecord},
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, MAX_IDENTITY_PATH_SEGMENTS,
    map::DefinitionContext, mapping::DefinitionMappingError, table::DefinitionTableError,
};
use kagari_contract::{
    layout::{EnumLayout, EnumVariantLayout, StructFieldLayout, StructLayout},
    types::{PublicItem, TraitContract},
};

use kagari_types::{
    collection::CollectionAccess,
    declaration::{
        AssociatedConstDef, ConstDef, FieldDef, TraitDef, TypeDef, TypeDefKind,
        host::PathAccess,
        path::{HostPathDeclaration, HostPathSegmentDeclaration},
    },
    host_interface::{HostFunctionDeclaration, host_type_identity, value_type::HostValueType},
    scalar::BuiltinType,
    ty::Ty,
    visibility::Visibility,
};

#[test]
fn typed_path_operands_preflight_before_decoding_registers() {
    let context = DefinitionContext::new().unwrap();
    let instruction: BytecodeInstruction = BytecodeInstruction::ReadPath {
        dst: Register::new(0),
        root_or_view: Register::new(1),
        path: PathId::new(0),
        dynamic_args: vec![Register::new(2); MAX_ARTIFACT_NESTED_RECORDS + 1],
    };
    let mut artifact = KbcArtifact::from_program(
        BytecodeProgram {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        },
        Default::default(),
        &context,
    )
    .unwrap();
    artifact.program.modules[0]
        .functions
        .push(BytecodeFunction {
            instructions: vec![instruction],
            ..Default::default()
        });
    let bytes = codec()
        .serialize(
            &wire::encode_artifact(&artifact, &context.snapshot(), &Default::default()).unwrap(),
        )
        .unwrap();
    let error = KbcArtifact::from_bytes(&bytes).unwrap_err();
    assert!(
        error
            .message()
            .contains("instruction operand count limit exceeded")
    );
}

#[test]
fn instruction_operand_vectors_are_bounded_before_verification() {
    let context = DefinitionContext::new().unwrap();

    let valid = BytecodeProgram {
        exports: Default::default(),
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
        KbcArtifact::from_program(oversized.clone(), Default::default(), &context),
        Err(ArtifactValidationError::ResourceLimit(
            "instruction operand record limit exceeded"
        ))
    ));
    let mut artifact =
        KbcArtifact::from_program(valid.clone(), Default::default(), &context).unwrap();
    artifact.program = oversized;
    assert!(matches!(
        artifact.validate_for_loader(&Default::default(), &context),
        Err(ArtifactValidationError::ResourceLimit(
            "instruction operand record limit exceeded"
        ))
    ));
    assert!(artifact.to_bytes(&context.snapshot()).is_err());
    let crafted = codec()
        .serialize(
            &wire::encode_artifact(&artifact, &context.snapshot(), &Default::default()).unwrap(),
        )
        .unwrap();
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
        KbcArtifact::from_program(aggregate, Default::default(), &context),
        Err(ArtifactValidationError::ResourceLimit(
            "instruction operand aggregate limit exceeded"
        ))
    ));
}

#[test]
fn oversized_memory_identity_paths_reject_before_fingerprinting() {
    let context = DefinitionContext::new().unwrap();

    let valid = BytecodeProgram {
        exports: Default::default(),
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule::default()],
    };
    let mut oversized_identity = valid.modules[0].identity.clone();
    oversized_identity.path = vec!["part".into(); MAX_IDENTITY_PATH_SEGMENTS + 1];
    assert_eq!(
        context.intern_root(&oversized_identity),
        Err(DefinitionTableError::PathLimit)
    );
    let mut host = host_type_identity("demo.Player");
    host.module.path = oversized_identity.path.clone();
    assert_eq!(context.intern(&host), Err(DefinitionTableError::PathLimit));
    let mut program = valid.clone();
    program.modules[0].identity = oversized_identity;
    assert!(matches!(
        KbcArtifact::from_program(program.clone(), Default::default(), &context),
        Err(ArtifactValidationError::ResourceLimit(
            "identity path segment limit exceeded"
        ))
    ));
    let mut artifact =
        KbcArtifact::from_program(valid.clone(), Default::default(), &context).unwrap();
    artifact.program = program;
    assert!(matches!(
        artifact.validate_for_loader(&Default::default(), &context),
        Err(ArtifactValidationError::ResourceLimit(
            "identity path segment limit exceeded"
        ))
    ));
    assert!(artifact.to_bytes(&context.snapshot()).is_err());
    let mut artifact = KbcArtifact::from_program(valid, Default::default(), &context).unwrap();
    artifact.header.module_identity.path = vec!["part".into(); MAX_IDENTITY_PATH_SEGMENTS + 1];
    assert!(matches!(
        artifact.validate_for_loader(&Default::default(), &context),
        Err(ArtifactValidationError::ResourceLimit(
            "identity path segment limit exceeded"
        ))
    ));
    assert!(artifact.to_bytes(&context.snapshot()).is_err());
}

#[test]
fn nested_function_layout_tables_are_bounded_on_memory_and_wire_routes() {
    let context = DefinitionContext::new().unwrap();
    let valid = BytecodeProgram {
        exports: Default::default(),
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
            KbcArtifact::from_program(program.clone(), Default::default(), &context),
            Err(ArtifactValidationError::ResourceLimit(found)) if found == reason
        ));
        let mut artifact =
            KbcArtifact::from_program(valid.clone(), Default::default(), &context).unwrap();
        artifact.program = program;
        assert!(matches!(
            artifact.validate_for_loader(&Default::default(), &context),
            Err(ArtifactValidationError::ResourceLimit(found)) if found == reason
        ));
        assert!(artifact.to_bytes(&context.snapshot()).is_err());
        let crafted = codec()
            .serialize(
                &wire::encode_artifact(&artifact, &context.snapshot(), &Default::default())
                    .unwrap(),
            )
            .unwrap();
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
    let context = DefinitionContext::new().unwrap();
    let valid = BytecodeProgram {
        exports: Default::default(),
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
            },
            &context
        ),
        Err(ArtifactValidationError::ResourceLimit(
            "debug record limit exceeded"
        ))
    ));
    let mut artifact = KbcArtifact::from_program(valid, Default::default(), &context).unwrap();
    artifact.debug = Some(metadata);
    assert!(matches!(
        artifact.validate_for_loader(&Default::default(), &context),
        Err(ArtifactValidationError::ResourceLimit(
            "debug record limit exceeded"
        ))
    ));
    assert!(artifact.to_bytes(&context.snapshot()).is_err());
    let crafted = codec()
        .serialize(
            &wire::encode_artifact(&artifact, &context.snapshot(), &Default::default()).unwrap(),
        )
        .unwrap();
    assert!(
        KbcArtifact::from_bytes(&crafted)
            .unwrap_err()
            .message()
            .contains("artifact table count limit exceeded")
    );
}

#[test]
fn decoder_rejects_forged_header_identity_path_length() {
    let context = DefinitionContext::new().unwrap();
    let artifact = KbcArtifact::from_program(
        BytecodeProgram {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        },
        Default::default(),
        &context,
    )
    .unwrap();
    let mut bytes = artifact.to_bytes(&context.snapshot()).unwrap();
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
    let context = DefinitionContext::new().unwrap();
    let artifact = KbcArtifact::from_program(
        BytecodeProgram {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        },
        Default::default(),
        &context,
    )
    .unwrap();
    let mut bytes = artifact.to_bytes(&context.snapshot()).unwrap();
    let offset = (codec().serialized_size(&artifact.header).unwrap()
        + codec().serialized_size(&artifact.program.root).unwrap()) as usize;
    bytes[offset..offset + 8].copy_from_slice(&u64::MAX.to_le_bytes());
    let error = KbcArtifact::from_bytes(&bytes).unwrap_err();
    assert!(error.message().contains("module count limit exceeded"));
}

#[test]
fn deep_abi_types_are_rejected_before_artifact_fingerprinting() {
    let context = DefinitionContext::new().unwrap();

    let valid = BytecodeProgram {
        exports: Default::default(),
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule::default()],
    };
    let mut deep = Ty::Builtin(BuiltinType::I32);
    for _ in 0..64 {
        deep = Ty::Array(Box::new(deep));
    }
    let mut program = valid.clone();
    program.modules[0]
        .public_items
        .push(PublicItem::Const(ConstDef {
            name: "deep".into(),
            ty: deep,
            value: "0".into(),
        }));
    assert!(matches!(
        KbcArtifact::from_program(program.clone(), Default::default(), &context),
        Err(ArtifactValidationError::ResourceLimit(
            "ABI type resource limit exceeded"
        ))
    ));
    let mut artifact = KbcArtifact::from_program(valid, Default::default(), &context).unwrap();
    artifact.program = program;
    assert!(matches!(
        artifact.validate_for_loader(&Default::default(), &context),
        Err(ArtifactValidationError::ResourceLimit(
            "ABI type resource limit exceeded"
        ))
    ));
    assert!(artifact.to_bytes(&context.snapshot()).is_err());
}

#[test]
fn nested_layout_and_host_path_counts_are_bounded_on_all_artifact_routes() {
    let context = DefinitionContext::new().unwrap();

    let valid = BytecodeProgram {
        exports: Default::default(),
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule::default()],
    };
    let id = |kind| {
        context
            .intern(&DefinitionPath {
                module: valid.modules[0].identity.clone(),
                path: vec![DefinitionPathSegment {
                    kind,
                    name: "item".into(),
                    occurrence: 0,
                }],
            })
            .unwrap()
    };
    let field_id = id(DefinitionKind::Field);
    let mut cases = Vec::new();

    let mut structure = valid.clone();
    structure.modules[0].structures.push(StructLayout {
        declaration: id(DefinitionKind::Struct),
        arguments: Vec::new(),
        fields: vec![
            StructFieldLayout {
                visibility: Visibility::Public,
                declaration: field_id,
                name: "field".into(),
                ty: Ty::Builtin(BuiltinType::I32),
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
            reports_failure: false,
            declaration: id(DefinitionKind::Variant),
            payload: vec![Ty::Builtin(BuiltinType::I32); MAX_ARTIFACT_NESTED_RECORDS + 1],
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
                HostPathSegmentDeclaration::Field(field_id);
                MAX_ARTIFACT_NESTED_RECORDS + 1
            ],
            access: PathAccess::ReadOnly,
            schema_epoch: 0,
        });
    cases.push(host_path);

    let mut public_abi = valid.clone();
    public_abi.modules[0]
        .public_items
        .push(PublicItem::Type(TypeDef {
            name: "item".into(),
            kind: TypeDefKind::Struct,
            generic_params: Vec::new(),
            bounds: Vec::new(),
            fields: vec![
                FieldDef {
                    host_policy: None,
                    visibility: Visibility::Public,
                    name: "field".into(),
                    ty: Ty::Builtin(BuiltinType::I32),
                    mutable: false,
                };
                MAX_ARTIFACT_NESTED_RECORDS + 1
            ],
            variants: Vec::new(),
        }));
    cases.push(public_abi);

    let mut source_reference = valid.clone();
    source_reference.modules[0]
        .trait_references
        .push(TraitContract {
            declaration: id(DefinitionKind::Trait),
            abi: TraitDef {
                name: "item".into(),
                associated_consts: vec![
                    AssociatedConstDef {
                        declaration: id(DefinitionKind::Const),
                        ty: Ty::Builtin(BuiltinType::I32),
                        default_value: None,
                    };
                    MAX_ARTIFACT_NESTED_RECORDS + 1
                ],
                supertraits: Vec::new(),
                generic_params: Vec::new(),
                bounds: Vec::new(),
                methods: Vec::new(),
                associated_types: Vec::new(),
                conversion_adapter: None,
            },
        });
    let mut reference_table = source_reference.clone();
    let mut reference = reference_table.modules[0].trait_references.remove(0);
    reference.abi.associated_consts.clear();
    reference_table.modules[0].trait_references = vec![reference; MAX_ARTIFACT_NESTED_RECORDS + 1];
    cases.push(source_reference);
    cases.push(reference_table);

    for (index, program) in cases.into_iter().enumerate() {
        assert!(matches!(
            KbcArtifact::from_program(program.clone(), Default::default(), &context),
            Err(ArtifactValidationError::ResourceLimit(
                "nested module record limit exceeded"
            ))
        ));
        let mut artifact =
            KbcArtifact::from_program(valid.clone(), Default::default(), &context).unwrap();
        artifact.program = program;
        assert!(matches!(
            artifact.validate_for_loader(&Default::default(), &context),
            Err(ArtifactValidationError::ResourceLimit(
                "nested module record limit exceeded"
            ))
        ));
        assert!(artifact.to_bytes(&context.snapshot()).is_err());
        let crafted = codec()
            .serialize(
                &wire::encode_artifact(&artifact, &context.snapshot(), &Default::default())
                    .unwrap(),
            )
            .unwrap();
        let error = KbcArtifact::from_bytes(&crafted).unwrap_err();
        assert!(
            error
                .message()
                .contains("nested declaration count limit exceeded"),
            "{index}: {error}"
        );
    }
}

#[test]
fn module_count_limit_rejects_memory_and_encoded_artifacts_before_verification() {
    let context = DefinitionContext::new().unwrap();
    let valid = BytecodeProgram {
        exports: Default::default(),
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule::default()],
    };
    let mut excessive = valid.clone();
    excessive
        .modules
        .resize(MAX_ARTIFACT_MODULES + 1, BytecodeModule::default());
    assert!(matches!(
        KbcArtifact::from_program(excessive.clone(), Default::default(), &context),
        Err(ArtifactValidationError::ResourceLimit("too many modules"))
    ));
    let mut artifact = KbcArtifact::from_program(valid, Default::default(), &context).unwrap();
    artifact.program = excessive;
    assert!(matches!(
        artifact.validate_for_loader(&Default::default(), &context),
        Err(ArtifactValidationError::ResourceLimit("too many modules"))
    ));
    assert!(
        artifact
            .to_bytes(&context.snapshot())
            .unwrap_err()
            .message()
            .contains("too many modules")
    );
    let crafted = codec()
        .serialize(
            &wire::encode_artifact(&artifact, &context.snapshot(), &Default::default()).unwrap(),
        )
        .unwrap();
    assert!(
        KbcArtifact::from_bytes(&crafted)
            .unwrap_err()
            .message()
            .contains("module count limit exceeded")
    );
}

#[test]
fn declared_section_counts_are_bounded_independently_of_payload_size() {
    let context = DefinitionContext::new().unwrap();
    let mut artifact = KbcArtifact::from_program(
        BytecodeProgram {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        },
        Default::default(),
        &context,
    )
    .unwrap();
    artifact.tables.sections[0].record_count = MAX_ARTIFACT_TABLE_RECORDS + 1;
    assert!(matches!(
        artifact.validate_for_loader(&Default::default(), &context),
        Err(ArtifactValidationError::ResourceLimit(
            "artifact metadata record limit exceeded"
        ))
    ));
    assert!(artifact.to_bytes(&context.snapshot()).is_err());
    let crafted = codec()
        .serialize(
            &wire::encode_artifact(&artifact, &context.snapshot(), &Default::default()).unwrap(),
        )
        .unwrap();
    assert!(KbcArtifact::from_bytes(&crafted).is_err());
}

#[test]
fn recomputed_hash_cannot_hide_inconsistent_artifact_tables() {
    let context = DefinitionContext::new().unwrap();
    let artifact = KbcArtifact::from_program(
        BytecodeProgram {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        },
        Default::default(),
        &context,
    )
    .unwrap();
    assert!(
        artifact
            .validate_for_loader(&Default::default(), &context)
            .is_ok()
    );
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
        changed.header.content_hash = changed.compute_content_hash(&context.snapshot()).unwrap();
        assert!(
            matches!(
                changed.validate_for_loader(&Default::default(), &context),
                Err(ArtifactValidationError::TableMismatch)
            ),
            "change {change}"
        );
    }
}

#[test]
fn invalid_host_types_are_rejected_before_fingerprinting_memory_artifacts() {
    let context = DefinitionContext::new().unwrap();

    let valid = BytecodeProgram {
        exports: Default::default(),
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule::default()],
    };
    let mut deep = HostValueType::I32;
    for _ in 0..64 {
        deep = HostValueType::Array(Box::new(deep));
    }
    for ty in [
        HostValueType::Set(Box::new(HostValueType::F32), CollectionAccess::Mutable),
        deep,
    ] {
        let mut program = valid.clone();
        program.modules[0]
            .host_interface
            .functions
            .push(HostFunctionDeclaration::new(&context, "host.bad", vec![], ty).unwrap());
        assert!(matches!(
            VerificationMetadata::from_program(&program, &Default::default(), &context),
            Err(ArtifactValidationError::Bytecode(_))
        ));
        // Artifact resource preflight visits bounded host types before encoding
        // or fingerprinting; direct verification still rejects the host contract.
        let error =
            KbcArtifact::from_program(program.clone(), Default::default(), &context).unwrap_err();
        assert!(
            matches!(
                error,
                ArtifactValidationError::Identity(DefinitionMappingError::InvalidContract)
            ),
            "{error:?}"
        );
        let mut artifact =
            KbcArtifact::from_program(valid.clone(), Default::default(), &context).unwrap();
        artifact.program = program;
        assert!(matches!(
            artifact.validate_for_loader(&Default::default(), &context),
            Err(ArtifactValidationError::Identity(
                DefinitionMappingError::InvalidContract
            ))
        ));
    }
}

#[test]
fn bytecode_cannot_claim_a_different_identity_from_its_header() {
    let context = DefinitionContext::new().unwrap();
    let mut artifact = KbcArtifact::from_program(
        BytecodeProgram {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        },
        Default::default(),
        &context,
    )
    .unwrap();
    artifact.program.modules[artifact.program.root.index()].identity =
        ModuleIdentity::single_file("forged.kgr");
    artifact.header.content_hash = artifact.compute_content_hash(&context.snapshot()).unwrap();
    assert!(matches!(
        artifact.validate_for_loader(&Default::default(), &context),
        Err(ArtifactValidationError::ModuleIdentityMismatch { .. })
    ));
}

#[test]
fn legacy_language_semantics_cannot_be_opted_into() {
    let context = DefinitionContext::new().unwrap();
    let mut artifact = KbcArtifact::from_program(
        BytecodeProgram {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        },
        Default::default(),
        &context,
    )
    .unwrap();
    artifact.header.language_version = "0.1.0".into();
    let requirements = ArtifactCompatibility {
        language_version: "0.1.0".into(),
        ..Default::default()
    };
    assert!(matches!(
        artifact.validate_for_loader(&requirements, &context),
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
fn header_metadata_is_covered_and_old_format_cannot_be_opted_into() {
    let context = DefinitionContext::new().unwrap();
    let mut artifact = KbcArtifact::from_program(
        BytecodeProgram {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        },
        ArtifactBuildOptions::default(),
        &context,
    )
    .unwrap();
    artifact
        .header
        .module_identity
        .package
        .0
        .push_str("changed");
    assert!(matches!(
        artifact.validate_for_loader(&ArtifactCompatibility::default(), &context),
        Err(ArtifactValidationError::ContentHashMismatch)
    ));
    artifact.header.format_version = 1;
    let requirements = ArtifactCompatibility {
        format_version: 1,
        ..ArtifactCompatibility::default()
    };
    assert!(matches!(
        artifact.validate_for_loader(&requirements, &context),
        Err(ArtifactValidationError::FormatVersionMismatch { .. })
    ));
}

#[test]
fn recomputed_checksum_cannot_hide_dependency_or_verification_metadata_changes() {
    let context = DefinitionContext::new().unwrap();

    let program = BytecodeProgram {
        exports: Default::default(),
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
    let original = KbcArtifact::from_program(program, Default::default(), &context).unwrap();
    original
        .validate_for_loader(&Default::default(), &context)
        .unwrap();
    let mut artifact = original.clone();
    artifact.program.modules[0].source_name.push_str("changed");
    artifact.header.content_hash = artifact.compute_content_hash(&context.snapshot()).unwrap();
    assert!(matches!(
        artifact.validate_for_loader(&Default::default(), &context),
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
    artifact.header.content_hash = artifact.compute_content_hash(&context.snapshot()).unwrap();
    assert!(matches!(
        artifact.validate_for_loader(&Default::default(), &context),
        Err(ArtifactValidationError::VerificationMetadataMismatch)
    ));
}

#[test]
fn portable_mir_is_opaque_but_integrity_and_manifest_bound() {
    let context = DefinitionContext::new().unwrap();
    let program = BytecodeProgram {
        exports: Default::default(),
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule::default()],
    };
    let plain = KbcArtifact::from_program(program.clone(), Default::default(), &context).unwrap();
    let artifact = KbcArtifact::from_program(
        program,
        ArtifactBuildOptions {
            portable_mir: Some(PortableMir {
                // Bytecode loading deliberately does not decode compiler input.
                bytes: vec![1, 2, 3],
            }),
            ..Default::default()
        },
        &context,
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
    let bytes = artifact.to_bytes(&context.snapshot()).unwrap();
    let decoded = KbcArtifact::from_bytes(&bytes)
        .unwrap()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    decoded
        .validate_for_loader(&Default::default(), &context)
        .unwrap();
    assert_eq!(decoded.to_bytes(&context.snapshot()).unwrap(), bytes);
    assert_eq!(decoded.portable_mir.unwrap().bytes, [1, 2, 3]);

    let mut changed = artifact.clone();
    changed.portable_mir.as_mut().unwrap().bytes[0] = 4;
    assert!(matches!(
        changed.validate_for_loader(&Default::default(), &context),
        Err(ArtifactValidationError::ContentHashMismatch)
    ));
    // Even a refreshed outer checksum cannot hide a stale section fingerprint.
    changed.header.content_hash = changed.compute_content_hash(&context.snapshot()).unwrap();
    assert!(matches!(
        changed.validate_for_loader(&Default::default(), &context),
        Err(ArtifactValidationError::TableMismatch)
    ));
    changed.portable_mir = None;
    changed.header.content_hash = changed.compute_content_hash(&context.snapshot()).unwrap();
    assert!(matches!(
        changed.validate_for_loader(&Default::default(), &context),
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
    let context = DefinitionContext::new().unwrap();
    let error = codec()
        .deserialize::<PortableMir>(&u64::MAX.to_le_bytes())
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("portable MIR byte limit exceeded")
    );
    let program = BytecodeProgram {
        exports: Default::default(),
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule::default()],
    };
    let mut artifact =
        KbcArtifact::from_program(program.clone(), Default::default(), &context).unwrap();
    artifact.portable_mir = Some(PortableMir {
        bytes: vec![0; MAX_ARTIFACT_BYTES as usize + 1],
    });
    assert!(matches!(
        artifact.validate_for_loader(&Default::default(), &context),
        Err(ArtifactValidationError::ResourceLimit(
            "portable MIR byte limit exceeded"
        ))
    ));
    assert!(
        artifact
            .to_bytes(&context.snapshot())
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
            },
            &context
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
            },
            &context
        ),
        Err(ArtifactValidationError::ResourceLimit(
            "artifact encoded size limit exceeded"
        ))
    ));
}
