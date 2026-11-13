use crate::{BytecodeArtifact, context::ExecutionContext, engine::KagariEngine, program::Program};
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_runtime::module::entry::ProgramEntry;
use kagari_source::source_database::SourceLayer;
use kagari_stdlib as stdlib;

#[test]
fn imported_defaults_keep_the_trait_module_constant_resolution() {
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let mut root = None;
    for (name, source) in [
        (
            "model",
            "const BASE: i32 = 21; pub trait Limit { const VALUE: i32 = BASE; fn limit(self) -> i32 { Self::VALUE } }",
        ),
        (
            "root",
            "use pkg::model::Limit; const BASE: i32 = 0; struct Number {} impl Limit for Number {} fn main() -> i32 { Number::VALUE + Number {}.limit() }",
        ),
    ] {
        let path = format!("mem://{name}");
        engine
            .bind_module(
                &path,
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
        let id = engine
            .set_source(&path, source.into(), SourceLayer::Base)
            .unwrap();
        if name == "root" {
            root = Some(id);
        }
    }
    let checked = engine
        .compile_snapshot(engine.source_snapshot(), root.unwrap(), &Default::default())
        .unwrap();
    let artifact = engine.emit_artifact(&checked, Default::default()).unwrap();
    let artifact =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded_program =
        Program::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .as_i32()
            .unwrap(),
        42
    );
}
