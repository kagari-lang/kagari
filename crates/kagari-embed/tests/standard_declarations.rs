use kagari_abi::{callable::EngineNativeBinding, standard::StandardIntrinsic};
use kagari_common::{
    SourceFile, host_interface::standard_log, identity::DefinitionKind,
    source_database::SourceDatabase,
};
use kagari_embed::{BytecodeArtifact, ExecutionContext, KagariEngine, program::PreparedProgram};
use kagari_hir::{
    analysis::AnalysisDatabase, declarations::DeclarationId, native::NativeBinding,
    resolver::ResolvedName, typeck::FunctionImplementation,
};
use kagari_runtime::{host::HostFunction, value::Value};
use std::collections::HashSet;

#[test]
fn inherent_native_declarations_enforce_receiver_shapes_and_remove_old_exports() {
    let engine = KagariEngine::default();
    for body in [
        "std::array::len([1]);",
        "std::set::contains(LinkedHashSet::from([1]), 1);",
        "[1].join(\",\");",
        "val a: List<i32> = [1]; ArrayList::push(a, 2);",
    ] {
        assert!(
            engine
                .compile_to_artifact(
                    SourceFile::new(
                        "removed-standard-api.kgr",
                        format!("fn main() {{ {body} }}")
                    ),
                    Default::default(),
                    Default::default(),
                )
                .is_err(),
            "{body}"
        );
    }
    let signatures = AnalysisDatabase::default()
        .signatures(SourceDatabase::default().snapshot(), &Default::default())
        .unwrap();
    let declarations = signatures.declaration_snapshot();
    for intrinsic in [
        StandardIntrinsic::ArrayGet,
        StandardIntrinsic::ArrayPush,
        StandardIntrinsic::ArrayJoin,
        StandardIntrinsic::ResultMap,
    ] {
        let mut found = 0;
        for source in declarations.files() {
            let file = signatures.file(source.source().id()).unwrap();
            for function in file
                .signatures()
                .facts()
                .functions()
                .iter()
                .filter(|function| {
                    function.implementation
                        == FunctionImplementation::Native(NativeBinding::Engine(
                            EngineNativeBinding::Intrinsic(intrinsic),
                        ))
                })
            {
                assert_eq!(function.params[0].name, "self");
                let declaration = file
                    .declarations()
                    .target(ResolvedName::Function(function.id))
                    .unwrap();
                let DeclarationId::Definition(id) = &declaration.id else {
                    panic!("source method identity")
                };
                assert_eq!(id.path.len(), 2);
                let docs = declarations.documentation(&declaration.id).unwrap();
                assert!(docs.written_signature.contains("(self"));
                found += 1;
            }
        }
        assert!(found > 0, "missing checked binding {intrinsic:?}");
    }
}

