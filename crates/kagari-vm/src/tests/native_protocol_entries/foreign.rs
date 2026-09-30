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
use std::{cell::RefCell, rc::Rc};

#[test]
fn foreign_protocol_entries_pin_private_generic_methods_and_associated_outputs() {
    let mut sources = SourceDatabase::default();
    let mut root = None;
    for (name, text) in [
        (
            "model",
            r#"
pub trait Run {fn run(self)->bool;}
pub struct Runner<T:FromStr+PartialEq> {pub val value:T}
impl<T:FromStr+PartialEq> Run for Runner<T> {
 fn run(self)->bool {"42".parse::<T>().map_or(false,|value|{std::debug::assert_eq(Some(value),Some(self.value),"private generic methods");true})}
}
"#,
        ),
        (
            "root",
            r#"
use pkg::model::{Run,Runner};
struct Wrapped<T> {val value:T}
impl<T:FromStr> FromStr for Wrapped<T> {type Err=<T as FromStr>::Err;fn from_str(text:String)->Result<Self,Self::Err>{print("parse");text.parse::<T>().map(|value|{print("wrap");Wrapped{value:value}})}}
impl<T:PartialEq> PartialEq for Wrapped<T> {fn eq(self,other:Self)->bool{print("eq");self.value==other.value}}
fn main()->i32 {
 val concrete:Runner<Wrapped<i32>> =Runner{value:Wrapped{value:42}};
 val dynamic:Run=Runner{value:Wrapped{value:42}};
 std::debug::assert(concrete.run() && dynamic.run(),"foreign parse/equality");42
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
        let effects = Rc::new(RefCell::new(Vec::new()));
        let sink = effects.clone();
        let mut rt = runtime();
        rt.register_host_function(HostFunction::new(standard_log(), move |context, args| {
            let Value::Str(label) = &args[0] else {
                panic!()
            };
            sink.borrow_mut().push(label.clone());
            context.runtime().collect_garbage().unwrap();
            Ok(Value::Unit)
        }))
        .unwrap();
        assert!(
            rt.load_program("unlinked-private-protocols", forged)
                .is_err()
        );
        let loaded = rt
            .load_program("foreign-protocols", route(&program, encoded))
            .unwrap();
        let mut vm = Vm::new(rt);
        assert_eq!(
            vm.execute(&loaded, "main").unwrap().return_value,
            Value::I32(42)
        );
        assert_eq!(
            *effects.borrow(),
            ["parse", "wrap", "eq", "parse", "wrap", "eq"]
        );
        clean(&vm);
    }
}
