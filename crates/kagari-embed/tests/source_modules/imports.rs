use super::*;
use kagari_embed::program::PreparedProgram;

#[test]
fn public_source_glob_reexports_members_through_artifacts() {
    let engine = KagariEngine::default();
    insert(&engine, "model", "pub fn value() -> i32 { 42 }");
    insert(&engine, "facade", "pub use pkg::model::*;");
    let root = insert(
        &engine,
        "root",
        "use pkg::facade::*; fn main() -> i32 { value() }",
    );
    let artifact = compile(&engine, root, Default::default());
    for artifact in [
        artifact.clone(),
        BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap(),
    ] {
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime
            .load_program(&loaded_program, Default::default())
            .unwrap();
        assert_eq!(
            runtime
                .execute(&loaded, "main", &[], &context)
                .unwrap()
                .return_value,
            Value::I32(42)
        );
    }
}

#[test]
fn parent_module_can_execute_pub_super_child_function() {
    let engine = KagariEngine::default();
    let root = insert(
        &engine,
        "root",
        "mod child { pub(super) fn value() -> i32 { 42 } } fn main() -> i32 { child::value() }",
    );
    let artifact = compile(&engine, root, Default::default());
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded_program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
}

#[test]
fn imported_public_inherent_method_executes_from_source_and_artifact() {
    let engine = KagariEngine::default();
    insert(
        &engine,
        "model",
        "pub struct Counter { val value: i32 } impl Counter { pub fn read(self) -> i32 { self.value } fn hidden(self) -> i32 { 99 } } pub fn make() -> Counter { Counter { value: 42 } }",
    );
    let root = insert(
        &engine,
        "root",
        "use pkg::model::make; fn main() -> i32 { make().read() }",
    );
    let artifact = compile(&engine, root, Default::default());
    for artifact in [
        artifact.clone(),
        BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap(),
    ] {
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime
            .load_program(&loaded_program, Default::default())
            .unwrap();
        assert_eq!(
            runtime
                .execute(&loaded, "main", &[], &context)
                .unwrap()
                .return_value,
            Value::I32(42)
        );
    }
    let inaccessible = insert(
        &engine,
        "inaccessible",
        "use pkg::model::make; fn main() -> i32 { make().hidden() }",
    );
    let error = engine
        .compile_snapshot(
            engine.source_snapshot(),
            inaccessible,
            Default::default(),
            &Default::default(),
        )
        .unwrap_err();
    assert!(format!("{error:?}").contains("KG_RESOLVE_UNKNOWN_NAME"));
    let invalid_import = insert(
        &engine,
        "invalid_method_import",
        "use pkg::model::read; fn main() -> i32 { 0 }",
    );
    assert!(
        engine
            .compile_snapshot(
                engine.source_snapshot(),
                invalid_import,
                Default::default(),
                &Default::default()
            )
            .is_err()
    );
}

#[test]
fn parent_module_can_call_pub_super_inherent_method() {
    let engine = KagariEngine::default();
    let root = insert(
        &engine,
        "root",
        "mod child { pub(super) struct Data { val value: i32 } impl Data { pub(super) fn read(self) -> i32 { self.value } } pub(super) fn make() -> Data { Data { value: 42 } } } fn main() -> i32 { child::make().read() }",
    );
    let artifact = compile(&engine, root, Default::default());
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded_program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
}

#[test]
fn qualified_public_module_alias_does_not_expose_private_members() {
    let engine = KagariEngine::default();
    insert(
        &engine,
        "model",
        "pub fn visible() -> i32 { 42 } fn hidden() -> i32 { 99 }",
    );
    insert(&engine, "facade", "pub use pkg::model as api;");
    let root = insert(
        &engine,
        "root",
        "use pkg::facade as f; fn main() -> i32 { f::api::visible() }",
    );
    let artifact = compile(&engine, root, Default::default());
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded_program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
    let private = insert(
        &engine,
        "private",
        "use pkg::facade as f; fn main() -> i32 { f::api::hidden() }",
    );
    assert!(
        engine
            .compile_snapshot(
                engine.source_snapshot(),
                private,
                Default::default(),
                &Default::default()
            )
            .is_err()
    );
}

