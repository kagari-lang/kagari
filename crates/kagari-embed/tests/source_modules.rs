use kagari_common::{
    cancellation::CancellationToken,
    identity::{FileId, ModuleIdentity, PackageId},
    source_database::SourceLayer,
};
use kagari_embed::{
    BytecodeArtifact, CompileOptions, EmbeddingError, ExecutionContext, KagariEngine,
};
use kagari_runtime::value::Value;

fn compile(engine: &KagariEngine, root: FileId, options: CompileOptions) -> BytecodeArtifact {
    let checked = engine
        .compile_snapshot(engine.source_snapshot(), root, options, &Default::default())
        .unwrap();
    engine.emit_bytecode(&checked, Default::default()).unwrap()
}

fn insert(engine: &KagariEngine, name: &str, text: &str) -> FileId {
    let source = format!("mem://{name}");
    engine
        .bind_module(
            &source,
            ModuleIdentity {
                package: PackageId("pkg".into()),
                path: vec![name.into()],
            },
        )
        .unwrap();
    engine
        .set_source(&source, text.into(), SourceLayer::Base)
        .unwrap()
}

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
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
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
    let loaded = runtime.load_program(artifact, Default::default()).unwrap();
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
    let loaded = runtime.load_program(artifact, Default::default()).unwrap();
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
    insert(&engine, "facade", "pub use std::math as math;");
    let root = insert(
        &engine,
        "root",
        "use pkg::facade::math::*; fn main() -> i32 { min(42, 99) }",
    );
    let artifact = compile(&engine, root, Default::default());
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(artifact, Default::default()).unwrap();
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
    let loaded = runtime.load_program(artifact, Default::default()).unwrap();
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
    assert_eq!(artifact.program().modules().len(), 3);
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

