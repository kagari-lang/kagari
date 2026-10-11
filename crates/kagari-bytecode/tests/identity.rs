//! Scope, verification evidence and canonical file boundaries of executable metadata.
use bincode::{DefaultOptions, Options};
use kagari_bytecode::{
    artifact::{
        ArtifactBuildOptions, ArtifactFingerprint, ArtifactValidationError,
        KBC_ARTIFACT_FORMAT_VERSION, KbcArtifact, MAX_ARTIFACT_BYTES, MAX_ARTIFACT_MODULES,
    },
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef, verified::VerifiedBytecodeProgram},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, ModuleIdentity, PackageId,
        map::DefinitionContext,
        metadata::{DefinitionMetadata, scope_record},
        table::wire::{PortableDefinitionRef, PortableDefinitionTable},
    },
};

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
    use kagari_types::host_interface::{
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
    let adopted = scope_record(&module, &CancellationToken::default())
        .unwrap()
        .import_into(&context, &CancellationToken::default())
        .unwrap();
    let artifact = KbcArtifact::from_program(
        BytecodeProgram {
            root: ModuleRef::new(0),
            modules: vec![adopted.into_records()],
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
        same_scope.records().program.modules[0].host_interface.paths,
        artifact.program.modules[0].host_interface.paths
    );
    let decoded_path = &decoded.records().program.modules[0].host_interface.paths[0];
    assert_eq!(
        decoded
            .definitions()
            .resolve(decoded_path.root)
            .unwrap()
            .to_path(),
        path.root
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
            .resolve(unverified.records().modules[0].host_interface.paths[0].root)
            .is_ok()
    );

    // Corrupt a file-local reference in a current envelope; decoding cannot turn
    // it into a trusted process identity or silently fall back to a path.
    let bytes = artifact.to_bytes(&context.snapshot()).unwrap();
    let (mut wire, definitions): (KbcArtifact<PortableDefinitionRef>, PortableDefinitionTable) =
        codec().deserialize(&bytes).unwrap();
    wire.program.modules[0].host_interface.paths[0].root =
        codec().deserialize(&u32::MAX.to_le_bytes()).unwrap();
    let forged = codec().serialize(&(wire, definitions)).unwrap();
    assert!(KbcArtifact::from_bytes(&forged).is_err());
}

#[test]
fn required_host_fingerprint_is_derived_and_independent_of_docs_and_order() {
    use kagari_types::host_interface::{
        HostFunctionDeclaration, HostInterface, standard_log, value_type::HostValueType,
    };
    let context = DefinitionContext::new().unwrap();
    let cancel = CancellationToken::default();
    let log = standard_log()
        .unwrap()
        .import_into(&context, &cancel)
        .unwrap();
    let root = context
        .intern_root(&ModuleIdentity {
            package: PackageId("host".into()),
            path: vec!["host".into()],
        })
        .unwrap();
    let other = HostFunctionDeclaration {
        id: context
            .intern_child(root, DefinitionKind::Function, "other", 0)
            .unwrap(),
        symbol: "host.other".into(),
        params: vec![],
        return_type: HostValueType::Unit,
        effects: Default::default(),
        documentation: String::new(),
    };
    let interface = HostInterface {
        paths: vec![],
        types: vec![],
        functions: vec![log.into_records(), other],
    };
    let definitions = context.snapshot();
    let fingerprint = ArtifactFingerprint::of_host_interface(&interface, &definitions).unwrap();
    let mut reordered = interface.clone();
    reordered.functions.reverse();
    reordered.functions[0].documentation = "different docs".into();
    assert_eq!(
        fingerprint,
        ArtifactFingerprint::of_host_interface(&reordered, &definitions).unwrap()
    );
    let independent_context = DefinitionContext::new().unwrap();
    let independent_root = independent_context
        .intern_root(&ModuleIdentity {
            package: PackageId("host".into()),
            path: vec!["host".into()],
        })
        .unwrap();
    independent_context
        .intern_child(independent_root, DefinitionKind::Function, "other", 0)
        .unwrap();
    let independent = DefinitionMetadata::checked(definitions.clone(), reordered.clone(), &cancel)
        .unwrap()
        .import_into(&independent_context, &cancel)
        .unwrap();
    assert_eq!(
        fingerprint,
        ArtifactFingerprint::of_host_interface(independent.records(), independent.definitions())
            .unwrap()
    );
    reordered.functions[0].effects.may_trap = true;
    assert_ne!(
        fingerprint,
        ArtifactFingerprint::of_host_interface(&reordered, &definitions).unwrap()
    );
    let mut artifact = KbcArtifact::from_program(
        BytecodeProgram {
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule {
                host_interface: interface,
                ..Default::default()
            }],
        },
        Default::default(),
        &context,
    )
    .unwrap();
    assert_eq!(
        artifact.verification.host_interface_fingerprint,
        ArtifactFingerprint::of_program_hosts(&artifact.program, &definitions).unwrap()
    );
    artifact.verification.host_interface_fingerprint = ArtifactFingerprint::empty();
    artifact.header.content_hash = ArtifactFingerprint::empty();
    artifact.header.content_hash = ArtifactFingerprint::of_record(&artifact, &definitions).unwrap();
    assert!(matches!(
        artifact.validate_for_loader(&Default::default(), &context),
        Err(ArtifactValidationError::HostInterfaceFingerprintMismatch { .. })
    ));
}
