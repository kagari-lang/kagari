use crate::{
    BytecodeArtifact, context::ExecutionContext, engine::KagariEngine, program::PreparedProgram,
};
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_runtime::{module::entry::ProgramEntry, value::Value};
use kagari_source::source_database::SourceLayer;

#[test]
fn imported_generic_operators_keep_the_defining_implementation() {
    for downstream_override in [false, true] {
        let engine = KagariEngine::default();
        let model = r#"use std::ops::{Add, Index};

pub struct Box<T> {pub val value:T}
impl<T:Add<T,Output=T>> Add<T> for Box<T> {type Output=Box<T>;fn add(self,rhs:T)->Box<T> {Box{value:self.value+rhs}}}
impl<T> Index<i32> for Box<T> {type Output=T;fn index(self,rhs:i32)->T {self.value}}
fn plus<T:Add<T,Output=T>>(a:Box<T>,b:T)->Box<T> {a+b}
pub fn add(a:Box<i32>,b:i32)->Box<i32> {plus(a,b)}
"#;
        let root_source = if downstream_override {
            "use std::ops::{Neg};\nuse pkg::model::Box; impl Neg for Box<i32> {type Output=i32;fn neg(self)->i32 {0}} fn main()->i32 {42}"
        } else {
            "use std::ops::{Index};\nuse pkg::model::{Box,add}; fn read<T:Index<i32,Output=i32>>(v:T)->i32 {v[0]} fn main()->i32 {read(add(Box{value:20},1)+21)}"
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
