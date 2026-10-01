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
fn foreign_grouping_pins_private_keys_iterators_and_callable_adapters() {
    let mut sources = SourceDatabase::default();
    let mut root = None;
    for (name, text) in [
        (
            "model",
            r#"
pub trait Run{fn run(self)->i32;}
pub struct Group<K:Eq+Hash,T,I:Iterator<Item=T>>{pub val source:I,pub val key:fn(T)->K}
impl<K:Eq+Hash,T,I:Iterator<Item=T>> Run for Group<K,T,I>{fn run(self)->i32{val output=self.source.group_by(self.key);std::debug::assert(output.len()==2usize,"foreign groups");21}}
"#,
        ),
        (
            "root",
            r#"
use pkg::model::{Run,Group};
struct Key<T>{val id:T,var visits:i32}
impl<T:PartialEq> PartialEq for Key<T>{fn eq(self,other:Self)->bool{print("eq");self.visits+=1;self.id==other.id}}
impl<T:Eq> Eq for Key<T>{}impl<T:Eq+Hash> Hash for Key<T>{fn hash(self)->i64{print("hash");self.visits+=1;0i64}}
struct Item<K>{val key:K,var visits:i32}
struct Cursor<T>{val items:ArrayList<T>,var index:usize}
impl<T> Iterator for Cursor<T>{type Item=T;fn next(self)->Option<T>{print("next");val item=self.items.get(self.index);self.index+=1usize;item}}
struct Picker<T>{var calls:i32}
impl<T> Fn<(Item<T>,)> for Picker<T>{type Output=T;fn call(self,args:(Item<T>,))->T{self.calls+=1;args[0].visits+=1;print("key");args[0].key}}
fn main()->i32{
 val a=Key{id:20,visits:0};val b=Key{id:21,visits:0};val items=[Item{key:Some(a),visits:0},Item{key:Some(b),visits:0},Item{key:Some(a),visits:0}];
 val picker:Picker<Option<Key<i32>>> =Picker{calls:0};
 val first:Group<Option<Key<i32>>,Item<Option<Key<i32>>>,Cursor<Item<Option<Key<i32>>>>> =Group{source:Cursor{items:items,index:0usize},key:picker};
 val second:Group<Key<i32>,Key<i32>,Iter<Key<i32>>> =Group{source:[a,b,a].iter(),key:|item|{print("closure");item}};
 val result=first.run()+second.run();std::debug::assert(picker.calls==3 && items[0usize].visits==1 && items[1usize].visits==1 && items[2usize].visits==1 && a.visits>0 && b.visits>0,"private effects");result
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
        assert!(
            rt.load_program("unlinked-private-grouping", forged)
                .is_err()
        );
        let loaded = rt
            .load_program("foreign-grouping", route(&program, encoded))
            .unwrap();
        let mut vm = Vm::new(rt);
        assert_eq!(
            vm.execute(&loaded, "main").unwrap().return_value,
            Value::I32(42)
        );
        clean(&vm);
    }
}
