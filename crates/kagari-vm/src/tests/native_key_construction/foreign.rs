use super::{
    boundaries::{clean, route},
    runtime,
};
use crate::vm::Vm;
use kagari_bytecode::program::verify_program;
use kagari_common::{
    host_interface::standard_log,
    identity::{ModuleIdentity, PackageId},
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{host::HostFunction, value::Value};
#[test]
fn foreign_construction_pins_private_traversal_keys_and_payloads() {
    let mut sources = SourceDatabase::default();
    let mut root = None;
    for (name, text) in [
        (
            "model",
            r#"
pub trait Build{type Output;fn build(self)->Self::Output;}
pub struct Gather<K:Eq+Hash,V,I:Iterable<Item=(K,V)>>{pub val source:I,pub val key:K,pub val value:V}
impl<K:Eq+Hash,V,I:Iterable<Item=(K,V)>> Build for Gather<K,V,I>{type Output=LinkedHashMap<K,V>;fn build(self)->LinkedHashMap<K,V>{LinkedHashMap::from_iter(self.source)}}
pub struct Make<K:Eq+Hash>{pub val source:List<K>}
impl<K:Eq+Hash> Build for Make<K>{type Output=LinkedHashSet<K>;fn build(self)->LinkedHashSet<K>{LinkedHashSet::from(self.source)}}
"#,
        ),
        (
            "root",
            r#"
use pkg::model::{Build,Gather,Make};
struct Cell{var value:i32}
struct Private<T>{val id:T,var visits:i32}
impl<T:PartialEq> PartialEq for Private<T>{fn eq(self,other:Self)->bool{print("eq");self.visits+=1;self.id==other.id}}
impl<T:Eq> Eq for Private<T>{}
impl<T:Eq+Hash> Hash for Private<T>{fn hash(self)->i64{print("hash");self.visits+=1;0i64}}
struct Cursor<T>{val items:ArrayList<T>,var index:usize}
impl<T> Iterator for Cursor<T>{type Item=T;fn next(self)->Option<T>{print("next");val value=self.items.get(self.index);self.index+=1usize;value}}
struct Source<T>{val items:ArrayList<T>}
impl<T> Iterable for Source<T>{type Item=T;type Iter=Cursor<T>;fn iter(self)->Cursor<T>{print("iter");Cursor{items:self.items,index:0usize}}}
fn main()->i32{
 val key=Private{id:1,visits:0};val equal=Private{id:1,visits:0};val shared=Cell{value:20};val later=Cell{value:42};
 val producer:Gather<Private<i32>,Cell,Source<(Private<i32>,Cell)>> =Gather{source:Source{items:[(key,shared),(equal,later)]},key:key,value:shared};
 val maker:Make<Private<i32>> =Make{source:[key,equal]};val map=producer.build();val set=maker.build();
 std::debug::assert(map.len()==1usize && set.len()==1usize && equal.visits>0,"foreign constructors");
 std::debug::assert(map.get(equal).unwrap_or(shared)===later,"last payload");
 val entries=map.entries();match entries.get(0usize){Some(pair)=>{std::debug::assert(pair[0]===key,"original key");},None=>std::debug::panic("entry")};
 later.value
}
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
    let ir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let program = lower_program_to_bytecode(&ir).unwrap();
    for encoded in [false, true] {
        let mut forged = route(&program, encoded);
        let owner = forged
            .modules
            .iter_mut()
            .find(|module| module.identity.path == ["model"])
            .unwrap();
        assert!(owner.dependencies.contains(&forged.root));
        owner
            .dependencies
            .retain(|dependency| *dependency != forged.root);
        assert!(verify_program(&forged).is_err());
        let mut rt = runtime();
        rt.register_host_function(HostFunction::new(standard_log(), |context, _| {
            context.runtime().collect_garbage().unwrap();
            Ok(Value::Unit)
        }))
        .unwrap();
        assert!(rt.load_program("unlinked-private-keys", forged).is_err());
        let loaded = rt
            .load_program("foreign-keys", route(&program, encoded))
            .unwrap();
        let mut vm = Vm::new(rt);
        assert_eq!(
            vm.execute(&loaded, "main").unwrap().return_value,
            Value::I32(42)
        );
        clean(&vm);
    }
}