#[test]
fn source_and_encoded_programs_execute_transitive_calls_and_shared_struct_layouts() {
    let engine = KagariEngine::default();
    insert(
        &engine,
        "shared",
        "pub struct Data { var x: i32 } fn id<T>(x: T) -> T { x } pub fn make(x: i32) -> Data { Data { x: id(x) } } pub fn add(p: Data, x: i32) -> i32 { p.x += x; p.x }",
    );
    insert(&engine, "left", "pub use pkg::shared::make;");
    insert(&engine, "right", "pub use pkg::shared::add;");
    let root = insert(
        &engine,
        "root",
        "use pkg::left::make; use pkg::right::add; fn id() -> bool { true } fn main() -> i32 { val p = make(20); val n = add(p, 22); p.x }",
    );
    let artifact = compile(&engine, root, Default::default());
    assert_eq!(artifact.program.modules.len(), 4);
    assert_eq!(artifact.verification.dependency_fingerprints.len(), 3);
    let encoded = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    for artifact in [artifact, encoded] {
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
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
fn imported_closure_keeps_its_defining_module_and_capture_state() {
    let engine = KagariEngine::default();
    insert(
        &engine,
        "provider",
        r#"
pub fn make() -> fn() -> i32 {
    var count = 40;
    || { count = count + 1; count }
}
"#,
    );
    let root = insert(
        &engine,
        "root",
        r#"
use pkg::provider::make;
fn main() -> i32 {
    val next = make();
    next();
    next()
}
"#,
    );
    let artifact = compile(&engine, root, Default::default());
    let encoded = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    for artifact in [artifact, encoded] {
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
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
fn imported_applied_trait_impl_runs_through_source_artifact_and_jit() {
    let engine = KagariEngine::default();
    insert(
        &engine,
        "api",
        include_str!("../../../examples/imported-traits/api.kgr"),
    );
    let root = insert(
        &engine,
        "root",
        include_str!("../../../examples/imported-traits/main.kgr"),
    );
    let mut context = ExecutionContext::default();
    context.language_profile.allow_jit = true;
    context.capabilities.jit = true;
    let artifact = compile(
        &engine,
        root,
        CompileOptions {
            language_profile: context.language_profile,
        },
    );
    let mut wrong_contract = artifact.program.clone();
    let api = wrong_contract
        .modules
        .iter_mut()
        .find(|module| module.identity.path == ["api"])
        .unwrap();
    let method = api.public_items.iter_mut().find_map(|item| match item {
        kagari_ir::module::PublicAbiItem::Trait(interface) => interface.methods.first_mut(),
        _ => None,
    });
    method.unwrap().return_type =
        kagari_ir::module::abi::AbiType::Builtin(kagari_hir::types::BuiltinType::Bool);
    assert!(matches!(
        kagari_ir::bytecode::verify_program(&wrong_contract),
        Err(kagari_ir::bytecode::BytecodeVerificationError::InvalidInterfaceTable)
    ));
    assert!(matches!(
        kagari_ir::bytecode::KbcArtifact::from_program(wrong_contract, Default::default()),
        Err(kagari_ir::bytecode::ArtifactValidationError::Bytecode(
            kagari_ir::bytecode::BytecodeVerificationError::InvalidInterfaceTable
        ))
    ));
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        context.jit_policy = if jit {
            kagari_embed::JitPolicy::Enabled
        } else {
            kagari_embed::JitPolicy::Disabled
        };
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
        let report = if jit {
            let mut backend = kagari_jit_cranelift::CraneliftBackend::for_host().unwrap();
            runtime.execute_with_backend(&loaded, "main", &[], &context, &mut backend)
        } else {
            runtime.execute(&loaded, "main", &[], &context)
        }
        .unwrap();
        assert_eq!(report.return_value, Value::I32(7));
    }
}

#[test]
fn imported_generic_trait_method_specializes_across_execution_routes() {
    let engine = KagariEngine::default();
    insert(
        &engine,
        "api",
        "pub trait Echo { fn echo<U>(self, value: U) -> U; }",
    );
    insert(
        &engine,
        "model",
        "use pkg::api::Echo; pub struct Holder<T> { val marker: T } impl<T> Echo for Holder<T> { fn echo<V>(self, value: V) -> V { value } } pub fn make() -> Holder<bool> { Holder { marker: true } }",
    );
    let root = insert(
        &engine,
        "root",
        "use pkg::api::Echo; use pkg::model::make; fn read<T: Echo>(value: T) -> i32 { value.echo(42) } fn main() -> i32 { read(make()) }",
    );
    let mut context = ExecutionContext::default();
    context.language_profile.allow_jit = true;
    context.capabilities.jit = true;
    let artifact = compile(
        &engine,
        root,
        CompileOptions {
            language_profile: context.language_profile,
        },
    );
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        context.jit_policy = if jit {
            kagari_embed::JitPolicy::Enabled
        } else {
            kagari_embed::JitPolicy::Disabled
        };
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
        let report = if jit {
            let mut backend = kagari_jit_cranelift::CraneliftBackend::for_host().unwrap();
            runtime.execute_with_backend(&loaded, "main", &[], &context, &mut backend)
        } else {
            runtime.execute(&loaded, "main", &[], &context)
        }
        .unwrap();
        assert_eq!(report.return_value, Value::I32(42));
    }
}

#[test]
fn dependency_defined_trait_impl_dispatches_through_bound_call() {
    let engine = KagariEngine::default();
    insert(
        &engine,
        "api",
        include_str!("../../../examples/imported-traits/api.kgr"),
    );
    insert(
        &engine,
        "model",
        include_str!("../../../examples/imported-traits/model.kgr"),
    );
    let root = insert(
        &engine,
        "root",
        include_str!("../../../examples/imported-traits/consumer.kgr"),
    );
    let mut context = ExecutionContext::default();
    context.language_profile.allow_jit = true;
    context.capabilities.jit = true;
    let artifact = compile(
        &engine,
        root,
        CompileOptions {
            language_profile: context.language_profile,
        },
    );
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        context.jit_policy = if jit {
            kagari_embed::JitPolicy::Enabled
        } else {
            kagari_embed::JitPolicy::Disabled
        };
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
        let report = if jit {
            let mut backend = kagari_jit_cranelift::CraneliftBackend::for_host().unwrap();
            runtime.execute_with_backend(&loaded, "main", &[], &context, &mut backend)
        } else {
            runtime.execute(&loaded, "main", &[], &context)
        }
        .unwrap();
        assert_eq!(report.return_value, Value::I32(9));
    }
}

#[test]
fn ambiguous_dependency_trait_implementations_reject_bound_call() {
    let engine = KagariEngine::default();
    insert(
        &engine,
        "api",
        include_str!("../../../examples/imported-traits/api.kgr"),
    );
    insert(
        &engine,
        "model",
        include_str!("../../../examples/imported-traits/model.kgr"),
    );
    insert(
        &engine,
        "duplicate",
        "use pkg::api::Echo; use pkg::model::Holder; impl Echo<i32> for Holder { fn get(self) -> i32 { 10 } }",
    );
    let root = insert(
        &engine,
        "root",
        "use pkg::api::Echo; use pkg::model::make; use pkg::duplicate; fn read<U: Echo<i32>>(value: U) -> i32 { value.get() } fn main() -> i32 { read(make()) }",
    );
    let error = engine
        .compile_snapshot(
            engine.source_snapshot(),
            root,
            CompileOptions::default(),
            &CancellationToken::default(),
        )
        .unwrap_err();
    let EmbeddingError::Diagnostics { diagnostics } = error else {
        panic!("expected source diagnostics");
    };
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "KG_TYPE_GENERIC_BOUND_NOT_SATISFIED"
            && diagnostic
                .span
                .is_some_and(|location| location.file == root)
    }));
}

