#![cfg(feature = "source")]
use kagari::{context::ExecutionContext, engine::KagariEngine, program::PreparedProgram};
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_runtime::{
    host::HostFunction,
    module::entry::ProgramEntry,
    native::{
        binding::NativeResult, builder::ModuleBuilder, registration::FunctionSpec,
        typed::NativeContext,
    },
    value::Value,
};
use kagari_source::{source::SourceFile, source_database::SourceLayer};
use kagari_syntax::parser::ParseLimits;
use kagari_types::host_interface::{HostFunctionDeclaration, value_type::HostValueType};

fn provider(answer: i32) -> KagariEngine {
    let mut builder = KagariEngine::builder().unwrap();
    let mut module = ModuleBuilder::new("application::answer", builder.declarations()).unwrap();
    module
        .add_function(
            FunctionSpec::new("read"),
            move |_: &mut NativeContext<'_>, (): ()| -> NativeResult<i32> { Ok(answer) },
        )
        .unwrap();
    builder.install(module.finish().unwrap()).unwrap();
    builder.build().unwrap()
}

#[test]
fn sessions_retain_exact_providers_without_inheriting_source_or_query_settings() {
    // Equal module identities do not authorize sharing bindings across engines.
    let first = provider(42);
    let second = provider(99);
    let stale = first
        .set_source(
            "memory://stale.kgr",
            "fn stale() {}".into(),
            SourceLayer::Base,
        )
        .unwrap();
    first.set_parse_limits(ParseLimits {
        max_nesting: 0,
        ..Default::default()
    });
    for (engine, answer) in [(&first, 42), (&second, 99)] {
        let session = engine.new_session(Default::default());
        assert_eq!(session.source_snapshot().files().count(), 0);
        let artifact = session
            .compile_to_artifact(
                SourceFile::new(
                    "memory://same.kgr",
                    "use application::answer::read; fn main() -> i32 { read() }",
                ),
                Default::default(),
            )
            .unwrap();
        let snapshot = session
            .analyze(session.source_snapshot(), &Default::default())
            .unwrap();
        assert!(snapshot.file(stale).is_none());
        let program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let context = ExecutionContext::default();
        let mut runtime = session.runtime(context.clone());
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        let result = runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap();
        assert_eq!(
            result.return_value.value(runtime.runtime().gc()).unwrap(),
            Value::I32(answer)
        );
    }
    // Forking did not mutate the parent's parser configuration or source database.
    assert_eq!(first.source_snapshot().files().count(), 1);
    assert!(
        first
            .compile_to_artifact(
                SourceFile::new("limited.kgr", "fn main() {}"),
                Default::default()
            )
            .is_err()
    );
}

#[test]
fn runtimes_from_one_installation_have_independent_host_state_and_heap_owners() {
    let engine = KagariEngine::default();
    let mut first = engine.runtime(Default::default());
    let second = engine.runtime(Default::default());
    first
        .runtime_mut()
        .register_host_function(HostFunction::new(
            {
                let context = DefinitionContext::new().unwrap();
                let declaration = HostFunctionDeclaration::new(
                    &context,
                    "host.answer",
                    vec![],
                    HostValueType::I32,
                )
                .unwrap();
                DefinitionMetadata::checked(context.snapshot(), declaration, &Default::default())
                    .unwrap()
            },
            |_, _| Ok(Value::I32(42)),
        ))
        .unwrap();
    assert_eq!(
        first.runtime().invoke_host("host.answer", &[]).unwrap(),
        Value::I32(42)
    );
    assert!(second.runtime().invoke_host("host.answer", &[]).is_err());
    let value = first.runtime().gc().alloc_string("owned".into()).unwrap();
    let rooted = first.runtime().root_value(value).unwrap();
    assert!(rooted.value(second.runtime().gc()).is_none());
    assert!(rooted.value(first.runtime().gc()).is_some());
}
