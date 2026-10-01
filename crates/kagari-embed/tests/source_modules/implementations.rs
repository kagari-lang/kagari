use super::*;
use kagari_bytecode::{
    artifact::{ArtifactValidationError, KbcArtifact},
    program::verify_program,
    verifier::BytecodeVerificationError,
};
use kagari_embed::{context::JitPolicy, program::PreparedProgram};

#[test]
fn source_and_encoded_programs_execute_transitive_calls_and_shared_struct_layouts() {
    let engine = KagariEngine::default();
    insert(
        &engine,
        "shared",
        "pub struct Data { pub var x: i32 } fn id<T>(x: T) -> T { x } pub fn make(x: i32) -> Data { Data { x: id(x) } } pub fn add(p: Data, x: i32) -> i32 { p.x += x; p.x }",
    );
    insert(&engine, "left", "pub use pkg::shared::make;");
    insert(&engine, "right", "pub use pkg::shared::add;");
    let root = insert(
        &engine,
        "root",
        "use pkg::left::make; use pkg::right::add; fn id() -> bool { true } fn main() -> i32 { val p = make(20); val n = add(p, 22); p.x }",
    );
    let artifact = compile(&engine, root, Default::default());
    let mut user_modules: Vec<_> = artifact
        .program
        .modules
        .iter()
        .filter(|module| module.identity.package == PackageId("pkg".into()))
        .map(|module| module.identity.path.join("::"))
        .collect();
    user_modules.sort();
    assert_eq!(user_modules, ["left", "right", "root", "shared"]);
    assert!(
        artifact
            .program
            .modules
            .iter()
            .any(|module| module.identity.package == PackageId("kagari-std".into()))
    );
    assert_eq!(
        artifact.verification.dependency_fingerprints.len(),
        artifact.program.modules.len() - 1
    );
    let encoded = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    for artifact in [artifact, encoded] {
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
fn imported_applied_trait_impl_runs_through_source_artifact_and_jit() {
    let engine = KagariEngine::default();
    insert(
        &engine,
        "api",
        include_str!("../../../../examples/imported-traits/api.kgr"),
    );
    let root = insert(
        &engine,
        "root",
        include_str!("../../../../examples/imported-traits/main.kgr"),
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
        kagari_abi::types::PublicAbiItem::Trait(interface) => interface.methods.first_mut(),
        _ => None,
    });
    method.unwrap().return_type =
        kagari_abi::types::AbiType::Builtin(kagari_abi::scalar::BuiltinType::Bool);
    assert!(matches!(
        verify_program(&wrong_contract),
        Err(BytecodeVerificationError::InvalidInterfaceTable)
    ));
    assert!(matches!(
        KbcArtifact::from_program(wrong_contract, Default::default()),
        Err(ArtifactValidationError::Bytecode(
            BytecodeVerificationError::InvalidInterfaceTable
        ))
    ));
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        context.jit_policy = if jit {
            JitPolicy::Enabled
        } else {
            JitPolicy::Disabled
        };
        let mut runtime = engine.runtime(context.clone());
        let loaded_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime
            .load_program(&loaded_program, Default::default())
            .unwrap();
        let report = if jit {
            let mut backend = kagari_codegen_cranelift::CraneliftBackend::for_host().unwrap();
            let prepared = runtime
                .prepare_native(
                    &loaded_program,
                    &loaded,
                    "main",
                    &mut backend,
                    &context.cancellation,
                )
                .unwrap();
            runtime.execute_prepared(&loaded, "main", &[], &context, &prepared)
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
            JitPolicy::Enabled
        } else {
            JitPolicy::Disabled
        };
        let mut runtime = engine.runtime(context.clone());
        let loaded_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime
            .load_program(&loaded_program, Default::default())
            .unwrap();
        let report = if jit {
            let mut backend = kagari_codegen_cranelift::CraneliftBackend::for_host().unwrap();
            let prepared = runtime
                .prepare_native(
                    &loaded_program,
                    &loaded,
                    "main",
                    &mut backend,
                    &context.cancellation,
                )
                .unwrap();
            runtime.execute_prepared(&loaded, "main", &[], &context, &prepared)
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
        include_str!("../../../../examples/imported-traits/api.kgr"),
    );
    insert(
        &engine,
        "model",
        include_str!("../../../../examples/imported-traits/model.kgr"),
    );
    let root = insert(
        &engine,
        "root",
        include_str!("../../../../examples/imported-traits/consumer.kgr"),
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
            JitPolicy::Enabled
        } else {
            JitPolicy::Disabled
        };
        let mut runtime = engine.runtime(context.clone());
        let loaded_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime
            .load_program(&loaded_program, Default::default())
            .unwrap();
        let report = if jit {
            let mut backend = kagari_codegen_cranelift::CraneliftBackend::for_host().unwrap();
            let prepared = runtime
                .prepare_native(
                    &loaded_program,
                    &loaded,
                    "main",
                    &mut backend,
                    &context.cancellation,
                )
                .unwrap();
            runtime.execute_prepared(&loaded, "main", &[], &context, &prepared)
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
        include_str!("../../../../examples/imported-traits/api.kgr"),
    );
    insert(
        &engine,
        "model",
        include_str!("../../../../examples/imported-traits/model.kgr"),
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
        include_str!("../../../../examples/imported-traits/api.kgr"),
    );
    insert(
        &engine,
        "model",
        "pub struct Holder { pub val number: i32 } pub fn make() -> Holder { Holder { number: 9 } }",
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
        include_str!("../../../../examples/imported-traits/api.kgr"),
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
        include_str!("../../../../examples/imported-traits/api.kgr"),
    );
    insert(
        &engine,
        "model",
        include_str!("../../../../examples/imported-traits/generic-model.kgr"),
    );
    let root = insert(
        &engine,
        "root",
        include_str!("../../../../examples/imported-traits/generic-consumer.kgr"),
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
            JitPolicy::Enabled
        } else {
            JitPolicy::Disabled
        };
        let mut runtime = engine.runtime(context.clone());
        let loaded_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime
            .load_program(&loaded_program, Default::default())
            .unwrap();
        let report = if jit {
            let mut backend = kagari_codegen_cranelift::CraneliftBackend::for_host().unwrap();
            let prepared = runtime
                .prepare_native(
                    &loaded_program,
                    &loaded,
                    "main",
                    &mut backend,
                    &context.cancellation,
                )
                .unwrap();
            runtime.execute_prepared(&loaded, "main", &[], &context, &prepared)
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
        include_str!("../../../../examples/imported-traits/api.kgr"),
    );
    insert(
        &engine,
        "model",
        "use pkg::api::Echo; pub struct Holder<T> { val value: T } impl<T: Eq + Hash> Echo<i32> for Holder<T> { fn get(self) -> i32 { 9 } } pub fn good() -> Holder<i32> { Holder { value: 1 } } pub fn bad() -> Holder<f32> { Holder { value: 1.5 } }",
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
        include_str!("../../../../examples/imported-traits/api.kgr"),
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
    let loaded_program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
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
        "pub struct Marker<T> { pub val value: i32 } pub enum Token<T> { Empty } pub fn take(value: Marker<i32>, token: Token<bool>) -> i32 { value.value }",
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
            JitPolicy::Enabled
        } else {
            JitPolicy::Disabled
        };
        let mut runtime = engine.runtime(context.clone());
        let loaded_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime
            .load_program(&loaded_program, Default::default())
            .unwrap();
        let report = if jit {
            let mut backend = kagari_codegen_cranelift::CraneliftBackend::for_host().unwrap();
            let prepared = runtime
                .prepare_native(
                    &loaded_program,
                    &loaded,
                    "main",
                    &mut backend,
                    &context.cancellation,
                )
                .unwrap();
            runtime.execute_prepared(&loaded, "main", &[], &context, &prepared)
        } else {
            runtime.execute(&loaded, "main", &[], &context)
        }
        .unwrap();
        assert_eq!(report.return_value, Value::I32(42));
    }
}
