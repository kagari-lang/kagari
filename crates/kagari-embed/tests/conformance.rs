use kagari_common::SourceFile;
use kagari_embed::{
    ArtifactOptions, CompileOptions, EmbeddingError, ExecutionContext, KagariEngine, LoadOptions,
};
use kagari_ir::bytecode::{
    ArtifactBuildOptions, ArtifactCompatibility, ArtifactFingerprint, ArtifactModuleIdentity,
    ArtifactValidationError, DependencyFingerprint,
};
use kagari_runtime::{ResourcePolicy, value::Value};

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

    assert_eq!(error.code(), "KG_ARTIFACT_RUNTIME_HELPER_ABI_MISMATCH");
    assert!(matches!(
        error,
        EmbeddingError::ArtifactValidation {
            error: ArtifactValidationError::RuntimeHelperAbiMismatch { .. }
        }
    ));
    assert_eq!(runtime.runtime().modules().loaded_count(), 0);
}

#[test]
fn embedding_conformance_executes_standard_intrinsic_artifacts() {
    let engine = KagariEngine::default();
    let context = ExecutionContext::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "builtins.kgr",
                r#"
fn main() -> (usize, usize, usize, bool, i32) {
    val values = [1, 2];
    values.push(3);
    val map: Map<String, i32> = std::map::new();
    map.insert("ok", 7);
    val set: Set<String> = std::set::new();
    set.insert("ready");
    (values.len(), "ok".len_chars(), map.len(), set.contains("ready"), std::math::max(4, 7))
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
        Value::Tuple(vec![
            Value::I64(3),
            Value::I64(2),
            Value::I64(1),
            Value::Bool(true),
            Value::I32(7),
        ])
    );
}

#[test]
fn embedding_conformance_reloads_standard_intrinsic_artifacts() {
    let engine = KagariEngine::default();
    let context = ExecutionContext::default();
    let first = engine
        .compile_to_artifact(
            SourceFile::new(
                "stdlib_reload.kgr",
                r#"
pub fn main() -> usize {
    val values = [1];
    values.len()
}
"#,
            ),
            CompileOptions::default(),
            ArtifactOptions::default(),
        )
        .expect("first standard artifact should compile");
    let second = engine
        .compile_to_artifact(
            SourceFile::new(
                "stdlib_reload.kgr",
                r#"
pub fn main() -> usize {
    val values = [1, 2];
    values.len()
}
"#,
            ),
            CompileOptions::default(),
            ArtifactOptions::default(),
        )
        .expect("second standard artifact should compile");
    let mut invalid = second.clone();
    invalid.header.runtime_helper_abi_version = "wrong-helper-abi".to_owned();

    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_module(
            first,
            LoadOptions {
                module_name: Some("stdlib_reload".to_owned()),
                ..LoadOptions::default()
            },
        )
        .expect("first standard artifact should load");
    let reloaded = runtime
        .reload_module(
            &loaded,
            second,
            kagari_embed::ReloadOptions {
                module_name: Some("stdlib_reload".to_owned()),
                ..kagari_embed::ReloadOptions::default()
            },
        )
        .expect("compatible standard artifact should reload");
    let report = runtime
        .execute(&reloaded, "main", &[], &context)
        .expect("reloaded standard artifact should execute");
    assert_eq!(report.return_value, Value::I64(2));

    let failed_epoch = runtime
        .reload_module(
            &reloaded,
            invalid,
            kagari_embed::ReloadOptions {
                module_name: Some("stdlib_reload".to_owned()),
                ..kagari_embed::ReloadOptions::default()
            },
        )
        .expect_err("invalid standard artifact should fail before publication");
    assert_eq!(
        failed_epoch.code(),
        "KG_ARTIFACT_RUNTIME_HELPER_ABI_MISMATCH"
    );
    assert_eq!(
        runtime
            .runtime()
            .modules()
            .latest("stdlib_reload")
            .expect("latest module should remain published")
            .epoch,
        reloaded.epoch
    );
}

#[test]
fn embedding_conformance_surfaces_standard_intrinsic_resource_limits() {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "stdlib_resource.kgr",
                r#"
fn main() -> usize {
    val values = [1, 2];
    values.push(3);
    values.len()
}
"#,
            ),
            CompileOptions::default(),
            ArtifactOptions::default(),
        )
        .expect("standard resource source should compile");
    let context = ExecutionContext {
        resources: ResourcePolicy {
            max_instruction_steps: Some(1),
            ..ResourcePolicy::default()
        },
        ..ExecutionContext::default()
    };
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_module(
            artifact,
            LoadOptions {
                module_name: Some("stdlib_resource".to_owned()),
                ..LoadOptions::default()
            },
        )
        .expect("standard resource artifact should load");

    let error = runtime
        .execute(&loaded, "main", &[], &context)
        .expect_err("standard intrinsic execution should hit context resource limit");

    assert_eq!(error.code(), "KG_RUNTIME_RESOURCE_LIMIT_EXCEEDED");
    assert!(matches!(
        error,
        EmbeddingError::Runtime {
            kind: kagari_embed::RuntimeFailureKind::ResourceLimitExceeded,
            ..
        }
    ));
}
