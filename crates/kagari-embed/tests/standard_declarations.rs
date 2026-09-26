use kagari_common::{SourceFile, host_interface::standard_log};
use kagari_embed::{BytecodeArtifact, ExecutionContext, KagariEngine};
use kagari_hir::builtin::surface;
use kagari_runtime::{host::HostFunction, value::Value};

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
    std::iter::for_each(values, |item| { values.push(item); });
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
    let loaded = runtime.load_program(artifact, Default::default()).unwrap();
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
    let engine = KagariEngine::default();
    use kagari_common::host_interface::{HostFunctionDeclaration, HostInterface, HostValueType};
    let number = HostFunctionDeclaration::new("doc_test.number", vec![], HostValueType::F64);
    engine
        .set_host_interface(HostInterface {
            functions: vec![standard_log(), number.clone()],
            types: vec![],
            paths: vec![],
        })
        .unwrap();
    let mut failures = Vec::new();
    let mut checked = 0;
    for item in surface::STANDARD_ITEMS {
        let name = format!("{}::{:?}", item.module, item.path);
        let doc = item.documentation;
        if item.path.len() == 1 {
            assert!(doc.contains("# Examples"), "{name}");
        }
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
                let loaded = runtime.load_program(artifact, Default::default()).unwrap();
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
    assert!(checked >= surface::standard_functions().len() + surface::STANDARD_TRAITS.len());
}
