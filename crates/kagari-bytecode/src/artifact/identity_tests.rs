//! Scope, verification evidence and canonical file boundaries of executable metadata.
use crate::{
    artifact::{
        ArtifactBuildOptions, ArtifactHeader, ArtifactValidationError, KBC_ARTIFACT_FORMAT_VERSION,
        KbcArtifact, MAX_ARTIFACT_BYTES, MAX_ARTIFACT_MODULES, encode_program_facts,
    },
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef, verified::VerifiedBytecodeProgram},
    verifier::BytecodeVerificationError,
};
use bincode::{DefaultOptions, Options};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext},
};
use kagari_contract::{
    layout::{StructFieldLayout, StructLayout},
    types::{PublicItem, TraitContract},
};
use kagari_types::{
    declaration::{FieldDef, TraitDef, TypeDeclaration, TypeDef, TypeDefKind},
    scalar::BuiltinType,
    ty::{GenericBound, GenericParam, NominalTy, Ty},
    visibility::Visibility,
};
use std::io::Cursor;

fn codec() -> impl Options {
    DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .reject_trailing_bytes()
        .with_limit(MAX_ARTIFACT_BYTES)
}

#[test]
fn private_types_round_trip_preserves_bounds_and_rejects_forged_authority() {
    let context = DefinitionContext::new().unwrap();
    let identity = ModuleIdentity::single_file("private-types");
    let root = context.intern_root(&identity).unwrap();
    let hidden = context
        .intern_child(root, DefinitionKind::Struct, "Hidden", 0)
        .unwrap();
    let leaf = context
        .intern_child(root, DefinitionKind::Struct, "Leaf", 0)
        .unwrap();
    let field = context
        .intern_child(hidden, DefinitionKind::Field, "value", 0)
        .unwrap();
    let marker = context
        .intern_child(root, DefinitionKind::Trait, "Marker", 0)
        .unwrap();
    let unknown = context
        .intern_child(root, DefinitionKind::Trait, "Unknown", 0)
        .unwrap();
    let foreign_root = context
        .intern_root(&ModuleIdentity::single_file("other"))
        .unwrap();
    let foreign = context
        .intern_child(foreign_root, DefinitionKind::Struct, "Hidden", 0)
        .unwrap();
    let parameter = GenericParam {
        owner: hidden,
        position: 0,
    };
    let mut module = BytecodeModule {
        identity,
        private_types: vec![
            TypeDeclaration {
                declaration: hidden,
                definition: TypeDef {
                    name: "Hidden".into(),
                    kind: TypeDefKind::Struct,
                    generic_params: vec![parameter.clone()],
                    bounds: vec![GenericBound {
                        ty: parameter.as_type(),
                        constraints: vec![NominalTy {
                            declaration: marker,
                            arguments: vec![],
                            associated_types: Default::default(),
                        }],
                    }],
                    variants: vec![],
                    fields: vec![FieldDef {
                        host_policy: None,
                        visibility: Visibility::Private,
                        name: "value".into(),
                        ty: parameter.as_type(),
                        mutable: false,
                    }],
                },
            },
            // Declaration-only evidence must not force an otherwise unused layout.
            TypeDeclaration {
                declaration: leaf,
                definition: TypeDef {
                    name: "Leaf".into(),
                    kind: TypeDefKind::Struct,
                    generic_params: vec![],
                    bounds: vec![],
                    variants: vec![],
                    fields: vec![],
                },
            },
        ],
        trait_contracts: vec![TraitContract {
            declaration: marker,
            abi: TraitDef {
                name: "Marker".into(),
                generic_params: vec![],
                bounds: vec![],
                supertraits: vec![],
                methods: vec![],
                associated_types: vec![],
                associated_consts: vec![],
                conversion_adapter: None,
            },
        }],
        ..Default::default()
    };
    module.structures.push(StructLayout {
        declaration: hidden,
        arguments: vec![parameter.as_type()],
        fields: vec![StructFieldLayout {
            declaration: field,
            name: "value".into(),
            ty: parameter.as_type(),
            visibility: Visibility::Private,
            mutable: false,
        }],
    });
    let artifact = KbcArtifact::from_program(
        BytecodeProgram {
            update_evidence: crate::program::updates::empty(),
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![module],
        },
        Default::default(),
        &context,
        &Default::default(),
    )
    .unwrap();
    let bytes = artifact.to_bytes(&context.snapshot()).unwrap();
    let decoded = KbcArtifact::from_bytes(&bytes).unwrap();
    let imported = decoded.import_into(&context, &Default::default()).unwrap();
    assert_eq!(
        imported.records().program.modules[0].private_types,
        artifact.program.modules[0].private_types
    );
    assert!(
        imported.records().program.modules[0]
            .public_items
            .is_empty()
    );
    assert!(
        imported.records().program.modules[0]
            .external_types
            .is_empty()
    );
    assert_eq!(
        decoded.records().to_bytes(decoded.definitions()).unwrap(),
        bytes
    );
    imported
        .records()
        .validate_for_loader(&Default::default(), &context, &Default::default())
        .unwrap();

    for corruption in 0..13 {
        let mut forged = artifact.clone();
        let module = &mut forged.program.modules[0];
        match corruption {
            0 => module.private_types[0].declaration = foreign,
            1 => module.private_types[0].definition.generic_params[0].owner = leaf,
            2 => module.private_types[0].definition.generic_params[0].position = 1,
            3 => module.private_types[0].definition.name = "Wrong".into(),
            4 => module.private_types[0].definition.kind = TypeDefKind::Enum,
            5 => module.private_types.push(module.private_types[0].clone()),
            6 => module
                .public_items
                .push(PublicItem::Type(module.private_types[0].definition.clone())),
            7 => module.private_types[0].definition.fields[0].ty = Ty::Builtin(BuiltinType::Bool),
            8 => module.private_types[0].definition.fields[0].visibility = Visibility::Public,
            9 => module.private_types[0].definition.fields[0].mutable = true,
            10 => module.private_types[0].definition.bounds[0].constraints[0].declaration = hidden,
            11 => module.private_types[0].definition.bounds[0].constraints[0].declaration = unknown,
            12 => {
                let invalid_application = Ty::Struct(NominalTy {
                    declaration: leaf,
                    arguments: vec![Ty::Builtin(BuiltinType::I32)],
                    associated_types: Default::default(),
                });
                module.private_types[0].definition.fields[0].ty = invalid_application.clone();
                module.structures[0].fields[0].ty = invalid_application;
            }
            _ => unreachable!(),
        }
        let bytes = forged.to_bytes(&context.snapshot()).unwrap();
        let decoded = KbcArtifact::from_bytes(&bytes).unwrap();
        let imported = decoded.import_into(&context, &Default::default()).unwrap();
        let result = imported.records().validate_for_loader(
            &Default::default(),
            &context,
            &Default::default(),
        );
        assert!(
            matches!(result, Err(ArtifactValidationError::Bytecode(_))),
            "corruption {corruption}: {result:?}"
        );
        if corruption <= 10 {
            assert!(
                matches!(
                    result,
                    Err(ArtifactValidationError::Bytecode(
                        BytecodeVerificationError::InvalidPublicAbi
                    ))
                ),
                "corruption {corruption}: {result:?}"
            );
        }
    }
}