#[test]
fn sibling_dependency_implementations_reject_even_when_unused_and_recover_after_edit() {
    let engine = KagariEngine::default();
    insert(
        &engine,
        "api",
        include_str!("../../../examples/imported-traits/api.kgr"),
    );
    insert(
        &engine,
        "model",
        "pub struct Holder { val number: i32 } pub fn make() -> Holder { Holder { number: 9 } }",
    );
    insert(
        &engine,
        "a",
        "use pkg::api::Echo; use pkg::model::Holder; impl Echo<i32> for Holder { fn get(self) -> i32 { self.number } }",
    );
    insert(
        &engine,
        "b",
        "use pkg::api::Echo; use pkg::model::Holder; impl Echo<i32> for Holder { fn get(self) -> i32 { self.number + 1 } }",
    );
    let root = insert(
        &engine,
        "root",
        "use pkg::a; use pkg::b; fn main() -> i32 { 1 }",
    );
    let before = engine.source_snapshot();
    let compile_from = |sources| {
        engine.compile_snapshot(
            sources,
            root,
            CompileOptions::default(),
            &CancellationToken::default(),
        )
    };
    let error = compile_from(before.clone()).unwrap_err();
    let EmbeddingError::Diagnostics { diagnostics } = error else {
        panic!("expected source diagnostics");
    };
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "KG_TYPE_INVALID_TRAIT_IMPL"
            && diagnostic
                .span
                .is_some_and(|location| location.file == root)
    }));

    engine
        .set_source(
            "mem://b",
            "pub fn helper() -> i32 { 2 }".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    assert!(compile_from(engine.source_snapshot()).is_ok());
    assert!(compile_from(before).is_err());
}

