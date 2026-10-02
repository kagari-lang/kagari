use kagari_bytecode::artifact::{
    ArtifactBuildOptions, ArtifactCompatibility, ArtifactFingerprint, ArtifactValidationError,
    DependencyFingerprint,
};
use kagari_embed::{
    context::ExecutionContext,
    engine::{KagariEngine, source::ArtifactOptions},
    program::{PreparedProgram, ProgramPreparationError},
    runtime::{LoadOptions, ReloadOptions},
};

use kagari_common::{
    identity::{ModuleIdentity, PackageId},
    source::SourceFile,
};

use kagari_runtime::value::Value;

fn exact_compatibility(
    artifact: &kagari_embed::BytecodeArtifact,
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
        .compile_source(SourceFile::new(
            source_name,
            "use gameplay::math; fn main() -> i32 { 7 }",
        ))
        .expect("source should compile");

    let artifact = engine
        .emit_bytecode(
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
    assert_eq!(artifact.header.module_identity, identity);
    assert_eq!(artifact.verification.loader.module_identity, identity);
    let mut expected_dependencies: Vec<_> = artifact
        .program
        .modules
        .iter()
        .filter(|module| module.identity != identity)
        .map(|module| DependencyFingerprint {
            module_id: module.identity.clone(),
            fingerprint: ArtifactFingerprint::of_serialized(module),
        })
        .collect();
    expected_dependencies.sort_by(|left, right| left.module_id.cmp(&right.module_id));
    assert!(
        expected_dependencies
            .iter()
            .any(|dependency| { dependency.module_id == dependency_identity })
    );
    assert_eq!(
        artifact.verification.loader.dependency_fingerprints,
        expected_dependencies
    );
    assert_eq!(
        artifact.verification.host_interface_fingerprint,
        ArtifactFingerprint::of_program_hosts(&artifact.program)
    );

    let compatibility = exact_compatibility(&artifact, identity.clone());
    artifact
        .validate_for_loader(&compatibility)
        .expect("artifact should satisfy exact loader compatibility");

    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let program = PreparedProgram::from_artifact(artifact, &compatibility, &context.cancellation)
        .expect("artifact should satisfy exact preparation compatibility");
    let loaded = runtime
        .load_program(&program, LoadOptions::default())
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
            ArtifactOptions::default(),
        )
        .expect("source should compile to an artifact");
    let mut incompatible = artifact.clone();
    incompatible.header.runtime_helper_abi_version = "wrong-helper-abi".to_owned();

    let runtime = engine.runtime(ExecutionContext::default());
    let error =
        PreparedProgram::from_artifact(incompatible, &Default::default(), &Default::default())
            .expect_err("incompatible artifact should be rejected before publication");
    let ProgramPreparationError::Artifact(error) = error else {
        panic!("expected artifact validation error");
    };
    assert_eq!(error.code(), "KG_ARTIFACT_RUNTIME_HELPER_ABI_MISMATCH");
    assert!(matches!(
        error,
        ArtifactValidationError::RuntimeHelperAbiMismatch { .. }
    ));
    assert_eq!(runtime.runtime().modules().loaded_count(), 0);
}

#[test]
fn embedding_conformance_executes_foundation_native_artifacts() {
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
    val map: HashMap<String, i32> = HashMap::new();
    map.insert("ok", 7);
    val set: HashSet<String> = HashSet::new();
    set.insert("ready");
    (values.len(), values.len() - 1usize, map.len(), set.contains("ready"), match map.get("ok") { Some(x) => x, None => 0 })
}
"#,
            ),

            ArtifactOptions::default(),
        )
        .expect("builtin source should compile to artifact");
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(
            &PreparedProgram::from_artifact(artifact, &Default::default(), &context.cancellation)
                .unwrap(),
            LoadOptions {
                module_name: Some("builtins".to_owned()),
            },
        )
        .expect("builtin artifact should load");

    let report = runtime
        .execute(&loaded, "main", &[], &context)
        .expect("builtin surface should execute through embedding API");

    assert_eq!(
        report.return_value,
        Value::Tuple(vec![
            Value::U64(3),
            Value::U64(2),
            Value::U64(1),
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
            ArtifactOptions::default(),
        )
        .expect("second standard artifact should compile");
    let mut invalid = second.clone();
    invalid.header.runtime_helper_abi_version = "wrong-helper-abi".to_owned();

    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(
            &PreparedProgram::from_artifact(first, &Default::default(), &context.cancellation)
                .unwrap(),
            LoadOptions {
                module_name: Some("stdlib_reload".to_owned()),
            },
        )
        .expect("first standard artifact should load");
    let reloaded = runtime
        .reload_program(
            &loaded,
            &PreparedProgram::from_artifact(second, &Default::default(), &context.cancellation)
                .unwrap(),
            ReloadOptions {
                module_name: Some("stdlib_reload".to_owned()),
            },
        )
        .expect("compatible standard artifact should reload");
    let report = runtime
        .execute(&reloaded, "main", &[], &context)
        .expect("reloaded standard artifact should execute");
    assert_eq!(report.return_value, Value::U64(2));

    let error = PreparedProgram::from_artifact(invalid, &Default::default(), &context.cancellation)
        .expect_err("invalid standard artifact should fail before publication");
    let ProgramPreparationError::Artifact(error) = error else {
        panic!("expected artifact validation error");
    };
    assert_eq!(error.code(), "KG_ARTIFACT_RUNTIME_HELPER_ABI_MISMATCH");
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
