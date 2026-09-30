use super::{
    boundaries::{clean, route},
    runtime,
};
use crate::Vm;
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
fn foreign_sets_pin_private_membership_traversal_and_key_protocols() {
    let mut sources = SourceDatabase::default();
    let mut root = None;
    for (name, text) in [
        (
            "model",
            r#"
pub trait Run{fn run(self)->i32;}
pub struct Algebra<K:Eq+Hash,S:Set<K>>{pub val left:S,pub val right:Set<K>}
impl<K:Eq+Hash,S:Set<K>> Run for Algebra<K,S>{fn run(self)->i32{
 val union=self.left.union(self.right);val intersection=self.left.intersection(self.right);val difference=self.left.difference(self.right);val symmetric=self.left.symmetric_difference(self.right);
 std::debug::assert(union.len()==3usize && intersection.len()==1usize && difference.len()==1usize && symmetric.len()==2usize,"foreign algebra");
 std::debug::assert(!self.left.is_subset(self.right) && !self.left.is_superset(self.right) && !self.left.is_disjoint(self.right),"foreign relations");21
}}
pub struct Relations<K,S:Set<K>>{pub val left:S,pub val right:Set<K>}
impl<K,S:Set<K>> Run for Relations<K,S>{fn run(self)->i32{std::debug::assert(self.left.is_subset(self.right) && self.left.is_superset(self.right) && !self.left.is_disjoint(self.right),"without hash");21}}
"#,
        ),
        (
            "root",
            r#"
use pkg::model::{Run,Algebra,Relations};
struct Private<T>{val id:T,var visits:i32}
impl<T:PartialEq> PartialEq for Private<T>{fn eq(self,other:Self)->bool{print("eq");self.visits+=1;self.id==other.id}}
impl<T:Eq> Eq for Private<T>{}
impl<T:Eq+Hash> Hash for Private<T>{fn hash(self)->i64{print("hash");self.visits+=1;0i64}}
struct Policy<T:PartialEq>{val items:ArrayList<T>}
impl<T:PartialEq> Iterable for Policy<T>{type Item=T;type Iter=Iter<T>;fn iter(self)->Iter<T>{print("iter");self.items.iter().inspect(|item|print("next"))}}
impl<T:PartialEq> Set<T> for Policy<T>{fn len(self)->usize{self.items.len()}fn is_empty(self)->bool{self.items.is_empty()}fn contains(self,item:T)->bool{print("contains");self.items.contains(item)}}
fn main()->i32{
 val a=Private{id:20,visits:0};val b=Private{id:21,visits:0};val c=Private{id:22,visits:0};
 val builder:Algebra<Private<i32>,Policy<Private<i32>>> =Algebra{left:Policy{items:[a,b]},right:Policy{items:[b,c]}};
 val same=Policy{items:[20.0,21.0]};val query:Relations<f64,Policy<f64>> =Relations{left:same,right:same};
 val result=builder.run()+query.run();std::debug::assert(a.visits>0 && b.visits>0 && c.visits>0,"private keys");result
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
