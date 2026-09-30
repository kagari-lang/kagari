use super::{
    boundaries::{clean, route},
    runtime,
};
use crate::{Vm, tests::common::compile_test_bytecode};
use kagari_bytecode::verify_program;
use kagari_common::{
    host_interface::standard_log,
    identity::{ModuleIdentity, PackageId},
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{host::HostFunction, value::Value};

#[test]
fn foreign_generic_sorting_and_equality_use_private_keys_and_payload_layouts() {
    let mut sources = SourceDatabase::default();
    let mut root = None;
    for (name, text) in [
        (
            "model",
            r#"
pub struct Ranked<K,V>{pub val key:K,pub val payload:V,pub val tag:i32}
impl<K:PartialEq,V> PartialEq for Ranked<K,V>{fn eq(self,other:Self)->bool{print("rank_eq");self.key==other.key}}
impl<K:Eq,V> Eq for Ranked<K,V>{}
impl<K:PartialOrd,V> PartialOrd for Ranked<K,V>{fn partial_cmp(self,other:Self)->Option<Ordering>{self.key.partial_cmp(other.key)}}
impl<K:Ord,V> Ord for Ranked<K,V>{fn cmp(self,other:Self)->Ordering{print("rank_cmp");self.key.cmp(other.key)}}
pub trait Sorter{fn apply(self);}
pub trait Keyed{type Key:Ord;fn key(self)->Self::Key;}
pub struct ProjectedSorter<T:Keyed>{pub val items:ArrayList<T>}
impl<T:Keyed> Sorter for ProjectedSorter<T>{fn apply(self){self.items.sort_by_key(|item|item.key());}}
pub struct CompareSorter<T>{pub val items:ArrayList<T>,pub val compare:fn(T,T)->Ordering}
impl<T> Sorter for CompareSorter<T>{fn apply(self){self.items.sort_by(|a,b|(self.compare)(a,b));}}
pub struct KeySorter<T,K>{pub val items:ArrayList<T>,pub val extract:fn(T)->K}
impl<T,K:Ord> Sorter for KeySorter<T,K>{fn apply(self){self.items.sort_by_key(|item|(self.extract)(item));}}
"#,
        ),
        (
            "root",
            r#"
use pkg::model::{Ranked,Sorter,CompareSorter,KeySorter,Keyed,ProjectedSorter};
struct Cell{var value:i32}
struct Secret{val key:Key,val payload:Cell,var visits:i32}
impl Keyed for Secret{type Key=Key;fn key(self)->Key{print("projected_key");self.visits+=1;self.key}}
struct Key{val value:i32,var visits:i32}
impl PartialEq for Key{fn eq(self,other:Self)->bool{print("key_eq");self.visits+=1;self.value==other.value}}
impl Eq for Key{}
impl PartialOrd for Key{fn partial_cmp(self,other:Self)->Option<Ordering>{self.value.partial_cmp(other.value)}}
impl Ord for Key{fn cmp(self,other:Self)->Ordering{print("key_cmp");self.visits+=1;self.value.cmp(other.value)}}
fn main()->i32{
 val values:ArrayList<Ranked<Key,Cell>> =[
   Ranked{key:Key{value:2,visits:0},payload:Cell{value:20},tag:0},
   Ranked{key:Key{value:1,visits:0},payload:Cell{value:22},tag:1},
   Ranked{key:Key{value:1,visits:0},payload:Cell{value:99},tag:2}];
 val compared=ArrayList::from(values);val keyed=ArrayList::from(values);
 val by:CompareSorter<Ranked<Key,Cell>> =CompareSorter{items:compared,compare:|a,b|a.key.cmp(b.key)};by.apply();
 val by_key:KeySorter<Ranked<Key,Cell>,Key> =KeySorter{items:keyed,extract:|item|{print("key");item.key}};by_key.apply();
 val projected_items=[Secret{key:values[0usize].key,payload:values[0usize].payload,visits:0},Secret{key:values[1usize].key,payload:values[1usize].payload,visits:0}];
 val projected:ProjectedSorter<Secret> =ProjectedSorter{items:projected_items};projected.apply();
 std::debug::assert(projected_items[0usize].payload.value==22 && projected_items[1usize].payload.value==20,"private associated key");
 std::debug::assert(projected_items[0usize].visits==1 && projected_items[1usize].visits==1,"projected key once");
 values.sort();
 std::debug::assert(values[0usize].tag==1 && values[1usize].tag==2 && values[2usize].tag==0,"foreign stable order");
 std::debug::assert(compared[0usize].tag==1 && keyed[0usize].tag==1,"foreign callback layout");
 val payload=values[0usize].payload;compared[0usize].payload.value=21;
 std::debug::assert(payload.value==21 && keyed[0usize].payload.value==21,"shared private payload");payload.value=22;
 values.dedup();std::debug::assert(values.len()==2usize && values[0usize].tag==1 && values[1usize].tag==0,"foreign equality targets");
 values[0usize].payload.value+values[1usize].payload.value
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
        assert!(
            verify_program(&forged).is_err(),
            "private implementation needs its executable dependency"
        );
        let mut rt = runtime();
        rt.register_host_function(HostFunction::new(standard_log(), |_, _| Ok(Value::Unit)))
            .unwrap();
        assert!(rt.load_program("unlinked-private-sort", forged).is_err());
    }
    for encoded in [false, true] {
        let mut rt = runtime();
        rt.register_host_function(HostFunction::new(standard_log(), |context, _| {
            context.runtime().collect_garbage().unwrap();
            Ok(Value::Unit)
        }))
        .unwrap();
        let loaded = rt
            .load_program("foreign-sort", route(&program, encoded))
            .unwrap();
        let mut vm = Vm::new(rt);
        assert_eq!(
            vm.execute(&loaded, "main").unwrap().return_value,
            Value::I32(42)
        );
        clean(vm.runtime());
    }
}

#[test]
fn primitive_and_dynamic_collection_dedup_keep_existing_identity_equality() {
    let program = compile_test_bytecode(
        r#"
fn dedup<T:PartialEq>(items:ArrayList<T>){items.dedup();}
fn main()->i32{
 val item=[1];val other=[1];
 val concrete=[item,item,other,item];dedup(concrete);
 std::debug::assert(concrete.len()==3usize,"array identity");
 val view:List<i32> =item;val duplicate:List<i32> =item;val distinct:List<i32> =other;
 val dynamic:ArrayList<List<i32>> =[view,duplicate,distinct,view];dedup(dynamic);
 std::debug::assert(dynamic.len()==3usize,"dynamic identity");
 val optional:ArrayList<Option<i32>> =[None,None,Some(1),Some(1),None];optional.dedup();
 std::debug::assert(optional.len()==3usize,"primitive option composition");
 val tuple=[(1,2),(1,2),(1,3),(1,2)];tuple.dedup();
 std::debug::assert(tuple.len()==3usize,"primitive tuple composition");42
}
"#,
    );
    for encoded in [false, true] {
        let mut rt = runtime();
        let loaded = rt
            .load_program("identity-dedup", route(&program, encoded))
            .unwrap();
        let mut vm = Vm::new(rt);
        assert_eq!(
            vm.execute(&loaded, "main").unwrap().return_value,
            Value::I32(42)
        );
        clean(vm.runtime());
    }
}