#[test]
fn dependency_generic_implementation_overlap_respects_trait_arguments() {
    let engine = KagariEngine::default();
    insert(
        &engine,
        "api",
        include_str!("../../../examples/imported-traits/api.kgr"),
    );
    insert(&engine, "model", "pub struct Holder<T> { val value: T }");
    insert(
        &engine,
        "a",
        "use pkg::api::Echo; use pkg::model::Holder; impl<T> Echo<i32> for Holder<T> { fn get(self) -> i32 { 1 } }",
    );
    insert(
        &engine,
        "b",
        "use pkg::api::Echo; use pkg::model::Holder; impl Echo<i32> for Holder<i32> { fn get(self) -> i32 { 2 } }",
    );
    let root = insert(
        &engine,
        "root",
        "use pkg::a; use pkg::b; fn main() -> i32 { 3 }",
    );
    let before = engine.source_snapshot();
    let compile_from = |sources| {
        engine.compile_snapshot(
            sources,
            root,
            CompileOptions::default(),
            &CancellationToken::default(),
        )
    };
    let error = compile_from(before.clone()).unwrap_err();
    let EmbeddingError::Diagnostics { diagnostics } = error else {
        panic!("expected source diagnostics");
    };
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "KG_TYPE_INVALID_TRAIT_IMPL"
            && diagnostic
                .span
                .is_some_and(|location| location.file == root)
    }));

    engine
        .set_source(
            "mem://b",
            "use pkg::api::Echo; use pkg::model::Holder; impl Echo<bool> for Holder<i32> { fn get(self) -> bool { true } }".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    assert!(compile_from(engine.source_snapshot()).is_ok());
    assert!(compile_from(before).is_err());
}

#[test]
fn dependency_generic_implementation_is_specialized_for_reachable_calls() {
    let engine = KagariEngine::default();
    insert(
        &engine,
        "api",
        include_str!("../../../examples/imported-traits/api.kgr"),
    );
    insert(
        &engine,
        "model",
        include_str!("../../../examples/imported-traits/generic-model.kgr"),
    );
    let root = insert(
        &engine,
        "root",
        include_str!("../../../examples/imported-traits/generic-consumer.kgr"),
    );
    let mut context = ExecutionContext::default();
    context.language_profile.allow_jit = true;
    context.capabilities.jit = true;
    let artifact = compile(
        &engine,
        root,
        CompileOptions {
            language_profile: context.language_profile,
        },
    );
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        context.jit_policy = if jit {
            kagari_embed::JitPolicy::Enabled
        } else {
            kagari_embed::JitPolicy::Disabled
        };
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
        let report = if jit {
            let mut backend = kagari_jit_cranelift::CraneliftBackend::for_host().unwrap();
            runtime.execute_with_backend(&loaded, "main", &[], &context, &mut backend)
        } else {
            runtime.execute(&loaded, "main", &[], &context)
        }
        .unwrap();
        assert_eq!(report.return_value, Value::I32(18));
    }
}

#[test]
fn dependency_generic_implementation_checks_specialized_bounds() {
    let engine = KagariEngine::default();
    insert(
        &engine,
        "api",
        include_str!("../../../examples/imported-traits/api.kgr"),
    );
    insert(
        &engine,
        "model",
        "use pkg::api::Echo; pub struct Holder<T> { val value: T } impl<T: HashKey> Echo<i32> for Holder<T> { fn get(self) -> i32 { 9 } } pub fn good() -> Holder<i32> { Holder { value: 1 } } pub fn bad() -> Holder<f32> { Holder { value: 1.5 } }",
    );
    let root = insert(
        &engine,
        "root",
        "use pkg::api::Echo; use pkg::model::{good, bad}; fn read<U: Echo<i32>>(value: U) -> i32 { value.get() } fn main() -> i32 { read(good()) }",
    );
    assert!(
        engine
            .compile_snapshot(
                engine.source_snapshot(),
                root,
                CompileOptions::default(),
                &CancellationToken::default(),
            )
            .is_ok()
    );
    engine
        .set_source(
            "mem://root",
            "use pkg::api::Echo; use pkg::model::{good, bad}; fn read<U: Echo<i32>>(value: U) -> i32 { value.get() } fn main() -> i32 { read(bad()) }".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    let error = engine
        .compile_snapshot(
            engine.source_snapshot(),
            root,
            CompileOptions::default(),
            &CancellationToken::default(),
        )
        .unwrap_err();
    let EmbeddingError::Diagnostics { diagnostics } = error else {
        panic!("expected source diagnostics");
    };
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "KG_TYPE_GENERIC_BOUND_NOT_SATISFIED")
    );
}

