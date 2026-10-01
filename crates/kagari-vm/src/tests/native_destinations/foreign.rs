use super::{
    boundaries::{clean, route},
    runtime,
};
use crate::vm::Vm;
use kagari_bytecode::program::{BytecodeProgram, verify_program};
use kagari_common::{
    host_interface::standard_log,
    identity::{ModuleIdentity, PackageId},
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{host::HostFunction, value::Value};
#[test]
fn foreign_fallible_construction_pins_private_factories_traversal_and_keys() {
    let program = compile_foreign(
        r#"
pub trait Build{type Output;fn build(self)->Self::Output;}
pub struct Gather<T,C:FromIterator<T>,I:Iterable<Item=Result<T,String>>>{pub val source:I}
impl<T,C:FromIterator<T>,I:Iterable<Item=Result<T,String>>> Build for Gather<T,C,I>{
 type Output=Result<C,String>;
 fn build(self)->Result<C,String>{<Result<C,String> as FromIterator<Result<T,String>>>::from_iter(self.source)}
}
"#,
        r#"
use pkg::model::{Build,Gather};
struct Cell{var value:i32}
struct Private<T>{val id:T,var visits:i32}
impl<T:PartialEq> PartialEq for Private<T>{fn eq(self,other:Self)->bool{print("eq");self.visits+=1;self.id==other.id}}
impl<T:Eq> Eq for Private<T>{}
impl<T:Eq+Hash> Hash for Private<T>{fn hash(self)->i64{print("hash");self.visits+=1;0i64}}
struct Bag<T>{val items:ArrayList<T>}
impl<T> FromIterator<T> for Bag<T>{fn from_iter<I:Iterable<Item=T>>(source:I)->Self{print("construct");Bag{items:source.iter().collect::<ArrayList<T>>()}}}
struct Cursor<T>{val items:ArrayList<T>,var index:usize}
impl<T> Iterator for Cursor<T>{type Item=T;fn next(self)->Option<T>{print("next");val value=self.items.get(self.index);self.index+=1usize;value}}
struct Source<T>{val items:ArrayList<T>}
impl<T> Iterable for Source<T>{type Item=T;type Iter=Cursor<T>;fn iter(self)->Cursor<T>{print("iter");Cursor{items:self.items,index:0usize}}}
fn main()->i32{
 val key=Private{id:1,visits:0};val equal=Private{id:1,visits:0};val shared=Cell{value:20};val later=Cell{value:42};
 val producer:Gather<(Private<i32>,Cell),LinkedHashMap<Private<i32>,Cell>,Source<Result<(Private<i32>,Cell),String>>> =Gather{source:Source{items:[Ok((key,shared)),Ok((equal,later))]}};
 val maker:Gather<Option<Private<i32>>,Option<Bag<Private<i32>>>,Source<Result<Option<Private<i32>>,String>>> =Gather{source:Source{items:[Ok(Some(key)),Ok(Some(equal))]}};
 val map=producer.build().unwrap_or_else(|error|std::debug::panic(error));
 val bag=maker.build().unwrap_or_else(|error|std::debug::panic(error)).unwrap_or_else(||std::debug::panic("none"));
 std::debug::assert(map.len()==1usize && bag.items.len()==2usize && equal.visits>0,"foreign constructors");
 std::debug::assert(bag.items[0]===key && map.get(equal).unwrap_or(shared)===later,"shallow payloads");
 val entries=map.entries();match entries.get(0usize){Some(pair)=>{std::debug::assert(pair[0]===key,"original key");},None=>std::debug::panic("entry")};
 later.value
}
"#,
    );
    execute_foreign(&program);
}

fn compile_foreign(model: &str, root_source: &str) -> BytecodeProgram {
    let mut sources = SourceDatabase::default();
    let mut root = None;
    for (name, text) in [("model", model), ("root", root_source)] {
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
    lower_program_to_bytecode(&ir).unwrap()
}

fn execute_foreign(program: &BytecodeProgram) {
    for encoded in [false, true] {
        let mut forged = route(program, encoded);
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
            .load_program("foreign-keys", route(program, encoded))
            .unwrap();
        let mut vm = Vm::new(rt);
        assert_eq!(
            vm.execute(&loaded, "main").unwrap().return_value,
            Value::I32(42)
        );
        clean(&vm);
    }
}

#[test]
fn foreign_collect_and_partition_pin_private_factories_next_keys_and_predicates() {
    let program = compile_foreign(
        r#"
pub trait Build{type Output;fn build(self)->Self::Output;}
pub trait Divide<T>{type Output;fn divide(self,predicate:fn(T)->bool)->Self::Output;}
pub struct Gather<T,C:FromIterator<T>,I:Iterator<Item=T>>{pub val source:I}
impl<T,C:FromIterator<T>,I:Iterator<Item=T>> Build for Gather<T,C,I>{type Output=C;fn build(self)->C{self.source.collect()}}
impl<T,C:FromIterator<T>,I:Iterator<Item=T>> Divide<T> for Gather<T,C,I>{type Output=(C,C);fn divide(self,predicate:fn(T)->bool)->(C,C){self.source.partition(predicate)}}
"#,
        r#"
use pkg::model::{Build,Divide,Gather};
struct Cell{var value:i32}
struct Key<T>{val id:T,var visits:i32}
impl<T:PartialEq> PartialEq for Key<T>{fn eq(self,other:Self)->bool{print("eq");self.visits+=1;self.id==other.id}}
impl<T:Eq> Eq for Key<T>{}
impl<T:Eq+Hash> Hash for Key<T>{fn hash(self)->i64{print("hash");self.visits+=1;0i64}}
struct Bag<T>{val items:ArrayList<T>}
impl<T> FromIterator<T> for Bag<T>{fn from_iter<I:Iterable<Item=T>>(source:I)->Self{print("construct");Bag{items:source.iter().collect::<ArrayList<T>>()}}}
struct Cursor<T>{val items:ArrayList<T>,var index:usize}
impl<T> Iterator for Cursor<T>{type Item=T;fn next(self)->Option<T>{print("next");val value=self.items.get(self.index);self.index+=1usize;value}}
fn main()->i32{
 val key=Key{id:1,visits:0};val equal=Key{id:1,visits:0};val other=Key{id:2,visits:0};val early=Cell{value:20};val later=Cell{value:42};
 val map_source:Gather<(Key<i32>,Cell),LinkedHashMap<Key<i32>,Cell>,Cursor<(Key<i32>,Cell)>> =Gather{source:Cursor{items:[(key,early),(other,early),(equal,later)],index:0}};
 val part_source:Gather<(Key<i32>,Cell),LinkedHashMap<Key<i32>,Cell>,Cursor<(Key<i32>,Cell)>> =Gather{source:Cursor{items:[(key,early),(other,early),(equal,later)],index:0}};
 val map=map_source.build();val parts=part_source.divide(|pair|{print("predicate");pair[0].id==1});
 std::debug::assert(map.len()==2usize && parts[0].len()==1usize && parts[1].len()==1usize && equal.visits>0,"native factories");
 std::debug::assert(map.get(equal).unwrap_or(early)===later && parts[0].get(key).unwrap_or(early)===later,"last payload");
 match parts[0].entries().get(0usize){Some(pair)=>std::debug::assert(pair[0]===key,"first key"),None=>std::debug::panic("entry")};
 val bag_source:Gather<Key<i32>,Bag<Key<i32>>,Cursor<Key<i32>>> =Gather{source:Cursor{items:[key,equal,other],index:0}};
 val bag_parts:Gather<Key<i32>,Bag<Key<i32>>,Cursor<Key<i32>>> =Gather{source:Cursor{items:[key,equal,other],index:0}};
 val bag=bag_source.build();val split=bag_parts.divide(|item|{print("predicate");item.id==1});
 std::debug::assert(bag.items.len()==3usize && split[0].items[0]===key && split[0].items[1]===equal && split[1].items[0]===other,"script factories");
 val nested:Gather<Result<Option<Key<i32>>,String>,Result<Option<Bag<Key<i32>>>,String>,Cursor<Result<Option<Key<i32>>,String>>> =Gather{source:Cursor{items:[Ok(Some(key)),Ok(None),Err("error")],index:0}};
 val groups=nested.divide(|item|{print("predicate");item.is_ok()});
 std::debug::assert(groups[0].map_or(false,|items|items.is_none()) && groups[1].is_err(),"nested factories");
 later.value
}
"#,
    );
    execute_foreign(&program);
}
