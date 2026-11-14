use crate::{
    BytecodeArtifact,
    context::ExecutionContext,
    engine::{KagariEngine, source::ArtifactOptions},
    program::{Program, ProgramPreparationError},
    runtime::{LoadOptions, ReloadOptions},
};
use kagari_bytecode::artifact::{
    ArtifactBuildOptions, ArtifactCompatibility, ArtifactFingerprint, ArtifactValidationError,
    DependencyFingerprint,
};
use kagari_runtime::module::entry::ProgramEntry;
use kagari_runtime::reload::ReloadValidationError;
use kagari_stdlib as stdlib;

use {
    kagari_common::identity::{
        ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
    },
    kagari_source::{source::SourceFile, source_database::SourceLayer},
};

fn exact_compatibility(
    artifact: &BytecodeArtifact,
    identity: ModuleIdentity,
) -> ArtifactCompatibility {
    ArtifactCompatibility {
        module_identity: Some(identity),
        dependency_fingerprints: Some(artifact.verification.loader.dependency_fingerprints.clone()),

        ..ArtifactCompatibility::default()
    }
}

#[test]
fn embedding_conformance_preserves_module_identity_through_artifact_loading() {
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let identity = ModuleIdentity {
        package: PackageId("gameplay".into()),
        path: vec!["combat".into(), "main".into()],
    };
    let source_name = "pkg://gameplay/combat/main.kgr";
    engine.bind_module(source_name, identity.clone()).unwrap();
    let dependency_identity = ModuleIdentity {
        package: PackageId("gameplay".into()),
        path: vec!["math".into()],
    };
    engine
        .bind_module("pkg://gameplay/math.kgr", dependency_identity.clone())
        .unwrap();
    engine
        .set_source(
            "pkg://gameplay/math.kgr",
            "pub fn value() -> i32 { 42 }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let checked = engine
        .compile_source(SourceFile::new(
            source_name,
            "use gameplay::math; fn main() -> i32 { 7 }",
        ))
        .expect("source should compile");

    let artifact = engine
        .emit_artifact(
            &checked,
            ArtifactOptions {
                lowering: Default::default(),
                build: ArtifactBuildOptions {
                    ..ArtifactBuildOptions::default()
                },
                ..Default::default()
            },
        )
        .expect("checked module should emit bytecode");

    assert_eq!(checked.module_identity(), &identity);
    assert_eq!(artifact.records().header.module_identity, identity);
    assert_eq!(
        artifact.records().verification.loader.module_identity,
        identity
    );
    let mut expected_dependencies: Vec<_> = artifact
        .records()
        .program
        .modules
        .iter()
        .filter(|module| module.identity != identity)
        .map(|module| DependencyFingerprint {
            module_id: module.identity.clone(),
            fingerprint: ArtifactFingerprint::of_module(module, artifact.definitions()).unwrap(),
        })
        .collect();
    expected_dependencies.sort_by(|left, right| left.module_id.cmp(&right.module_id));
    assert!(
        expected_dependencies
            .iter()
            .any(|dependency| { dependency.module_id == dependency_identity })
    );
    assert_eq!(
        artifact
            .records()
            .verification
            .loader
            .dependency_fingerprints,
        expected_dependencies
    );

    let compatibility = exact_compatibility(artifact.records(), identity.clone());
    let definitions = DefinitionContext::new().unwrap();
    artifact
        .import_into(&definitions, &Default::default())
        .unwrap()
        .records()
        .validate_for_loader(&compatibility, &definitions, &Default::default())
        .expect("artifact should satisfy exact loader compatibility");

    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let program = Program::from_artifact(artifact, &compatibility, &context.cancellation)
        .expect("artifact should satisfy exact preparation compatibility");
    let loaded = runtime
        .load_program(&program, LoadOptions::default())
        .expect("compatible artifact should load");

    let selected = runtime.select_program(&loaded).unwrap();
    assert_eq!(selected.name, source_name);
    assert_eq!(selected.epoch.0, 1);
}

#[test]
fn preparation_rejects_incompatible_runtime_and_helper_abis() {
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("stale.kgr", "fn main() -> i32 { 1 }"),
            ArtifactOptions::default(),
        )
        .expect("source should compile to an artifact");
    let (definitions, mut incompatible) = artifact.clone().into_parts();
    incompatible.header.runtime_abi_version = "wrong-runtime-abi".to_owned();
    let incompatible =
        DefinitionMetadata::checked(definitions, incompatible, &Default::default()).unwrap();
    let error = Program::from_artifact(incompatible, &Default::default(), &Default::default())
        .expect_err("incompatible runtime ABI should be rejected during preparation");
    let ProgramPreparationError::Artifact(error) = error else {
        panic!("expected artifact validation error");
    };
    assert_eq!(error.code(), "KG_ARTIFACT_RUNTIME_ABI_MISMATCH");
    assert!(matches!(
        error,
        ArtifactValidationError::RuntimeAbiMismatch { .. }
    ));

    let (definitions, mut incompatible) = artifact.into_parts();
    incompatible.header.runtime_helper_abi_version = "wrong-helper-abi".to_owned();
    let incompatible =
        DefinitionMetadata::checked(definitions, incompatible, &Default::default()).unwrap();
    let error = Program::from_artifact(incompatible, &Default::default(), &Default::default())
        .expect_err("incompatible helper ABI should be rejected during preparation");
    let ProgramPreparationError::Artifact(error) = error else {
        panic!("expected artifact validation error");
    };
    assert_eq!(error.code(), "KG_ARTIFACT_RUNTIME_HELPER_ABI_MISMATCH");
    assert!(matches!(
        error,
        ArtifactValidationError::RuntimeHelperAbiMismatch { .. }
    ));
}