#[test]
fn dependency_generic_instances_follow_transitive_method_calls() {
    let engine = KagariEngine::default();
    insert(
        &engine,
        "api",
        include_str!("../../../examples/imported-traits/api.kgr"),
    );
    insert(
        &engine,
        "inner",
        "use pkg::api::Echo; pub struct Inner<T> { val value: T } impl<T> Echo<i32> for Inner<T> { fn get(self) -> i32 { 4 } } pub fn make() -> Inner<i32> { Inner { value: 1 } }",
    );
    insert(
        &engine,
        "outer",
        "use pkg::api::Echo; use pkg::inner::{Inner, make}; pub struct Outer<T> { val inner: Inner<T> } fn read_inner<U: Echo<i32>>(value: U) -> i32 { value.get() } impl<T> Echo<i32> for Outer<T> { fn get(self) -> i32 { read_inner(self.inner) } } pub fn make_outer() -> Outer<i32> { Outer { inner: make() } }",
    );
    let root = insert(
        &engine,
        "root",
        "use pkg::api::Echo; use pkg::outer::make_outer; fn read<U: Echo<i32>>(value: U) -> i32 { value.get() } fn main() -> i32 { read(make_outer()) }",
    );
    let artifact = compile(&engine, root, CompileOptions::default());
    let mut runtime = engine.runtime(Default::default());
    let loaded = runtime.load_program(artifact, Default::default()).unwrap();
    let report = runtime
        .execute(&loaded, "main", &[], &ExecutionContext::default())
        .unwrap();
    assert_eq!(report.return_value, Value::I32(4));
}

#[test]
fn facade_call_signatures_supply_context_to_nominal_constructors() {
    let engine = KagariEngine::default();
    insert(
        &engine,
        "types",
        "pub struct Marker<T> { val value: i32 } pub enum Token<T> { Empty } pub fn take(value: Marker<i32>, token: Token<bool>) -> i32 { value.value }",
    );
    insert(
        &engine,
        "facade",
        "pub use pkg::types::{Marker, Token, take};",
    );
    let root = insert(
        &engine,
        "root",
        "use pkg::facade::{Marker, Token, take}; fn main() -> i32 { val explicit = Marker<i32> { value: 20 }; take(Marker { value: 22 }, Token<bool>::Empty) + explicit.value }",
    );
    let mut context = ExecutionContext::default();
    context.language_profile.allow_jit = true;
    context.capabilities.jit = true;
    let artifact = compile(
        &engine,
        root,
        CompileOptions {
            language_profile: context.language_profile,
        },
    );
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        context.jit_policy = if jit {
            kagari_embed::JitPolicy::Enabled
        } else {
            kagari_embed::JitPolicy::Disabled
        };
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
        let report = if jit {
            let mut backend = kagari_jit_cranelift::CraneliftBackend::for_host().unwrap();
            runtime.execute_with_backend(&loaded, "main", &[], &context, &mut backend)
        } else {
            runtime.execute(&loaded, "main", &[], &context)
        }
        .unwrap();
        assert_eq!(report.return_value, Value::I32(42));
    }
}

