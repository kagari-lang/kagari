use kagari_common::SourceFile;
use kagari_embed::{ArtifactOptions, EmbeddingError, KagariEngine};

#[test]
fn contextual_phantom_layouts_execute_from_source_and_encoded_artifacts() {
    let engine = KagariEngine::default();
    let mut context = kagari_embed::ExecutionContext::default();
    context.language_profile.allow_jit = true;
    context.capabilities.jit = true;
    let artifact = engine.compile_to_artifact(SourceFile::new("context.kgr",
        "struct Marker<T> { val value: i32 } struct Outer<T> { val marker: Marker<T> } fn build() -> (Outer<i32>, [Outer<bool>]) { (if true { Outer { marker: Marker { value: 20 } } } else { Outer { marker: Marker { value: 0 } } }, [match 1 { 1 => Outer { marker: Marker { value: 22 } }, _ => Outer { marker: Marker { value: 0 } } }]) } fn main() -> i32 { val a: Outer<i32> = Outer { marker: Marker { value: 20 } }; val b: Outer<bool> = Outer { marker: Marker { value: 22 } }; val result = build(); result[0].marker.value + result[1][0].marker.value + a.marker.value + b.marker.value }"
    ), kagari_embed::CompileOptions { language_profile: context.language_profile }, Default::default()).unwrap();
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            kagari_embed::BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
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
        let result = if jit {
            let mut backend = kagari_jit_cranelift::CraneliftBackend::for_host().unwrap();
            runtime.execute_with_backend(&loaded, "main", &[], &context, &mut backend)
        } else {
            runtime.execute(&loaded, "main", &[], &context)
        }
        .unwrap();
        assert_eq!(result.return_value, kagari_runtime::value::Value::I32(84));
    }
}

#[test]
fn instance_limits_report_revision_owned_diagnostics_without_poisoning_compilation() {
    let engine = KagariEngine::default();
    let checked = engine
        .compile_source(
            SourceFile::new(
                "instances.kgr",
                "// 泛型😀\r\nfn echo<T>(value: T) -> T { value } fn main() -> i32 { echo(7) }",
            ),
            Default::default(),
        )
        .unwrap();
    let error = engine
        .emit_bytecode(
            &checked,
            ArtifactOptions {
                lowering: kagari_ir::IrLoweringOptions {
                    max_generic_instances: 0,
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .unwrap_err();
    let EmbeddingError::Diagnostics { diagnostics } = error else {
        panic!("structured diagnostic");
    };
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, "KG_COMPILE_LIMIT_EXCEEDED");
    assert!(
        engine
            .source_snapshot()
            .contains(diagnostics[0].span.unwrap())
    );
    let artifact = engine.emit_bytecode(&checked, Default::default()).unwrap();
    assert_eq!(
        artifact.program.modules[artifact.program.root.index()]
            .functions
            .len(),
        2
    );
}

#[test]
fn cancelled_instantiation_keeps_the_checked_module_reusable() {
    let engine = KagariEngine::default();
    let checked = engine
        .compile_source(
            SourceFile::new("cancel-instances.kgr", "fn main() -> i32 { 7 }"),
            Default::default(),
        )
        .unwrap();
    let options = ArtifactOptions::default();
    options.lowering.cancel.cancel();
    assert!(matches!(
        engine.emit_bytecode(&checked, options),
        Err(EmbeddingError::Cancelled)
    ));
    engine.emit_bytecode(&checked, Default::default()).unwrap();
}

#[test]
fn unresolved_container_inference_is_a_diagnostic_at_codegen() {
    let engine = KagariEngine::default();
    let checked = engine
        .compile_source(
            SourceFile::new("inference.kgr", "fn main() { std::map::new(); }"),
            Default::default(),
        )
        .unwrap();
    let Err(EmbeddingError::Diagnostics { diagnostics }) =
        engine.emit_bytecode(&checked, Default::default())
    else {
        panic!("unresolved type must not enter IR");
    };
    assert_eq!(diagnostics[0].code, "KG_COMPILE_UNRESOLVED_TYPE");
}
