use kagari_common::SourceFile;
use kagari_embed::{
    ArtifactOptions, CompileOptions, EmbeddingError, ExecutionContext, KagariEngine, LoadOptions,
};
use kagari_ir::bytecode::{
    ArtifactBuildOptions, ArtifactCompatibility, ArtifactFingerprint, ArtifactModuleIdentity,
    ArtifactValidationError, DependencyFingerprint,
};
use kagari_runtime::value::Value;

fn exact_compatibility(
    artifact: &kagari_embed::BytecodeArtifact,
    identity: ArtifactModuleIdentity,
) -> ArtifactCompatibility {
    ArtifactCompatibility {
        module_identity: Some(identity),
        dependency_fingerprints: artifact.verification.loader.dependency_fingerprints.clone(),
        host_registry_fingerprint: artifact.verification.loader.host_registry_fingerprint,
        security_profile: artifact.verification.loader.security_profile.clone(),
        ..ArtifactCompatibility::default()
    }
}

#[test]
fn embedding_conformance_preserves_module_identity_through_artifact_loading() {
    let engine = KagariEngine::default();
    let identity = ArtifactModuleIdentity {
        package_id: "gameplay".to_owned(),
        module_path: "combat::main".to_owned(),
        source_uri: "pkg://gameplay/combat/main.kgr".to_owned(),
        module_id: "gameplay/combat/main".to_owned(),
    };
    let dependency = DependencyFingerprint {
        module_id: "gameplay/math".to_owned(),
        fingerprint: ArtifactFingerprint::of_str("math-v1"),
    };
    let checked = engine
        .compile_source(
            SourceFile::new("combat/main.kgr", "fn main() -> i32 { 7 }"),
            CompileOptions {
                module_identity: Some(identity.clone()),
                ..CompileOptions::default()
            },
        )
        .expect("source should compile");

    let artifact = engine
        .emit_bytecode(
            &checked,
            ArtifactOptions {
                build: ArtifactBuildOptions {
                    dependency_fingerprints: vec![dependency.clone()],
                    host_registry_fingerprint: ArtifactFingerprint::of_str("host-v1"),
                    security_profile: Some("dev".to_owned()),
                    ..ArtifactBuildOptions::default()
                },
                use_checked_module_identity: true,
            },
        )
        .expect("checked module should emit bytecode");

    assert_eq!(checked.module_identity, identity);
    assert_eq!(artifact.header.module_identity, identity);
    assert_eq!(artifact.verification.loader.module_identity, identity);
    assert_eq!(
        artifact.verification.loader.dependency_fingerprints,
        vec![dependency]
    );
    assert_eq!(
        artifact.verification.loader.host_registry_fingerprint,
        ArtifactFingerprint::of_str("host-v1")
    );
    assert_eq!(
        artifact.verification.loader.security_profile.as_deref(),
        Some("dev")
    );

    let compatibility = exact_compatibility(&artifact, identity.clone());
    artifact
        .validate_for_loader(&compatibility)
        .expect("artifact should satisfy exact loader compatibility");

    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_module(
            artifact,
            LoadOptions {
                compatibility,
                ..LoadOptions::default()
            },
        )
        .expect("compatible artifact should load");

    assert_eq!(loaded.name, identity.source_uri);
    assert_eq!(loaded.epoch.0, 1);
    assert_eq!(
        runtime
            .runtime()
            .modules()
            .latest(&identity.source_uri)
            .expect("loaded module should be visible by source uri")
            .id,
        loaded.id
    );
}

#[test]
fn embedding_conformance_rejects_incompatible_artifacts_before_publication() {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("stale.kgr", "fn main() -> i32 { 1 }"),
            CompileOptions::default(),
            ArtifactOptions::default(),
        )
        .expect("source should compile to an artifact");
    let mut incompatible = artifact.clone();
    incompatible.header.runtime_helper_abi_version = "wrong-helper-abi".to_owned();

    let mut runtime = engine.runtime(ExecutionContext::default());
    let error = runtime
        .load_module(incompatible, LoadOptions::default())
        .expect_err("incompatible artifact should be rejected before publication");

    assert!(matches!(
        error,
        EmbeddingError::ArtifactValidation {
            error: ArtifactValidationError::RuntimeHelperAbiMismatch { .. }
        }
    ));
    assert_eq!(runtime.runtime().modules().loaded_count(), 0);
}

#[test]
fn embedding_conformance_executes_core_builtin_surface() {
    let engine = KagariEngine::default();
    let context = ExecutionContext::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "builtins.kgr",
                r#"
fn main() -> (usize, usize) {
    val values = [1, 2];
    values.push(3);
    (values.len(), "ok".len())
}
"#,
            ),
            CompileOptions::default(),
            ArtifactOptions::default(),
        )
        .expect("builtin source should compile to artifact");
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_module(
            artifact,
            LoadOptions {
                module_name: Some("builtins".to_owned()),
                ..LoadOptions::default()
            },
        )
        .expect("builtin artifact should load");

    let report = runtime
        .execute(&loaded, "main", &[], &context)
        .expect("builtin surface should execute through embedding API");

    assert_eq!(
        report.return_value,
        Value::Tuple(vec![Value::I64(3), Value::I64(2)])
    );
}