#[test]
fn unused_dependency_body_errors_prevent_compilation_with_owned_locations() {
    let engine = KagariEngine::default();
    let dependency = insert(
        &engine,
        "dependency",
        "pub fn good() -> i32 { 42 } fn broken() -> i32 { false }",
    );
    let root = insert(
        &engine,
        "root",
        "use pkg::dependency; fn main() -> i32 { 7 }",
    );
    let source = engine.source_snapshot();
    let error = engine
        .compile_snapshot(
            source.clone(),
            root,
            Default::default(),
            &Default::default(),
        )
        .unwrap_err();
    let EmbeddingError::Diagnostics { diagnostics } = error else {
        panic!("expected dependency diagnostics")
    };
    assert!(!diagnostics.is_empty());
    for diagnostic in diagnostics {
        let span = diagnostic.span.unwrap();
        assert_eq!(span.file, dependency);
        assert!(source.contains(span));
    }
}

fn host_fixture() -> (
    KagariEngine,
    BytecodeArtifact,
    ExecutionContext,
    kagari_common::host_interface::HostFunctionDeclaration,
) {
    use kagari_common::host_interface::{
        HostFunctionDeclaration, HostInterface, HostParameter, HostPassingStyle, HostValueType,
    };
    let engine = KagariEngine::default();
    let declaration = HostFunctionDeclaration::new(
        "trace.record",
        vec![HostParameter {
            name: "value".into(),
            ty: HostValueType::I32,
            passing: HostPassingStyle::Owned,
        }],
        HostValueType::I32,
    );
    engine
        .set_host_interface(HostInterface {
            paths: vec![],
            types: Vec::new(),
            functions: vec![declaration.clone()],
        })
        .unwrap();
    insert(
        &engine,
        "shared",
        "pub fn value() -> i32 { trace::record(1); 21 }",
    );
    insert(
        &engine,
        "left",
        "use pkg::shared::value; pub fn left() -> i32 { trace::record(2); value() }",
    );
    insert(
        &engine,
        "right",
        "use pkg::shared; pub fn right() -> i32 { trace::record(3); 21 }",
    );
    let root = insert(
        &engine,
        "root",
        "use pkg::left::left; use pkg::right::right; fn main() -> i32 { trace::record(4); left() + right() }",
    );
    let context = ExecutionContext {
        language_profile: kagari_runtime::LanguageProfile {
            allow_host_calls: true,
            ..Default::default()
        },
        capabilities: kagari_runtime::CapabilitySet {
            host_calls: true,
            ..Default::default()
        },
        host_policy: kagari_runtime::HostExposurePolicy {
            allowed_host_functions: vec!["trace.record".into()],
            ..Default::default()
        },
        ..Default::default()
    };
    let artifact = compile(
        &engine,
        root,
        CompileOptions {
            language_profile: context.language_profile,
        },
    );
    (engine, artifact, context, declaration)
}

#[test]
fn execution_report_records_code_inputs_and_ordered_host_results() {
    let (engine, artifact, mut context, declaration) = host_fixture();
    context.tracing_enabled = true;
    context.inputs.unix_time_millis = 31_415;
    context.inputs.random_seed = 27;
    let decoded = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let mut traces = Vec::new();
    for artifact in [artifact, decoded] {
        let mut runtime = engine.runtime(context.clone());
        runtime
            .register_host_function(kagari_runtime::host::HostFunction::new(
                declaration.clone(),
                |_, args| Ok(args[0].clone()),
            ))
            .unwrap();
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
        let report = runtime.execute(&loaded, "main", &[], &context).unwrap();
        assert_eq!(report.return_value, Value::I32(42));
        let trace = report.trace.unwrap();
        assert_eq!(trace.code_fingerprint, loaded.program_fingerprint());
        assert_eq!(trace.inputs, context.inputs);
        assert_eq!(trace.host_calls.len(), 4);
        let observed = trace
            .host_calls
            .iter()
            .map(|call| (call.arguments.clone(), call.outcome.clone()))
            .collect::<Vec<_>>();
        let expected = [4, 2, 1, 3]
            .into_iter()
            .map(|value| {
                let value = kagari_runtime::TraceValue::I32(value);
                (vec![value.clone()], Some(Ok(value)))
            })
            .collect::<Vec<_>>();
        assert_eq!(observed, expected);
        traces.push(trace);
    }
    assert_eq!(traces[0], traces[1]);
}

