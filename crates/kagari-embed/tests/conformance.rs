use kagari_common::SourceFile;
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_embed::{
    ArtifactOptions, CompileOptions, EmbeddingError, ExecutionContext, KagariEngine, LoadOptions,
};
use kagari_ir::bytecode::{
    ArtifactBuildOptions, ArtifactCompatibility, ArtifactFingerprint, ArtifactValidationError,
    DependencyFingerprint,
};
use kagari_runtime::{ResourcePolicy, value::Value};

fn exact_compatibility(
    artifact: &kagari_embed::BytecodeArtifact,
    identity: ModuleIdentity,
) -> ArtifactCompatibility {
    ArtifactCompatibility {
        module_identity: Some(identity),
        dependency_fingerprints: Some(artifact.verification.loader.dependency_fingerprints.clone()),
        security_profile: artifact.verification.loader.security_profile.clone(),
        ..ArtifactCompatibility::default()
    }
}

#[test]
fn embedding_conformance_preserves_module_identity_through_artifact_loading() {
    let engine = KagariEngine::default();
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
            kagari_common::source_database::SourceLayer::Base,
        )
        .unwrap();
    let checked = engine
        .compile_source(
            SourceFile::new(source_name, "use gameplay::math; fn main() -> i32 { 7 }"),
            CompileOptions::default(),
        )
        .expect("source should compile");

    let artifact = engine
        .emit_bytecode(
            &checked,
            ArtifactOptions {
                lowering: Default::default(),
                build: ArtifactBuildOptions {
                    security_profile: Some("dev".to_owned()),
                    ..ArtifactBuildOptions::default()
                },
            },
        )
        .expect("checked module should emit bytecode");

    assert_eq!(checked.module_identity(), &identity);
    assert_eq!(artifact.header.module_identity, identity);
    assert_eq!(artifact.verification.loader.module_identity, identity);
    assert_eq!(
        artifact.verification.loader.dependency_fingerprints,
        vec![DependencyFingerprint {
            module_id: dependency_identity.clone(),
            fingerprint: ArtifactFingerprint::of_serialized(
                artifact
                    .program
                    .modules
                    .iter()
                    .find(|module| module.identity == dependency_identity)
                    .unwrap()
            )
        }]
    );
    assert_eq!(
        artifact.verification.host_interface_fingerprint,
        ArtifactFingerprint::of_program_hosts(&artifact.program)
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
        .load_program(
            artifact,
            LoadOptions {
                compatibility,
                ..LoadOptions::default()
            },
        )
        .expect("compatible artifact should load");

    assert_eq!(loaded.name, source_name);
    assert_eq!(loaded.epoch.0, 1);
    assert_eq!(
        runtime
            .runtime()
            .modules()
            .latest(source_name)
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
        .load_program(incompatible, LoadOptions::default())
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
        .load_program(
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
        .load_program(
            first,
            LoadOptions {
                module_name: Some("stdlib_reload".to_owned()),
                ..LoadOptions::default()
            },
        )
        .expect("first standard artifact should load");
    let reloaded = runtime
        .reload_program(
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
        .reload_program(
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
        .load_program(
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
