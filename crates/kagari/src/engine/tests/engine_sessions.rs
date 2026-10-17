#![cfg(feature = "source")]
use crate::{context::ExecutionContext, engine::KagariEngine, program::PreparedProgram};
use kagari_runtime::{
    module::entry::ProgramEntry,
    native::{
        binding::NativeResult, builder::ModuleBuilder, declarations::FunctionDecl,
        typed::NativeContext, types::Type,
    },
    value::Value,
};
use kagari_source::{source::SourceFile, source_database::SourceLayer};
use kagari_syntax::parser::ParseLimits;

fn provider(answer: i32) -> KagariEngine {
    let mut builder = KagariEngine::builder().unwrap();
    let mut module = ModuleBuilder::new("application::answer", builder.declarations()).unwrap();
    {
        let registered_function = module
            .define_function(FunctionDecl::new("read").returns(Type::i32()))
            .unwrap();
        module
            .bind(
                registered_function.clone(),
                move |_: &mut NativeContext<'_>, (): ()| -> NativeResult<i32> { Ok(answer) },
            )
            .map(|()| registered_function)
    }
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
fn runtimes_from_one_installation_have_independent_native_state_and_heap_owners() {
    let engine = KagariEngine::default();
    let mut first = engine.runtime(Default::default());
    let mut second = engine.runtime(Default::default());
    let mut builder = KagariEngine::builder().unwrap();
    let mut module = ModuleBuilder::new("application::local", builder.declarations()).unwrap();
    let answer = module
        .define_function(FunctionDecl::new("answer").returns(Type::i32()))
        .unwrap();
    module
        .bind(
            answer,
            |_: &mut NativeContext<'_>, (): ()| -> NativeResult<i32> { Ok(42) },
        )
        .unwrap();
    let module = module.finish().unwrap();
    module.install(first.runtime_mut()).unwrap();
    builder.install(module).unwrap();
    let artifact = builder
        .build()
        .unwrap()
        .compile_to_artifact(
            SourceFile::new(
                "local.kgr",
                "use application::local::answer; fn main() -> i32 { answer() }",
            ),
            Default::default(),
        )
        .unwrap();
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let owner = first.load_program(&program, Default::default()).unwrap();
    let context = ExecutionContext::default();
    assert_eq!(
        first
            .execute_typed::<_, i32>(&owner, ProgramEntry::Main, (), &context)
            .unwrap(),
        42
    );
    assert!(second.load_program(&program, Default::default()).is_err());
    let value = first.runtime().gc().alloc_string("owned".into()).unwrap();
    let rooted = first.runtime().root_value(value).unwrap();
    assert!(rooted.value(second.runtime().gc()).is_none());
    assert!(rooted.value(first.runtime().gc()).is_some());
}