#[test]
fn dependency_bindings_and_execution_policy_are_checked_before_execution() {
    use std::sync::{Arc, Mutex};
    let (engine, _, context, declaration) = host_fixture();
    let root = insert(
        &engine,
        "root",
        "use pkg::left; use pkg::right; fn main() -> i32 { 42 }",
    );
    let artifact = compile(
        &engine,
        root,
        CompileOptions {
            language_profile: context.language_profile,
        },
    );
    assert!(
        artifact.program.modules[artifact.program.root.index()]
            .host_interface
            .functions
            .is_empty()
    );
    let mut runtime = engine.runtime(context.clone());
    assert!(
        runtime
            .load_program(artifact.clone(), Default::default())
            .is_err()
    );
    assert_eq!(runtime.runtime().modules().loaded_count(), 0);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let recorded = calls.clone();
    runtime
        .register_host_function(kagari_runtime::host::HostFunction::new(
            declaration,
            move |_, args| {
                recorded.lock().unwrap().push(args[0].clone());
                Ok(args[0].clone())
            },
        ))
        .unwrap();
    let loaded = runtime.load_program(artifact, Default::default()).unwrap();
    assert_eq!(loaded.epoch.0, 1);
    assert!(
        runtime
            .execute(&loaded, "main", &[], &ExecutionContext::default())
            .is_err()
    );
    assert!(calls.lock().unwrap().is_empty());
}

#[test]
fn reload_rejects_same_named_dependency_type_changes_before_publication() {
    let engine = KagariEngine::default();
    for name in ["left", "right"] {
        insert(&engine, name, "pub struct Item { val value: i32 }");
    }
    let mut artifacts = Vec::new();
    for (side, result) in [("left", 42), ("right", 99)] {
        let root = insert(
            &engine,
            "root",
            &format!(
                "use pkg::left; use pkg::right; use pkg::{side}::Item; pub fn expose(value: Item) -> Item {{ value }} fn main() -> i32 {{ {result} }}"
            ),
        );
        artifacts.push(compile(&engine, root, Default::default()));
    }
    for encoded in [false, true] {
        let prepare = |artifact: &BytecodeArtifact| {
            if encoded {
                BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
            } else {
                artifact.clone()
            }
        };
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let active = runtime
            .load_program(prepare(&artifacts[0]), Default::default())
            .unwrap();
        let error = runtime
            .reload_program(&active, prepare(&artifacts[1]), Default::default())
            .unwrap_err();
        assert_eq!(error.code(), "KG_RELOAD_PUBLIC_ABI_FINGERPRINT_MISMATCH");
        assert_eq!(
            runtime
                .execute(&active, "main", &[], &context)
                .unwrap()
                .return_value,
            Value::I32(42)
        );
        // Rejection leaves the baseline active, so a valid reload can still publish.
        runtime
            .reload_program(&active, prepare(&artifacts[0]), Default::default())
            .unwrap();
    }
}

