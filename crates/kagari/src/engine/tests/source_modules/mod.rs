use crate::{
    BytecodeArtifact, context::ExecutionContext, engine::KagariEngine, error::EmbeddingError,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata},
};
use kagari_runtime::{
    host::HostCallContext,
    native::{builder::ModuleBuilder, declarations::FunctionDecl, types::Type},
    value::Value,
};
use kagari_source::{identity::FileId, source_database::SourceLayer};
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_types::declaration::host::{HostCallPolicy, HostPassingStyle};
use std::sync::{Arc, Mutex};

fn compile(engine: &KagariEngine, root: FileId) -> DefinitionMetadata<BytecodeArtifact> {
    let checked = engine
        .compile_snapshot(engine.source_snapshot(), root, &Default::default())
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

fn host_fixture() -> (
    KagariEngine,
    DefinitionMetadata<BytecodeArtifact>,
    ExecutionContext,
    Arc<Mutex<Vec<Value>>>,
) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let recorded = calls.clone();
    let mut builder =
        ModuleBuilder::new("trace", &StandardDeclarations::default().catalog().unwrap()).unwrap();
    let record = builder
        .define_function(
            FunctionDecl::new("record")
                .parameter("value", Type::i32())
                .returns(Type::i32())
                .host_policy(HostCallPolicy {
                    passing: vec![HostPassingStyle::Owned],
                    effects: Default::default(),
                }),
        )
        .unwrap();
    builder
        .bind(record, move |_: &HostCallContext<'_>, args: &[Value]| {
            recorded.lock().unwrap().push(args[0]);
            Ok(args[0])
        })
        .unwrap();
    let engine =
        KagariEngine::with_native_modules(Default::default(), vec![builder.finish().unwrap()])
            .unwrap();
    insert(
        &engine,
        "shared",
        "use trace; pub fn value() -> i32 { trace::record(1); 21 }",
    );
    insert(
        &engine,
        "left",
        "use trace; use pkg::shared::value; pub fn left() -> i32 { trace::record(2); value() }",
    );
    insert(
        &engine,
        "right",
        "use trace; use pkg::shared; pub fn right() -> i32 { trace::record(3); 21 }",
    );
    let root = insert(
        &engine,
        "root",
        "use trace; use pkg::left::left; use pkg::right::right; fn main() -> i32 { trace::record(4); left() + right() }",
    );
    let context = ExecutionContext {
        ..Default::default()
    };
    let artifact = compile(&engine, root);
    (engine, artifact, context, calls)
}

mod execution;
mod implementations;
mod imports;
