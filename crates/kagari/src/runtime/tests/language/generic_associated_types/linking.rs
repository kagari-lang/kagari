use crate::{
    BytecodeArtifact, context::ExecutionContext, engine::KagariEngine, program::PreparedProgram,
};
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_runtime::{module::entry::ProgramEntry, value::Value};
use kagari_source::source_database::SourceLayer;

#[test]
fn imported_families_and_defaults_keep_declaration_owned_binders() {
    let engine = KagariEngine::default();
    let mut root = None;
    for (name, source) in [
        (
            "model",
            "pub struct Holder<T> { pub val value: T } pub trait Family<A> { type Item<T>; fn make<T>(self, first: A, value: T) -> Self::Item<T>; fn again<T>(self, first:A, value:T) -> Self::Item<T> { self.make(first, value) } }",
        ),
        (
            "root",
            "use pkg::model::{Family, Holder}; struct N {} impl Family<i32> for N { type Item<U> = Holder<(i32, U)>; fn make<V>(self, first:i32, value:V) -> Holder<(i32,V)> { Holder { value: (first,value) } } } fn main() -> i32 { val item: <N as Family<i32>>::Item<i32> = N {}.again(20, 22); item.value[0] + item.value[1] }",
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
    for encoded in [false, true] {
        let artifact = if encoded {
            BytecodeArtifact::from_bytes(
                &artifact.records().to_bytes(artifact.definitions()).unwrap(),
            )
            .unwrap()
        } else {
            artifact.clone()
        };
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime
            .load_program(&loaded_program, Default::default())
            .unwrap();
        assert_eq!(
            runtime
                .execute(&loaded, ProgramEntry::Main, &[], &context)
                .unwrap()
                .return_value
                .inspect(runtime.runtime(), |view| view.as_i32().map(Value::I32))
                .expect("retained execution result"),
            Value::I32(42)
        );
    }
}
