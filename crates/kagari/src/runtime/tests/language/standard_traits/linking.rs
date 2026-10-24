//! Imported operations keep their defining module across portable artifact execution.
use crate::{
    BytecodeArtifact, context::ExecutionContext, engine::KagariEngine, program::PreparedProgram,
};
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_runtime::{module::entry::ProgramEntry, value::Value};
use kagari_source::source_database::SourceLayer;

#[test]
fn imported_equality_and_hash_use_the_defining_modules_implementations() {
    for downstream_override in [false, true] {
        let engine = KagariEngine::default();
        let model = r#"use std::collections::{HashMap};
use std::hash::{Hash};

pub struct Key {pub val id:i32}
impl PartialEq for Key {fn eq(self,other:Self)->bool {self.id==other.id}}
impl Eq for Key {}
impl Hash for Key {fn hash(self)->i64 {self.id.hash()}}
pub fn equal(a:Key,b:Key)->bool {a==b}
pub fn make()->HashMap<Key,i32> {val m:HashMap<Key,i32> = HashMap::new();m.insert(Key{id:1},42);m}
"#;
        let root_source = if downstream_override {
            "use pkg::model::Key; impl PartialEq for Key {fn eq(self,other:Self)->bool {false}} fn main()->i32 {42}"
        } else {
            "use pkg::model::{Key,equal,make}; fn same<T:Eq>(a:T,b:T)->bool {a==b} fn main()->i32 {val a=Key{id:1};val b=Key{id:1};if equal(a,b) && same(a,b) && a !== b {(match make().get(b) { Some(value) => value, None => 0 })} else {0}}"
        };
        let mut root = None;
        for (name, source) in [("model", model), ("root", root_source)] {
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
        let checked =
            engine.compile_snapshot(engine.source_snapshot(), root.unwrap(), &Default::default());
        if downstream_override {
            assert!(checked.is_err());
            continue;
        }
        let artifact = engine
            .emit_artifact(&checked.unwrap(), Default::default())
            .unwrap();
        let artifact = BytecodeArtifact::from_bytes(
            &artifact.records().to_bytes(artifact.definitions()).unwrap(),
        )
        .unwrap();
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
                .value(runtime.runtime().gc())
                .expect("retained execution result"),
            Value::I32(42)
        );
    }
}
