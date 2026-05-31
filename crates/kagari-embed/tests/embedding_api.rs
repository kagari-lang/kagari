use kagari_common::SourceFile;
use kagari_embed::{
    ArtifactOptions, CompileOptions, EmbeddingError, ExecutionContext, HostExposurePolicy,
    KagariEngine, LoadOptions, ReloadOptions, RuntimeFailureKind,
};
use kagari_runtime::{CapabilitySet, LanguageProfile, ResourcePolicy, value::Value};

fn compile_artifact(
    engine: &KagariEngine,
    name: &str,
    source: &str,
) -> kagari_embed::BytecodeArtifact {
    engine
        .compile_to_artifact(
            SourceFile::new(name, source),
            CompileOptions::default(),
            ArtifactOptions::default(),
        )
        .expect("source should compile")
}

#[test]
fn compiles_loads_executes_and_reloads_through_embedding_api() {
    let engine = KagariEngine::default();
    let context = ExecutionContext::default();
    let first = compile_artifact(&engine, "game/main.kgr", "fn main() -> i32 { 1 }");
    let second = compile_artifact(&engine, "game/main.kgr", "fn main() -> i32 { 2 }");

    let mut runtime = engine.runtime(context);
    let loaded = runtime
        .load_module(
            first,
            LoadOptions {
                module_name: Some("game.main".to_owned()),
                ..LoadOptions::default()
            },
        )
        .expect("module should load");
    let report = runtime
        .execute(&loaded, "main", &[], &context)
        .expect("entry should execute");
    assert_eq!(report.return_value, Value::I32(1));

    let reloaded = runtime
        .reload_module(
            &loaded,
            second,
            ReloadOptions {
                module_name: Some("game.main".to_owned()),
                ..ReloadOptions::default()
            },
        )
        .expect("compatible module should reload");
    assert_eq!(reloaded.id, loaded.id);
    assert_eq!(reloaded.epoch.0, loaded.epoch.0 + 1);

    let report = runtime
        .execute(&reloaded, "main", &[], &context)
        .expect("reloaded entry should execute");
    assert_eq!(report.return_value, Value::I32(2));
}

#[test]
fn compile_failures_return_structured_diagnostics() {
    let engine = KagariEngine::default();
    let error = engine
        .compile_source(
            SourceFile::new("bad.kgr", "fn main( -> i32 { 1 }"),
            CompileOptions::default(),
        )
        .expect_err("parse failure should be structured");

    let EmbeddingError::Diagnostics { diagnostics } = error else {
        panic!("expected diagnostic error");
    };
    assert!(!diagnostics.is_empty());
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.code.is_empty())
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.message.is_empty())
    );
}

#[test]
fn analysis_failures_return_structured_diagnostics() {
    let engine = KagariEngine::default();
    let error = engine
        .compile_source(
            SourceFile::new("bad_type.kgr", "fn main() -> Missing { 1 }"),
            CompileOptions::default(),
        )
        .expect_err("analysis failure should be structured");

    let EmbeddingError::Diagnostics { diagnostics } = error else {
        panic!("expected diagnostic error");
    };
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.contains("UnknownType"))
    );
}

#[test]
fn execution_context_resource_limits_surface_as_runtime_failures() {
    let engine = KagariEngine::default();
    let context = ExecutionContext {
        resources: ResourcePolicy {
            max_instruction_steps: Some(1),
            ..ResourcePolicy::default()
        },
        ..ExecutionContext::default()
    };
    let artifact = compile_artifact(&engine, "limited.kgr", "fn main() -> i32 { 1 }");
    let mut runtime = engine.runtime(context);
    let loaded = runtime
        .load_module(
            artifact,
            LoadOptions {
                module_name: Some("limited".to_owned()),
                ..LoadOptions::default()
            },
        )
        .expect("module should load");

    let error = runtime
        .execute(&loaded, "main", &[], &context)
        .expect_err("execution should hit context resource limit");

    assert!(matches!(
        error,
        EmbeddingError::Runtime {
            kind: RuntimeFailureKind::ResourceLimitExceeded,
            ..
        }
    ));
}

