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
fn foreign_generic_key_protocols_and_map_callbacks_pin_private_defining_modules() {
    let mut sources = SourceDatabase::default();
    let mut root = None;
    for (name, text) in [
        (
            "model",
            r#"
pub trait Apply{fn apply(self);}
pub struct Editor<K:Eq+Hash,V>{pub val items:LinkedHashMap<K,V>,pub val query:K,pub val value:V,pub val factory:fn()->V,pub val transform:fn(Option<V>)->V}
impl<K:Eq+Hash,V> Apply for Editor<K,V>{fn apply(self){
 std::debug::assert(self.items.contains_key(self.query),"foreign membership");self.items.get(self.query);
 self.items.insert(self.query,self.value);self.items.remove(self.query);
 self.items.get_or_insert_with(self.query,|| (self.factory)());
 self.items.update(self.query,|old| (self.transform)(old));
}}
pub struct SetEditor<K:Eq+Hash>{pub val items:LinkedHashSet<K>,pub val query:K}
impl<K:Eq+Hash> Apply for SetEditor<K>{fn apply(self){std::debug::assert(self.items.contains(self.query),"foreign set");self.items.remove(self.query);self.items.insert(self.query);}}
"#,
        ),
        (
            "root",
            r#"
use pkg::model::{Apply,Editor,SetEditor};
struct Cell{var value:i32}
struct Private<T>{val id:T,var visits:i32}
impl<T:PartialEq> PartialEq for Private<T>{fn eq(self,other:Self)->bool{print("eq");self.visits+=1;self.id==other.id}}
impl<T:Eq> Eq for Private<T>{}
impl<T:Eq+Hash> Hash for Private<T>{fn hash(self)->i64{print("hash");self.visits+=1;0i64}}
fn main()->i32{
 val key=Private{id:1,visits:0};val equal=Private{id:1,visits:0};val shared=Cell{value:20};
 val map:LinkedHashMap<Private<i32>,Cell> =LinkedHashMap::new();map.insert(key,shared);
 val editor:Editor<Private<i32>,Cell> =Editor{items:map,query:equal,value:shared,factory:||{print("factory");shared},transform:|old|{print("transform");val cell=old.unwrap_or(shared);cell.value+=2;cell}};
 editor.apply();std::debug::assert(map.get(equal).unwrap_or(shared)===shared,"private callback payload");
 val set:LinkedHashSet<Private<i32>> =LinkedHashSet::new();set.insert(key);
 val editing:SetEditor<Private<i32>> =SetEditor{items:set,query:equal};editing.apply();
 std::debug::assert(map.len()==1usize && set.len()==1usize && equal.visits>0,"foreign commits");shared.value+20
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
        clean(vm.runtime());
    }
}
#[test]
fn builtin_identity_keys_and_readonly_collection_view_keys_share_key_policy() {
    let program = compile_test_bytecode(
        r#"
struct DefaultKey{val value:i32}
fn put<K:Eq+Hash>(map:MutableMap<K,i32>,key:K){map.insert(key,42);}
fn main()->i32{
 val key=DefaultKey{value:1};val other=DefaultKey{value:1};
 val native:LinkedHashMap<DefaultKey,i32> =LinkedHashMap::new();put(native,key);
 std::debug::assert(native.get(key)==Some(42) && native.get(other)==None,"default struct identity");
 val array=[1];val readonly:List<i32> =array;val second:List<i32> =array;val distinct:List<i32> =[1];
 val views:LinkedHashMap<List<i32>,i32> =LinkedHashMap::new();put(views,readonly);
 std::debug::assert(views.get(second)==Some(42) && views.get(distinct)==None,"view identity hash");
 views.update(second,|old|old.unwrap_or(0)+1);views.remove(readonly);
 std::debug::assert(views.len()==0usize,"view mutation");42
}
"#,
    );
    for encoded in [false, true] {
        let mut rt = runtime();
        let loaded = rt
            .load_program("identity-keys", route(&program, encoded))
            .unwrap();
        let mut vm = Vm::new(rt);
        assert_eq!(
            vm.execute(&loaded, "main").unwrap().return_value,
            Value::I32(42)
        );
        clean(vm.runtime());
    }
}
