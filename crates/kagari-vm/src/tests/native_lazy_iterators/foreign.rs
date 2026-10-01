//! Generic native constructors carry private next/iter across defining modules.
use super::runtime;
use crate::vm::Vm;
use kagari_bytecode::{artifact::KbcArtifact, program::verify_program};
use kagari_common::{
    host_interface::standard_log,
    identity::{ModuleIdentity, PackageId},
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{host::HostFunction, value::Value};
#[test]
fn foreign_lazy_constructors_pin_private_generic_traversal() {
    let mut sources = SourceDatabase::default();
    let mut root = None;
    for (name, text) in [
        (
            "model",
            r#"
pub trait Pipeline{fn pipeline(self)->Iter<i32>;}
pub struct Build<I:Iterator<Item=i32>,R:Iterable<Item=i32>>{pub val left:I,pub val right:R}
impl<I:Iterator<Item=i32>,R:Iterable<Item=i32>> Pipeline for Build<I,R>{
 fn pipeline(self)->Iter<i32>{self.left.zip(self.right).map(|pair|pair[0]).flat_map(|item|[item,item+1]).inspect(|item|{print("visit");}).skip_while(|item|item<20).take_while(|item|item<30).fuse()}
}
"#,
        ),
        (
            "root",
            r#"
use pkg::model::{Pipeline,Build};
struct Cursor<T>{val items:ArrayList<T>,var index:usize}
impl<T> Iterator for Cursor<T>{type Item=T;fn next(self)->Option<T>{print("next");val result=self.items.get(self.index);self.index+=1usize;result}}
struct Private<T>{val items:ArrayList<T>}
impl<T> Iterable for Private<T>{type Item=T;type Iter=Cursor<T>;fn iter(self)->Cursor<T>{print("iter");Cursor{items:self.items,index:0usize}}}
fn main()->i32{val maker:Build<Cursor<i32>,Private<i32>> =Build{left:Cursor{items:[20,21,22],index:0usize},right:Private{items:[1,2,3]}};val iterator=maker.pipeline();val alias=iterator;std::debug::assert(iterator.next()==Some(20),"first");val rest:ArrayList<i32> =alias.collect();std::debug::assert(rest.len()==5usize && rest[0]==21 && rest[4]==23,"rest");42}
"#,
        ),
    ] {
        let uri = format!("mem://{name}");
        sources
            .bind_module(
                &uri,
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
        root = Some(sources.set(&uri, text.into(), SourceLayer::Base).unwrap());
    }
    let snapshot = AnalysisDatabase::default()
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap();
    let checked = snapshot
        .check_program(root.unwrap(), &Default::default())
        .unwrap();
    let program =
        lower_program_to_bytecode(&lower_program_to_mir(&checked, &Default::default()).unwrap())
            .unwrap();
    for encoded in [false, true] {
        let program = if encoded {
            KbcArtifact::from_bytes(
                &KbcArtifact::from_program(program.clone(), Default::default())
                    .unwrap()
                    .to_bytes()
                    .unwrap(),
            )
            .unwrap()
            .program
        } else {
            program.clone()
        };
        let mut forged = program.clone();
        let model = forged
            .modules
            .iter_mut()
            .find(|module| module.identity.path == ["model"])
            .unwrap();
        assert!(model.dependencies.contains(&forged.root));
        model
            .dependencies
            .retain(|dependency| *dependency != forged.root);
        assert!(verify_program(&forged).is_err());
        let mut rt = runtime();
        rt.register_host_function(HostFunction::new(standard_log(), |context, _| {
            context.runtime().collect_garbage().unwrap();
            Ok(Value::Unit)
        }))
        .unwrap();
        assert!(rt.load_program("unlinked-private-next", forged).is_err());
        let loaded = rt.load_program("foreign-lazy", program).unwrap();
        let mut vm = Vm::new(rt);
        assert_eq!(
            vm.execute(&loaded, "main").unwrap().return_value,
            Value::I32(42)
        );
        assert_eq!(vm.runtime().gc().active_roots(), 0);
        assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
        vm.runtime().collect_garbage().unwrap();
        assert_eq!(vm.runtime().gc().allocated_objects(), 0);
    }
}