#[test]
fn consuming_artifact_verification_rejects_changed_code_and_envelope() {
    let context = DefinitionContext::new().unwrap();
    let artifact = KbcArtifact::from_program(
        BytecodeProgram {
            update_evidence: crate::program::updates::empty(),
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        },
        Default::default(),
        &context,
        &Default::default(),
    )
    .unwrap();
    let verified = artifact
        .clone()
        .into_verified(&Default::default(), &context, &Default::default())
        .unwrap();
    assert_eq!(verified.bytecode().program().modules.len(), 1);
    assert!(verified.portable_mir().is_none());

    let mut changed = artifact.clone();
    changed.program.modules[0].source_name = "changed".into();
    assert!(matches!(
        changed.into_verified(&Default::default(), &context, &Default::default()),
        Err(ArtifactValidationError::ContentHashMismatch)
    ));
    let mut invalid = artifact.clone();
    invalid.program.root = ModuleRef::new(1);
    assert!(matches!(
        invalid.into_verified(&Default::default(), &context, &Default::default()),
        Err(ArtifactValidationError::Bytecode(_))
    ));
    let mut incompatible = artifact;
    incompatible.header.runtime_abi_version = "invalid".into();
    assert!(matches!(
        incompatible.into_verified(&Default::default(), &context, &Default::default()),
        Err(ArtifactValidationError::RuntimeAbiMismatch { .. })
    ));

    // Mutable extraction never carries the verification seal to the changed code.
    let retained = verified.into_bytecode().into_unverified();
    let later = context
        .intern_root(&ModuleIdentity::single_file("later"))
        .unwrap();
    assert!(retained.definitions().resolve(later).is_err());
    let (_, mut extracted) = retained.into_parts();
    extracted.root = ModuleRef::new(1);
    assert!(matches!(
        VerifiedBytecodeProgram::new(extracted, &context, &Default::default()),
        Err(ArtifactValidationError::Bytecode(_))
    ));
}

