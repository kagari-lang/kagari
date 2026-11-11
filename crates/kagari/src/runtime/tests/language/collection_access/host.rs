use crate::{BytecodeArtifact, context::ExecutionContext, engine::KagariEngine, program::Program};
use kagari_common::identity::map::DefinitionContext;
use kagari_runtime::{
    host::HostCallContext,
    module::entry::ProgramEntry,
    native::{builder::ModuleBuilder, declarations::FunctionDecl, types::Type},
    value::Value,
};
use kagari_source::source::SourceFile;
use kagari_stdlib as stdlib;
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_types::{
    declaration::host::{HostCallPolicy, HostFunctionEffects},
    scalar::BuiltinType,
    ty::Ty,
};

#[test]
fn host_results_preserve_builtin_array_shape_through_artifacts_and_binding_checks() {
    let declarations = StandardDeclarations::default();
    let module = |result: Type| {
        let mut builder = ModuleBuilder::new("demo", &declarations.catalog().unwrap()).unwrap();
        let values = builder
            .define_function(FunctionDecl::new("values").returns(result).host_policy(
                HostCallPolicy {
                    passing: vec![],
                    effects: HostFunctionEffects {
                        may_allocate: true,
                        ..Default::default()
                    },
                },
            ))
            .unwrap();
        builder
            .bind(values, |context: &mut HostCallContext<'_>, _: &[Value]| {
                Ok(Value::Array(
                    context
                        .runtime()
                        .alloc_array(
                            &context.runtime().execution_root().unwrap(),
                            Ty::Builtin(BuiltinType::I32),
                            vec![Value::I32(40)],
                        )
                        .unwrap(),
                ))
            })
            .unwrap();
        builder.finish().unwrap()
    };
    let array = Type::from_semantic(
        Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))),
        &DefinitionContext::new().unwrap().snapshot(),
    )
    .unwrap();
    let engine = KagariEngine::with_native_modules(
        Default::default(),
        stdlib::modules()
            .unwrap()
            .into_iter()
            .chain(vec![module(array)])
            .collect(),
    )
    .unwrap();

    assert!(
        engine
            .compile_to_artifact(
                SourceFile::new(
                    "host-negative.kgr",
                    "use demo; fn main() { demo::values().push(1); }"
                ),
                Default::default()
            )
            .is_err()
    );
    let artifact = engine.compile_to_artifact(SourceFile::new("host-readonly.kgr", "use demo; fn main() -> i32 { val source = demo::values(); val copy = [source[0], 2]; copy[0] + copy[1] }"),  Default::default()).unwrap();
    let artifact =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let context = ExecutionContext {
        ..Default::default()
    };

    let mut runtime = engine.runtime(context.clone());
    let loaded_program =
        Program::from_artifact(artifact.clone(), &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .value(runtime.gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    let changed = module(Type::tuple([Type::i32()]).unwrap());
    let changed = KagariEngine::with_native_modules(
        Default::default(),
        stdlib::modules()
            .unwrap()
            .into_iter()
            .chain(vec![changed])
            .collect(),
    )
    .unwrap();
    let mut mismatched = changed.runtime(context);
    assert!(
        mismatched
            .load_program(
                &Program::from_artifact(artifact, &Default::default(), &Default::default())
                    .unwrap(),
                Default::default()
            )
            .is_err()
    );
}
