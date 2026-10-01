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
fn foreign_list_join_pins_private_conversion_and_interface_adapters() {
    let mut sources = SourceDatabase::default();
    let mut root = None;
    for (name, text) in [
        (
            "model",
            r#"
pub trait Run {fn run(self)->String;}
pub struct Runner<L:List<String>> {pub val source:L}
impl<L:List<String>> Run for Runner<L> {fn run(self)->String{self.source.join("/")}}
"#,
        ),
        (
            "root",
            r#"
use pkg::model::{Run,Runner};
struct Sequence<T>{val items:ArrayList<T>}
impl<T> Index<usize> for Sequence<T>{type Output=T;fn index(self,index:usize)->T{self.items[index]}}
impl<T> Iterable for Sequence<T>{type Item=T;type Iter=Iter<T>;fn iter(self)->Iter<T>{print("iter");self.items.iter().map(|item|{print("item");item})}}
impl<T> List<T> for Sequence<T>{fn len(self)->usize{self.items.len()}fn is_empty(self)->bool{self.items.is_empty()}fn get(self,index:usize)->Option<T>{self.items.get(index)}}
fn main()->i32{
 val source=Sequence{items:["é","😀"]};val first:Runner<Sequence<String>> =Runner{source:source};
 val view:List<String> =source;val second:Run=Runner{source:source};
 std::debug::assert(first.run()=="é/😀" && second.run()=="é/😀" && view.join("/")=="é/😀","foreign private list");42
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
            rt.load_program("unlinked-private-list-join", forged)
                .is_err()
        );
        let loaded = rt
            .load_program("foreign-list-join", route(&program, encoded))
            .unwrap();
        let mut vm = Vm::new(rt);
        assert_eq!(
            vm.execute(&loaded, "main").unwrap().return_value,
            Value::I32(42)
        );
        clean(&vm);
    }
}