#[test]
fn bytecode_seal_rejects_in_memory_resource_exhaustion() {
    let context = DefinitionContext::new().unwrap();
    let oversized = BytecodeProgram {
        update_evidence: crate::program::updates::empty(),
        exports: Default::default(),
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule::default(); MAX_ARTIFACT_MODULES + 1],
    };
    assert!(matches!(
        VerifiedBytecodeProgram::new(oversized, &context, &Default::default()),
        Err(ArtifactValidationError::ResourceLimit(_))
    ));
}

#[test]
fn memory_artifacts_reject_oversized_strings_before_fingerprinting() {
    let context = DefinitionContext::new().unwrap();
    let valid = BytecodeProgram {
        update_evidence: crate::program::updates::empty(),
        exports: Default::default(),
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule::default()],
    };
    {
        let mut program = valid.clone();
        program.modules[0].source_name = "x".repeat(MAX_ARTIFACT_BYTES as usize);
        assert!(matches!(
            KbcArtifact::from_program(program, Default::default(), &context, &Default::default()),
            Err(ArtifactValidationError::ResourceLimit(
                "artifact encoded size limit exceeded"
            ))
        ));
    }
    let mut artifact =
        KbcArtifact::from_program(valid, Default::default(), &context, &Default::default())
            .unwrap();
    artifact.header.compiler_fingerprint = "x".repeat(MAX_ARTIFACT_BYTES as usize);
    assert!(matches!(
        artifact.validate_for_loader(&Default::default(), &context, &Default::default()),
        Err(ArtifactValidationError::ResourceLimit(
            "artifact encoded size limit exceeded"
        ))
    ));
    assert!(
        artifact
            .to_bytes(&context.snapshot())
            .unwrap_err()
            .message()
            .contains("artifact encoded size limit exceeded")
    );
}