#[test]
fn old_program_calls_keep_their_dependency_versions_after_reload() {
    use kagari_runtime::ModuleEpochRetention;
    let engine = KagariEngine::default();
    insert(&engine, "dependency", "pub fn value() -> i32 { 1 }");
    let root = insert(
        &engine,
        "root",
        "use pkg::dependency::value; fn main() -> i32 { value() }",
    );
    let first = compile(&engine, root, Default::default());
    insert(&engine, "dependency", "pub fn value() -> i32 { 2 }");
    let second = compile(&engine, root, Default::default());
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let old = runtime.load_program(first, Default::default()).unwrap();
    let dependency = old.members().next().unwrap();
    assert!(
        runtime
            .runtime()
            .modules()
            .retain_epoch(dependency.key(), ModuleEpochRetention::ActiveCall)
    );
    let new = runtime
        .reload_program(&old, second.clone(), Default::default())
        .unwrap();
    assert!(
        runtime
            .runtime()
            .modules()
            .collect_unreachable_epochs()
            .is_empty()
    );
    for (module, value) in [(&old, 1), (&new, 2), (&old, 1)] {
        assert_eq!(
            runtime
                .execute(module, "main", &[], &context)
                .unwrap()
                .return_value,
            Value::I32(value)
        );
    }
    assert!(
        runtime
            .reload_program(&old, second, Default::default())
            .is_err()
    );
    assert!(
        runtime
            .runtime()
            .modules()
            .release_epoch(dependency.key(), ModuleEpochRetention::ActiveCall)
    );
    assert_eq!(
        runtime
            .runtime()
            .modules()
            .collect_unreachable_epochs()
            .len(),
        2
    );
    assert_eq!(
        runtime
            .execute(&new, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(2)
    );
}

#[test]
fn malformed_programs_are_rejected_before_any_member_is_published() {
    use kagari_ir::bytecode::{
        BytecodeInstruction, BytecodeModule, CallTarget, FunctionRef, ModuleRef,
    };
    let engine = KagariEngine::default();
    insert(
        &engine,
        "dependency",
        "pub fn number(x: i32) -> i32 { x } pub fn flag(x: bool) -> i32 { if x { 1 } else { 0 } }",
    );
    let root = insert(
        &engine,
        "root",
        "use pkg::dependency::number; fn main() -> i32 { number(42) }",
    );
    let artifact = compile(&engine, root, Default::default());
    let program = &artifact.program;
    let mut malformed = Vec::new();
    let mut bad = program.clone();
    bad.root = ModuleRef::new(100);
    malformed.push(bad);
    let mut bad = program.clone();
    bad.modules[program.root.index()].dependencies.clear();
    malformed.push(bad);
    let mut bad = program.clone();
    bad.modules[program.root.index()]
        .dependencies
        .push(ModuleRef::new(0));
    malformed.push(bad);
    let mut bad = program.clone();
    bad.modules[0].identity = bad.modules[program.root.index()].identity.clone();
    malformed.push(bad);
    let mut bad = program.clone();
    bad.modules.push(BytecodeModule::default());
    malformed.push(bad);
    let flag = program.modules[0]
        .functions
        .iter()
        .find(|function| function.name == "flag")
        .unwrap()
        .id;
    for (module, function) in [
        (ModuleRef::new(100), FunctionRef::new(0)),
        (ModuleRef::new(0), FunctionRef::new(100)),
        (ModuleRef::new(0), flag),
    ] {
        let mut bad = program.clone();
        let call = bad.modules[program.root.index()]
            .functions
            .iter_mut()
            .flat_map(|function| &mut function.instructions)
            .find_map(|instruction| {
                if let BytecodeInstruction::Call {
                    callee: callee @ CallTarget::ModuleFunction { .. },
                    ..
                } = instruction
                {
                    Some(callee)
                } else {
                    None
                }
            })
            .unwrap();
        *call = CallTarget::ModuleFunction { module, function };
        malformed.push(bad);
    }
    let mut runtime = engine.runtime(ExecutionContext::default());
    for bad in malformed {
        assert!(BytecodeArtifact::from_program(bad.clone(), Default::default()).is_err());
        let mut corrupted = artifact.clone();
        corrupted.program = bad;
        let encoded = corrupted.to_bytes().unwrap();
        let decoded = BytecodeArtifact::from_bytes(&encoded).unwrap();
        assert!(runtime.load_program(decoded, Default::default()).is_err());
        assert_eq!(runtime.runtime().modules().loaded_count(), 0);
    }
    assert_eq!(
        runtime
            .load_program(artifact, Default::default())
            .unwrap()
            .epoch
            .0,
        1
    );
}
