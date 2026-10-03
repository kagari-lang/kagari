use {
    kagari_common::{
        cancellation::CancellationToken,
        identity::{ModuleIdentity, PackageId},
    },
    kagari_source::{identity::FileId, source_database::SourceLayer},
};

use kagari_embed::{
    BytecodeArtifact, context::ExecutionContext, engine::KagariEngine, error::EmbeddingError,
};
use kagari_runtime::value::Value;

fn compile(engine: &KagariEngine, root: FileId) -> BytecodeArtifact {
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
    BytecodeArtifact,
    ExecutionContext,
    kagari_common::host_interface::HostFunctionDeclaration,
) {
    use kagari_common::host_interface::{
        HostFunctionDeclaration, HostInterface, HostParameter, HostPassingStyle,
        value_type::HostValueType,
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
        ..Default::default()
    };
    let artifact = compile(&engine, root);
    (engine, artifact, context, declaration)
}

mod execution;
mod implementations;
mod imports;