#[test]
fn decoder_rejects_old_versions_trailing_bytes_and_oversized_lengths() {
    let context = DefinitionContext::new().unwrap();
    let artifact = KbcArtifact::from_program(
        BytecodeProgram {
            update_evidence: crate::program::updates::empty(),
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        },
        ArtifactBuildOptions::default(),
        &context,
        &Default::default(),
    )
    .unwrap();
    let bytes = artifact.to_bytes(&context.snapshot()).unwrap();
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
    let context = DefinitionContext::new().unwrap();
    use kagari_contract::types::PublicItem;
    use kagari_types::{
        declaration::{
            TypeDeclaration, TypeDef, TypeDefKind,
            host::{HostReflectionPolicy, HostTypeOwnership, HostTypePolicy, PathAccess},
            path::{
                HostPathDeclaration, HostPathSegmentDeclaration, HostVirtualSegmentDeclaration,
            },
            wire::declarations::TypeDeclarationRecord,
        },
        scalar::BuiltinType,
    };
    let identity = ModuleIdentity {
        package: PackageId("game".into()),
        path: vec!["state".into()],
    };
    let owner = context.intern_root(&identity).unwrap();
    let root = context
        .intern_child(owner, DefinitionKind::Struct, "Player", 0)
        .unwrap();
    let path = HostPathDeclaration {
        root,
        segments: vec![HostPathSegmentDeclaration::Virtual(
            HostVirtualSegmentDeclaration {
                name: "preview".into(),
                result: Ty::Builtin(BuiltinType::I32),
                access: PathAccess::ReadOnly,
            },
        )],
        access: PathAccess::ReadOnly,
        schema_epoch: 1,
    };
    let mut module = BytecodeModule {
        identity,
        ..Default::default()
    };
    module.external_types.push(TypeDeclaration {
        declaration: root,
        definition: TypeDef {
            name: "Player".into(),
            kind: TypeDefKind::Host(HostTypePolicy {
                ownership: HostTypeOwnership::HostRoot,
                reflection: HostReflectionPolicy::Metadata,
                path_access: PathAccess::ReadOnly,
            }),
            generic_params: vec![],
            bounds: vec![],
            fields: vec![],
            variants: vec![],
        },
    });
    module.public_items.push(PublicItem::Type(
        module.external_types[0].definition.clone(),
    ));
    module.path_declarations.push(path.clone());
    let artifact = KbcArtifact::from_program(
        BytecodeProgram {
            update_evidence: crate::program::updates::empty(),
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![module],
        },
        ArtifactBuildOptions::default(),
        &context,
        &Default::default(),
    )
    .unwrap();
    let decoded =
        KbcArtifact::from_bytes(&artifact.to_bytes(&context.snapshot()).unwrap()).unwrap();
    let same_scope = decoded
        .import_into(&context, &CancellationToken::default())
        .unwrap();
    assert_eq!(
        same_scope.records().program.modules[0].path_declarations,
        artifact.program.modules[0].path_declarations
    );
    let decoded_path = &decoded.records().program.modules[0].path_declarations[0];
    assert_eq!(
        decoded
            .definitions()
            .resolve(decoded_path.root)
            .unwrap()
            .to_path(),
        context.snapshot().resolve(path.root).unwrap().to_path()
    );
    assert_eq!(decoded_path.schema_epoch, path.schema_epoch);
    assert_eq!(
        decoded.records().to_bytes(decoded.definitions()).unwrap(),
        artifact.to_bytes(&context.snapshot()).unwrap()
    );

    let target = DefinitionContext::new().unwrap();
    target
        .intern_root(&ModuleIdentity::single_file("unrelated"))
        .unwrap();
    let imported = decoded
        .import_into(&target, &CancellationToken::default())
        .unwrap();
    assert_eq!(
        imported.records().header.content_hash,
        artifact.header.content_hash
    );
    assert_eq!(
        imported.records().to_bytes(imported.definitions()).unwrap(),
        artifact.to_bytes(&context.snapshot()).unwrap()
    );
    imported
        .records()
        .validate_for_loader(&Default::default(), &target, &Default::default())
        .unwrap();
    assert_eq!(
        encode_program_facts(&artifact.program, &context.snapshot(), &Default::default()).unwrap(),
        encode_program_facts(
            &imported.records().program,
            imported.definitions(),
            &Default::default()
        )
        .unwrap()
    );
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    assert!(encode_program_facts(&artifact.program, &context.snapshot(), &cancelled).is_err());
    let foreign = imported.records().clone();
    assert!(
        foreign
            .into_verified(&Default::default(), &context, &Default::default())
            .is_err()
    );
    let verified = imported
        .into_records()
        .into_verified(&Default::default(), &target, &Default::default())
        .unwrap();
    let unverified = verified.into_bytecode().into_unverified();
    drop(target);
    assert!(
        unverified
            .definitions()
            .resolve(unverified.records().modules[0].path_declarations[0].root)
            .is_ok()
    );

    // Corrupt a file-local reference in a current envelope; decoding cannot turn
    // it into a trusted process identity or silently fall back to a path.
    let bytes = artifact.to_bytes(&context.snapshot()).unwrap();
    // Read the actual shared-schema prefix to locate this fixture's first path.
    // Its header and module references have the real codec types; counts are
    // asserted so a schema change cannot accidentally corrupt a different field.
    let mut cursor = Cursor::new(&bytes);
    let (_, _, modules, dependencies, schemas, paths): (
        ArtifactHeader,
        ModuleRef,
        u64,
        Vec<ModuleRef>,
        Vec<TypeDeclarationRecord>,
        u64,
    ) = codec()
        .allow_trailing_bytes()
        .deserialize_from(&mut cursor)
        .unwrap();
    assert_eq!(modules, 1);
    assert!(dependencies.is_empty());
    assert_eq!(schemas.len(), 1);
    assert_eq!(paths, 1);
    let reference_offset = cursor.position() as usize;
    let mut forged = bytes;
    forged[reference_offset..reference_offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(KbcArtifact::from_bytes(&forged).is_err());
}