#[test]
fn declared_for_each_uses_script_frames_and_cleans_iteration_guards_on_failure() {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "iteration-doc.kgr",
                r#"
fn main() {
    val values=[1,2];
    values.iter().for_each(|item| { values.push(item); });
}
fn healthy()->i32 {42}
"#,
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(
            &PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap(),
            Default::default(),
        )
        .unwrap();
    let error = runtime.execute(&loaded, "main", &[], &context).unwrap_err();
    assert_eq!(error.code(), "KG_RUNTIME_SCRIPT_TRAP");
    assert!(error.error_trace().unwrap().frames.len() >= 2);
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    assert!(runtime.runtime().execution_root().is_none());
    assert_eq!(
        runtime
            .execute(&loaded, "healthy", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
}

#[test]
fn standard_api_documentation_examples_compile_and_execute() {
    use kagari_common::host_interface::{HostFunctionDeclaration, HostInterface, HostValueType};
    let number = HostFunctionDeclaration::new("doc_test.number", vec![], HostValueType::F64);
    let mut failures = Vec::new();
    let mut checked = 0;
    let signatures = AnalysisDatabase::default()
        .signatures(SourceDatabase::default().snapshot(), &Default::default())
        .unwrap();
    let declarations = signatures.declaration_snapshot();
    let mut intrinsic_bindings = HashSet::new();
    for source in declarations.files() {
        let file = signatures.file(source.source().id()).unwrap();
        for function in file.signatures().facts().functions() {
            if let FunctionImplementation::Native(NativeBinding::Engine(
                EngineNativeBinding::Intrinsic(binding),
            )) = function.implementation
            {
                intrinsic_bindings.insert(binding);
            }
        }
    }
    let mut traits = 0;
    let mut identities = HashSet::new();
    for declaration in declarations
        .files()
        .flat_map(|file| file.declarations().iter())
    {
        let DeclarationId::Definition(id) = &declaration.id else {
            continue;
        };
        if !identities.insert(id.clone()) {
            continue;
        }
        let Some(item) = declarations.documentation(&declaration.id) else {
            panic!("missing documentation query for {id:?}");
        };
        let name = format!("{}::{:?}", id.module, id.path);
        let doc = &item.documentation;
        if id.path.len() == 1 {
            assert!(doc.contains("# Examples"), "{name}");
        }
        traits += usize::from(id.path.last().unwrap().kind == DefinitionKind::Trait);
        for block in doc.split("```kgr").skip(1) {
            let (flags, body) = block.split_once('\n').unwrap();
            let (body, _) = body.split_once("```").unwrap();
            let host_float = body.contains("fn example(value: f64)");
            let text = if host_float {
                format!("{body}\nfn main()->f64 {{ example(doc_test::number()) }}")
            } else if body.contains("fn main(") {
                body.to_owned()
            } else {
                format!("fn main() {{\n{body}\n}}")
            };
            // Each example is a complete isolated program. Retaining every old
            // example in one source database needlessly grows its signature graph.
            let engine = KagariEngine::default();
            engine
                .set_host_interface(HostInterface {
                    functions: vec![standard_log(), number.clone()],
                    types: vec![],
                    paths: vec![],
                })
                .unwrap();
            let artifact = match engine.compile_to_artifact(
                SourceFile::new(format!("doctest-{checked}.kgr"), text),
                kagari_embed::CompileOptions {
                    language_profile: kagari_runtime::LanguageProfile {
                        allow_host_calls: true,
                        ..Default::default()
                    },
                },
                Default::default(),
            ) {
                Ok(artifact) => artifact,
                Err(error) => {
                    failures.push(format!("{} compile: {error:?}", name));
                    continue;
                }
            };
            for encoded in [false, true] {
                let artifact = if encoded {
                    BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
                } else {
                    artifact.clone()
                };
                let mut context = ExecutionContext::default();
                context.language_profile.allow_host_calls = true;
                context.capabilities.host_calls = true;
                context.host_policy.allowed_host_functions =
                    vec!["host.log".into(), "doc_test.number".into()];
                let mut runtime = engine.runtime(context.clone());
                runtime
                    .register_host_function(HostFunction::new(standard_log(), |_, _| {
                        Ok(Value::Unit)
                    }))
                    .unwrap();
                runtime
                    .register_host_function(HostFunction::new(number.clone(), |_, _| {
                        Ok(Value::F64(0.0))
                    }))
                    .unwrap();
                let loaded = runtime
                    .load_program(
                        &PreparedProgram::from_artifact(
                            artifact,
                            &Default::default(),
                            &Default::default(),
                        )
                        .unwrap(),
                        Default::default(),
                    )
                    .unwrap();
                let result = runtime.execute(&loaded, "main", &[], &context);
                if flags.contains("should_panic") {
                    if !result
                        .as_ref()
                        .is_err_and(|e| e.code() == "KG_RUNTIME_SCRIPT_TRAP")
                    {
                        failures.push(format!("{} expected trap: {result:?}", name));
                    }
                } else if let Err(error) = result {
                    failures.push(format!("{} execute: {error:?}", name));
                }
                assert_eq!(runtime.runtime().gc().active_roots(), 0);
            }
            checked += 1;
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(!intrinsic_bindings.is_empty());
    assert!(traits > 0);
    assert!(checked >= intrinsic_bindings.len() + traits);
}
