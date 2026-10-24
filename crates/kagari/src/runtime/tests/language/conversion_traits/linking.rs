use crate::{
    BytecodeArtifact, context::ExecutionContext, engine::KagariEngine, program::PreparedProgram,
};
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_runtime::{module::entry::ProgramEntry, value::Value};
use kagari_source::source_database::SourceLayer;

#[test]
fn imported_generic_conversions_and_iterators_link_to_their_defining_module() {
    let engine = KagariEngine::default();
    let mut root = None;
    for (name, source) in [
        (
            "model",
            r#"
pub struct Wrapper<T>{pub val value:T,var consumed:bool}
impl<T> From<T> for Wrapper<T>{fn from(value:T)->Self{Wrapper{value,consumed:false}}}
impl<T> Iterator for Wrapper<T>{type Item=T;fn next(self)->Option<T>{if self.consumed {None}else{self.consumed=true;Some(self.value)}}}
"#,
        ),
        (
            "root",
            r#"
use pkg::model::Wrapper;
fn total<I:Iterable<Item=i32>>(values:I)->i32{var n=0;for x in values{n+=x;}n}
fn propagate()->Result<i32,Wrapper<i32>> {val x:Result<i32,i32> = Err(42);Ok(x?)}
fn main()->i32 {match propagate(){Err(w)=>total(w),Ok(x)=>x}}
"#,
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
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
}