#[test]
fn wildcard_import_can_expose_a_public_inline_child_module() {
    let engine = KagariEngine::default();
    insert(
        &engine,
        "library",
        "pub mod child { pub fn value() -> i32 { 42 } }",
    );
    let root = insert(
        &engine,
        "root",
        "use pkg::library::*; fn main() -> i32 { child::value() }",
    );
    let artifact = compile(&engine, root, Default::default());
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded_program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
}

#[test]
fn wildcard_import_follows_a_public_module_alias() {
    let engine = KagariEngine::default();
    insert(
        &engine,
        "library",
        "pub mod child { pub fn value() -> i32 { 42 } }",
    );
    insert(&engine, "facade", "pub use pkg::library::child as api;");
    insert(&engine, "relay", "pub use pkg::facade::api as exported;");
    let root = insert(
        &engine,
        "root",
        "use pkg::relay::exported::*; fn main() -> i32 { value() }",
    );
    let artifact = compile(&engine, root, Default::default());
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded_program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
}

#[test]
fn wildcard_import_follows_a_public_standard_module_alias() {
    let engine = KagariEngine::default();
    insert(&engine, "facade", "pub use std::math;");
    let root = insert(
        &engine,
        "root",
        "use pkg::facade::math::*; fn main() -> i32 { min(42, 99) }",
    );
    let artifact = compile(&engine, root, Default::default());
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded_program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
}

#[test]
fn declared_external_child_module_resolves_qualified_calls() {
    let engine = KagariEngine::default();
    engine
        .bind_module(
            "mem://child",
            ModuleIdentity {
                package: PackageId("pkg".into()),
                path: vec!["root".into(), "child".into()],
            },
        )
        .unwrap();
    engine
        .set_source(
            "mem://child",
            "pub fn value() -> i32 { 42 }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let root = insert(
        &engine,
        "root",
        "mod child; fn main() -> i32 { child::value() }",
    );
    let artifact = compile(&engine, root, Default::default());
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded_program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
}

#[test]
fn duplicate_inline_and_external_module_identity_is_rejected() {
    let engine = KagariEngine::default();
    engine
        .bind_module(
            "mem://child",
            ModuleIdentity {
                package: PackageId("pkg".into()),
                path: vec!["root".into(), "child".into()],
            },
        )
        .unwrap();
    engine
        .set_source(
            "mem://child",
            "pub fn value() -> i32 { 1 }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let root = insert(
        &engine,
        "root",
        "mod child { pub fn value() -> i32 { 2 } } fn main() -> i32 { child::value() }",
    );
    let error = engine
        .compile_snapshot(
            engine.source_snapshot(),
            root,
            Default::default(),
            &Default::default(),
        )
        .unwrap_err();
    assert!(format!("{error:?}").contains("KG_RESOLVE_DUPLICATE_DECLARATION"));
}

#[test]
fn reachable_cycles_compile_without_initialization() {
    let engine = KagariEngine::default();
    let root = insert(&engine, "root", "use pkg::a; fn main() -> i32 { 7 }");
    insert(&engine, "a", "use pkg::b;");
    insert(&engine, "b", "use pkg::a;");
    let artifact = engine
        .compile_snapshot(
            engine.source_snapshot(),
            root,
            CompileOptions::default(),
            &CancellationToken::default(),
        )
        .unwrap();
    let mut user_modules: Vec<_> = artifact
        .program()
        .modules()
        .iter()
        .filter(|module| module.lowered.source.module_identity().package == PackageId("pkg".into()))
        .map(|module| module.lowered.source.module_identity().path.join("::"))
        .collect();
    user_modules.sort();
    assert_eq!(user_modules, ["a", "b", "root"]);
    let independent = insert(&engine, "independent", "fn main() -> i32 { 42 }");
    assert!(
        engine
            .compile_snapshot(
                engine.source_snapshot(),
                independent,
                CompileOptions::default(),
                &CancellationToken::default()
            )
            .is_ok()
    );
}