/* Temporarily paused: stdlib restoration (NC01-NC06).
#[test]
fn embedding_conformance_executes_foundation_native_artifacts() {
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let context = ExecutionContext::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "builtins.kgr",
                include_str!(
                    "../../../../tests/fixtures/embedding/installation/foundation-artifact.kgr"
                ),
            ),
            ArtifactOptions::default(),
        )
        .expect("builtin source should compile to artifact");
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(
            &Program::from_artifact(artifact, &Default::default(), &context.cancellation).unwrap(),
            LoadOptions {
                program_name: Some("builtins".to_owned()),
            },
        )
        .expect("builtin artifact should load");

    let report = runtime
        .execute(&loaded, ProgramEntry::Main, &[], &context)
        .expect("builtin surface should execute through embedding API");

    let owner = runtime.select_program(&loaded).unwrap();
    let actual = ConversionContext::new(&mut runtime, &owner)
        .unwrap()
        .decode::<(u64, u64, u64, bool, i32)>(&report.return_value)
        .unwrap();
    assert_eq!(actual, (3, 2, 1, true, 7));
}
*/

#[test]
fn embedding_conformance_reloads_standard_intrinsic_artifacts() {
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let context = ExecutionContext::default();
    let first = engine
        .compile_to_artifact(
            SourceFile::new(
                "stdlib_reload.kgr",
                r#"
pub fn main() -> u64 {
    val values = [1];
    values.len()
}
"#,
            ),
            ArtifactOptions::default(),
        )
        .expect("first standard artifact should compile");
    let second = engine
        .compile_to_artifact(
            SourceFile::new(
                "stdlib_reload.kgr",
                r#"
pub fn main() -> u64 {
    val values = [1, 2];
    values.len()
}
"#,
            ),
            ArtifactOptions::default(),
        )
        .expect("second standard artifact should compile");
    let (definitions, mut invalid) = second.clone().into_parts();
    invalid.header.runtime_helper_abi_version = "wrong-helper-abi".to_owned();
    let invalid = DefinitionMetadata::checked(definitions, invalid, &Default::default()).unwrap();

    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(
            &Program::from_artifact(first, &Default::default(), &context.cancellation).unwrap(),
            LoadOptions {
                program_name: Some("stdlib_reload".to_owned()),
            },
        )
        .expect("first standard artifact should load");
    runtime
        .program(&loaded)
        .map_err(ReloadValidationError::Runtime)
        .and_then(|base| {
            base.prepare_replacement(
                &Program::from_artifact(second, &Default::default(), &context.cancellation)
                    .unwrap(),
                &Default::default(),
            )
        })
        .and_then(|candidate| {
            runtime.replace_program(
                &loaded,
                &candidate,
                ReloadOptions {
                    program_name: Some("stdlib_reload".to_owned()),

                    ..Default::default()
                },
            )
        })
        .expect("compatible standard artifact should reload");
    let reloaded = runtime.select_program(&loaded).unwrap();
    let report = runtime
        .execute(&loaded, ProgramEntry::Main, &[], &context)
        .expect("reloaded standard artifact should execute");
    assert_eq!(report.return_value.as_u64().unwrap(), 2);

    let error = Program::from_artifact(invalid, &Default::default(), &context.cancellation)
        .expect_err("invalid standard artifact should fail before publication");
    let ProgramPreparationError::Artifact(error) = error else {
        panic!("expected artifact validation error");
    };
    assert_eq!(error.code(), "KG_ARTIFACT_RUNTIME_HELPER_ABI_MISMATCH");
    assert_eq!(
        runtime
            .select_program(&loaded)
            .expect("latest module should remain published")
            .epoch,
        reloaded.epoch
    );
}
