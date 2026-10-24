use super::*;
use crate::{
    artifact::{ArtifactCompatibility, KbcArtifact},
    instruction::BytecodeInstruction,
    module::{BytecodeFunction, BytecodeModule, FunctionRecord, InstructionOrigin},
    program::{ModuleRef, updates},
};
use kagari_common::{
    identity::{ModuleIdentity, map::DefinitionContext},
    span::Span,
};
use kagari_contract::{
    ids::FunctionRef,
    source::{
        AuthoredDeclaration, AuthoredKind, ExecutableOrigin, SourceDocument,
        manifest::{SourceDocumentAnchor, SourceManifest},
        text_fingerprint,
    },
};

fn fixture() -> (VerifiedBytecodeProgram, DefinitionMetadata<SourcePackage>) {
    let context = DefinitionContext::new().unwrap();
    let mut function = BytecodeFunction {
        id: FunctionRef::new(0),
        name: "run".into(),
        instructions: vec![BytecodeInstruction::Return(None)],
        ..Default::default()
    };
    function.metadata.origins.push(InstructionOrigin {
        instruction: 0,
        span: Span::default(),
        line: None,
        column: None,
    });
    let module = BytecodeModule {
        identity: ModuleIdentity::single_file("source-free"),
        types: vec![function.metadata.return_type],
        function_table: vec![FunctionRecord {
            id: function.id,
            identity: None,
            name: function.name.clone(),
            params: vec![],
            return_type: function.metadata.return_type,
            effects: Default::default(),
        }],
        functions: vec![function],
        ..Default::default()
    };
    let mut program = updates::fixtures::certify(
        BytecodeProgram {
            root: ModuleRef::new(0),
            modules: vec![module],
            exports: Default::default(),
            update_evidence: updates::empty(),
        },
        &context,
    );
    let text = "// π\r\nfn run() {}";
    let declaration = program.update_evidence.sources[0].declaration;
    let package = SourcePackage {
        executable_fingerprint: 0,
        documents: vec![SourceDocument::new(
            vec![ModuleIdentity::single_file("source-free")],
            "source-free.kgr".into(),
            text.into(),
        )],
        declarations: vec![AuthoredDeclaration {
            declaration,
            kind: AuthoredKind::Function,
            document: 0,
            range: Span::new(text.find("fn").unwrap(), text.len()),
            fingerprint: text_fingerprint("fn run() {}"),
        }],
        executables: vec![],
    };
    program.update_evidence.source_manifest = Some(SourceManifest {
        documents: package
            .documents
            .iter()
            .map(SourceDocumentAnchor::from_document)
            .collect(),
        declarations: package.declarations.clone(),
    });
    let program = VerifiedBytecodeProgram::new(program, &context, &Default::default()).unwrap();
    let mut package = package;
    package.executable_fingerprint = program.product_identity().fingerprint().0;
    package.executables = executable_sources(&program, &Default::default()).unwrap();
    let package =
        DefinitionMetadata::checked(program.definitions().clone(), package, &Default::default())
            .unwrap();
    (program, package)
}

#[test]
fn source_free_optional_transport_preserves_scopes_text_and_executable_identity() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<VerifiedSourcePackage>();
    let cancel = CancellationToken::default();
    let (program, package) = fixture();
    let source = VerifiedSourcePackage::validate(package, &program, &cancel).unwrap();
    let artifact = KbcArtifact::from_verified(program, Default::default(), &cancel).unwrap();
    assert!(artifact.records().source.is_none());
    let stripped = artifact.records().to_bytes(artifact.definitions()).unwrap();
    let context = DefinitionContext::new().unwrap();
    let stripped = KbcArtifact::from_bytes(&stripped)
        .unwrap()
        .import_into(&context, &cancel)
        .unwrap()
        .into_records()
        .into_verified(&ArtifactCompatibility::default(), &context, &cancel)
        .unwrap();
    assert!(stripped.source_package().is_none());
    let artifact = KbcArtifact::with_source_package(artifact, &source, &cancel).unwrap();
    assert!(artifact.records().debug.is_none());
    assert!(artifact.records().portable_mir.is_none());
    let bytes = artifact.records().to_bytes(artifact.definitions()).unwrap();
    let fresh = DefinitionContext::new().unwrap();
    let loaded = KbcArtifact::from_bytes(&bytes)
        .unwrap()
        .import_into(&fresh, &cancel)
        .unwrap()
        .into_records()
        .into_verified(&ArtifactCompatibility::default(), &fresh, &cancel)
        .unwrap();
    let (program, mir, source) = loaded.into_parts();
    assert!(mir.is_none());
    let source = source.unwrap();
    assert_eq!(source.records().documents[0].text, "// π\r\nfn run() {}");
    assert_eq!(
        source.records().executable_fingerprint,
        ArtifactFingerprint::of_program(program.program(), program.definitions())
            .unwrap()
            .0
    );
    assert_eq!(
        source.records().executables[0].target,
        program.program().update_evidence.bodies[0]
            .requirement
            .origin
    );
    assert_eq!(
        source.records().executables[0].origin,
        ExecutableOrigin::Named
    );
}

