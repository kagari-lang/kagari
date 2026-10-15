//! Scope, verification evidence and canonical file boundaries of executable metadata.
use bincode::{DefaultOptions, Options};
use kagari_bytecode::{
    artifact::{
        ArtifactBuildOptions, ArtifactHeader, ArtifactValidationError, KBC_ARTIFACT_FORMAT_VERSION,
        KbcArtifact, MAX_ARTIFACT_BYTES, MAX_ARTIFACT_MODULES, encode_program_facts,
    },
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef, verified::VerifiedBytecodeProgram},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext},
};
use kagari_types::ty::Ty;
use std::io::Cursor;

fn codec() -> impl Options {
    DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .reject_trailing_bytes()
        .with_limit(MAX_ARTIFACT_BYTES)
}

#[test]
fn consuming_artifact_verification_rejects_changed_code_and_envelope() {
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
    let verified = artifact
        .clone()
        .into_verified(&Default::default(), &context)
        .unwrap();
    assert_eq!(verified.bytecode().program().modules.len(), 1);
    assert!(verified.portable_mir().is_none());

    let mut changed = artifact.clone();
    changed.program.modules[0].source_name = "changed".into();
    assert!(matches!(
        changed.into_verified(&Default::default(), &context),
        Err(ArtifactValidationError::ContentHashMismatch)
    ));
    let mut invalid = artifact.clone();
    invalid.program.root = ModuleRef::new(1);
    assert!(matches!(
        invalid.into_verified(&Default::default(), &context),
        Err(ArtifactValidationError::Bytecode(_))
    ));
    let mut incompatible = artifact;
    incompatible.header.runtime_abi_version = "invalid".into();
    assert!(matches!(
        incompatible.into_verified(&Default::default(), &context),
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
        VerifiedBytecodeProgram::new(extracted, &context),
        Err(ArtifactValidationError::Bytecode(_))
    ));
}

#[test]
fn bytecode_seal_rejects_in_memory_resource_exhaustion() {
    let context = DefinitionContext::new().unwrap();
    let oversized = BytecodeProgram {
        exports: Default::default(),
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule::default(); MAX_ARTIFACT_MODULES + 1],
    };
    assert!(matches!(
        VerifiedBytecodeProgram::new(oversized, &context),
        Err(ArtifactValidationError::ResourceLimit(_))
    ));
}

#[test]
fn memory_artifacts_reject_oversized_strings_before_fingerprinting() {
    let context = DefinitionContext::new().unwrap();
    let valid = BytecodeProgram {
        exports: Default::default(),
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule::default()],
    };
    {
        let mut program = valid.clone();
        program.modules[0].source_name = "x".repeat(MAX_ARTIFACT_BYTES as usize);
        assert!(matches!(
            KbcArtifact::from_program(program, Default::default(), &context),
            Err(ArtifactValidationError::ResourceLimit(
                "artifact encoded size limit exceeded"
            ))
        ));
    }
    let mut artifact = KbcArtifact::from_program(valid, Default::default(), &context).unwrap();
    artifact.header.compiler_fingerprint = "x".repeat(MAX_ARTIFACT_BYTES as usize);
    assert!(matches!(
        artifact.validate_for_loader(&Default::default(), &context),
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
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        },
        ArtifactBuildOptions::default(),
        &context,
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
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![module],
        },
        ArtifactBuildOptions::default(),
        &context,
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
        .validate_for_loader(&Default::default(), &target)
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
            .into_verified(&Default::default(), &context)
            .is_err()
    );
    let verified = imported
        .into_records()
        .into_verified(&Default::default(), &target)
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