#[test]
fn failed_reload_validation_does_not_publish_new_epoch() {
    let engine = KagariEngine::default();
    let context = ExecutionContext::default();
    let first = compile_artifact(&engine, "reload.kgr", "fn main() -> i32 { 1 }");
    let mut candidate = compile_artifact(&engine, "reload.kgr", "fn main() -> i32 { 2 }");
    candidate.header.runtime_abi_version = "wrong-runtime-abi".to_owned();

    let mut runtime = engine.runtime(context);
    let loaded = runtime
        .load_module(
            first,
            LoadOptions {
                module_name: Some("reload".to_owned()),
                ..LoadOptions::default()
            },
        )
        .expect("module should load");
    let before_count = runtime.runtime().modules().loaded_count();

    let error = runtime
        .reload_module(
            &loaded,
            candidate,
            ReloadOptions {
                module_name: Some("reload".to_owned()),
                ..ReloadOptions::default()
            },
        )
        .expect_err("invalid artifact should not reload");

    assert!(matches!(error, EmbeddingError::ReloadValidation { .. }));
    assert_eq!(runtime.runtime().modules().loaded_count(), before_count);
    assert_eq!(
        runtime.runtime().modules().latest("reload").unwrap().epoch,
        loaded.epoch
    );
}

#[test]
fn execute_entry_accepts_args_boundary_and_rejects_unimplemented_arguments() {
    let engine = KagariEngine::default();
    let context = ExecutionContext::default();
    let artifact = compile_artifact(&engine, "args.kgr", "fn main() -> i32 { 1 }");
    let mut runtime = engine.runtime(context);
    let loaded = runtime
        .load_module(
            artifact,
            LoadOptions {
                module_name: Some("args".to_owned()),
                ..LoadOptions::default()
            },
        )
        .expect("module should load");

    let error = runtime
        .execute(&loaded, "main", &[Value::I32(1)], &context)
        .expect_err("argument passing is not implemented yet");

    assert!(matches!(
        error,
        EmbeddingError::Runtime {
            kind: RuntimeFailureKind::UnsupportedExecution,
            ..
        }
    ));
}

#[test]
fn execution_context_denies_host_and_reflection_helpers() {
    let engine = KagariEngine::default();
    let print_artifact = compile_artifact(&engine, "print.kgr", r#"fn main() { print("x"); }"#);
    let type_of_artifact = compile_artifact(
        &engine,
        "type_of.kgr",
        r#"fn main() -> String { type_of(7) }"#,
    );
    let mut runtime = engine.runtime(ExecutionContext::default());
    let print_module = runtime
        .load_module(
            print_artifact,
            LoadOptions {
                module_name: Some("print".to_owned()),
                ..LoadOptions::default()
            },
        )
        .expect("print module should load");
    let type_of_module = runtime
        .load_module(
            type_of_artifact,
            LoadOptions {
                module_name: Some("type_of".to_owned()),
                ..LoadOptions::default()
            },
        )
        .expect("type_of module should load");

    let host_denied = ExecutionContext {
        host_policy: HostExposurePolicy {
            allow_host_functions: false,
            ..HostExposurePolicy::default()
        },
        ..ExecutionContext::default()
    };
    let error = runtime
        .execute(&print_module, "main", &[], &host_denied)
        .expect_err("host function exposure should be denied");
    assert!(matches!(
        error,
        EmbeddingError::Runtime {
            kind: RuntimeFailureKind::CapabilityDenied,
            ..
        }
    ));

    let error = runtime
        .execute(&type_of_module, "main", &[], &ExecutionContext::default())
        .expect_err("reflection is denied by default context");
    assert!(matches!(
        error,
        EmbeddingError::Runtime {
            kind: RuntimeFailureKind::CapabilityDenied,
            ..
        }
    ));

    let reflection_allowed = ExecutionContext {
        language_profile: LanguageProfile {
            allow_reflection: true,
            ..LanguageProfile::default()
        },
        capabilities: CapabilitySet {
            reflection_read: true,
            ..CapabilitySet::default()
        },
        ..ExecutionContext::default()
    };
    let report = runtime
        .execute(&type_of_module, "main", &[], &reflection_allowed)
        .expect("reflection read should execute when profile and capability allow it");
    assert_eq!(report.return_value, Value::Str("i32".to_owned()));
}