#[test]
fn manifest_is_executable_identity_and_missing_anchors_forbid_attachment() {
    let cancel = CancellationToken::default();
    let (program, package) = fixture();
    let source = VerifiedSourcePackage::validate(package.clone(), &program, &cancel).unwrap();
    let mut changed = program.program().clone();
    changed
        .update_evidence
        .source_manifest
        .as_mut()
        .unwrap()
        .declarations[0]
        .fingerprint ^= 1;
    let context = DefinitionContext::new().unwrap();
    let changed = DefinitionMetadata::checked(program.definitions().clone(), changed, &cancel)
        .unwrap()
        .import_into(&context, &cancel)
        .unwrap();
    let changed = VerifiedBytecodeProgram::new(changed.into_records(), &context, &cancel).unwrap();
    assert_ne!(program.product_identity(), changed.product_identity());
    assert!(matches!(
        source.for_product(changed.product_identity(), changed.definitions(), &cancel),
        Err(SourcePackageError::ExecutableMismatch)
    ));

    let mut unsupported = program.program().clone();
    unsupported.update_evidence.source_manifest = None;
    let context = DefinitionContext::new().unwrap();
    let unsupported =
        DefinitionMetadata::checked(program.definitions().clone(), unsupported, &cancel)
            .unwrap()
            .import_into(&context, &cancel)
            .unwrap();
    let unsupported =
        VerifiedBytecodeProgram::new(unsupported.into_records(), &context, &cancel).unwrap();
    assert_ne!(program.product_identity(), unsupported.product_identity());
    let mut records = package.records().clone();
    records.executable_fingerprint = unsupported.product_identity().fingerprint().0;
    let package =
        DefinitionMetadata::checked(package.definitions().clone(), records, &cancel).unwrap();
    assert!(matches!(
        VerifiedSourcePackage::validate(package, &unsupported, &cancel),
        Err(SourcePackageError::MissingManifest)
    ));
}

#[test]
fn source_free_admission_rejects_duplicate_missing_and_forged_associations() {
    let (program, package) = fixture();
    let cancel = CancellationToken::default();
    let validate = |records| {
        VerifiedSourcePackage::validate(
            DefinitionMetadata::checked(package.definitions().clone(), records, &cancel).unwrap(),
            &program,
            &cancel,
        )
    };
    let mut bad = package.records().clone();
    bad.executables.push(bad.executables[0].clone());
    assert!(matches!(
        validate(bad),
        Err(SourcePackageError::OriginMismatch)
    ));
    let mut bad = package.records().clone();
    bad.executables.clear();
    assert!(matches!(
        validate(bad),
        Err(SourcePackageError::OriginMismatch)
    ));
    let mut bad = package.records().clone();
    bad.executables[0].authored = None;
    assert!(matches!(
        validate(bad),
        Err(SourcePackageError::OriginMismatch)
    ));
    let mut bad = package.records().clone();
    bad.declarations[0].range = Span::new(4, 5);
    assert!(matches!(
        validate(bad),
        Err(SourcePackageError::InvalidDocument)
    ));
    let mut bad = package.records().clone();
    bad.declarations[0].document = u32::MAX;
    assert!(matches!(
        validate(bad),
        Err(SourcePackageError::InvalidDocument)
    ));
    let mut bad = package.records().clone();
    bad.declarations.push(bad.declarations[0].clone());
    assert!(matches!(
        validate(bad),
        Err(SourcePackageError::DeclarationMismatch)
    ));
}
