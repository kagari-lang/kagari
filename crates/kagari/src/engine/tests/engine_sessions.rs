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
use kagari_source::{
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_syntax::parser::ParseLimits;
use std::{env, process::Command};

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
fn explicit_source_root_preserves_names_capture_and_fresh_sessions() {
    let mut builder = KagariEngine::builder().unwrap();
    builder
        .source_root("/host-owned/../virtual-project")
        .unwrap();
    for invalid in ["relative", "memory://root", "file:relative"] {
        assert!(builder.source_root(invalid).is_err());
    }
    let engine = builder.build().unwrap();
    let id = engine
        .set_source("src/../main.kgr", "fn main() {}".into(), SourceLayer::Base)
        .unwrap();
    let captured = engine.source_snapshot();
    assert_eq!(
        captured.file(id).unwrap().name(),
        "/virtual-project/main.kgr"
    );
    assert_eq!(
        engine
            .set_source(
                "/virtual-project/main.kgr",
                "fn main() { missing; }".into(),
                SourceLayer::Overlay
            )
            .unwrap(),
        id
    );
    assert_eq!(captured.file(id).unwrap().text(), "fn main() {}");
    engine
        .compile_snapshot(captured, id, &Default::default())
        .unwrap();
    assert!(
        engine
            .compile_snapshot(engine.source_snapshot(), id, &Default::default())
            .is_err()
    );

    let session = engine.new_session(Default::default());
    assert_eq!(session.source_snapshot().files().count(), 0);
    let virtual_name = "MEMORY://Host/../Main%2F?Version=One#Part";
    session
        .compile_to_artifact(
            SourceFile::new(virtual_name, "fn main() -> i32 { 42 }"),
            Default::default(),
        )
        .unwrap();
    let snapshot = session.source_snapshot();
    assert_eq!(
        snapshot.files().next().unwrap().name(),
        "memory://Host/../Main%2F?Version=One#Part"
    );
    let relative = session
        .set_source("other.kgr", "fn other() {}".into(), SourceLayer::Base)
        .unwrap();
    assert_eq!(
        session.source_snapshot().file(relative).unwrap().name(),
        "/virtual-project/other.kgr"
    );
    assert_eq!(engine.source_snapshot().files().count(), 1);
}

#[test]
fn native_source_root_is_captured_before_working_directory_changes() {
    const CHILD: &str = "KAGARI_SOURCE_ROOT_TEST_CHILD";
    if env::var_os(CHILD).is_none() {
        let status = Command::new(env::current_exe().unwrap())
            .args(["--exact", "engine::tests::engine_sessions::native_source_root_is_captured_before_working_directory_changes"])
            .env(CHILD, "1")
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    // Only the isolated child changes cwd; parallel tests retain their environment.
    let sources = SourceDatabase::default();
    let engine = KagariEngine::default();
    let previous = env::current_dir().unwrap();
    env::set_current_dir(previous.parent().unwrap()).unwrap();
    let session = engine.new_session(Default::default());
    for owner in [&engine, &session] {
        let id = owner
            .set_source("captured.kgr", "fn main() {}".into(), SourceLayer::Base)
            .unwrap();
        assert_eq!(
            owner.source_snapshot().file(id).unwrap().name(),
            sources.source_name("captured.kgr").unwrap()
        );
        owner
            .compile_snapshot(owner.source_snapshot(), id, &Default::default())
            .unwrap();
    }
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
