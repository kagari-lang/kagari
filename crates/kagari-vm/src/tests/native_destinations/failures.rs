use super::{
    boundaries::{clean, route},
    cases::TYPES,
    runtime,
};
use crate::{Vm, VmError, executor::Executor, tests::common::compile_test_bytecode};
use kagari_common::host_interface::standard_log;
use kagari_runtime::{RuntimeErrorKind, host::HostFunction, value::Value};
use std::{cell::RefCell, rc::Rc};

#[test]
fn fallible_collection_preserves_original_error_object_and_origin() {
    let program = compile_test_bytecode(&format!(
        r#"
{TYPES}
struct Error{{val message:String}}
fn fault()->Result<i32,Error>{{Err(Error{{message:"original"}})}}
fn setup()->ArrayList<Result<i32,Error>>{{[Ok(20),fault(),Ok(22)]}}
fn main(items:ArrayList<Result<i32,Error>>)->Result<Bag<i32>,Error>{{items.iter().collect()}}
"#
    ));
    let entry = program.modules[program.root.index()]
        .functions
        .iter()
        .find(|function| function.name == "main")
        .unwrap()
        .id;
    for encoded in [false, true] {
        let mut rt = runtime();
        rt.register_host_function(HostFunction::new(standard_log(), |_, _| {
            panic!("failure must skip destination")
        }))
        .unwrap();
        let loaded = rt
            .load_program("fallible-origin", route(&program, encoded))
            .unwrap();
        let mut vm = Vm::new(rt);
        let fixture = vm.execute(&loaded, "setup").unwrap().return_value;
        let roots = vm.runtime().gc().root_value(fixture.clone()).unwrap();
        let Value::Array(array) = fixture.clone() else {
            panic!()
        };
        let original = vm.runtime().gc().array_get(array, 1).unwrap();
        let Value::Enum(id) = original.clone() else {
            panic!()
        };
        let original_error = vm.runtime().gc().enum_snapshot(id).unwrap().fields[0].clone();
        let failure = vm.runtime().result_failure(&original).unwrap();
        let result = Executor::new(vm.runtime(), &loaded, entry, &[fixture])
            .unwrap()
            .run()
            .unwrap();
        assert_eq!(vm.runtime().result_failure(&result).unwrap(), failure);
        let Value::Enum(id) = result.clone() else {
            panic!()
        };
        assert_eq!(
            vm.runtime().gc().enum_snapshot(id).unwrap().fields[0],
            original_error
        );
        let result_root = vm.runtime().gc().root_value(result.clone()).unwrap();
        vm.runtime().collect_garbage().unwrap();
        assert_eq!(vm.runtime().result_failure(&result).unwrap(), failure);
        drop(result_root);
        drop(roots);
        clean(&vm);
    }
}
#[test]
fn fallible_constructor_and_source_traps_preserve_order_and_cleanup() {
    for (source, expected) in [
        ("[Some(42)].iter()", vec!["construct"]),
        ("Fault{index:0}", vec!["next"]),
    ] {
        let program = compile_test_bytecode(&format!(
            r#"
struct Fault{{var index:i32}}
impl Iterator for Fault{{type Item=Option<i32>;fn next(self)->Option<Option<i32>>{{print("next");val x=2147483647;val bad=x+1;Some(Some(bad))}}}}
struct Broken{{val value:i32}}
impl FromIterator<i32> for Broken{{fn from_iter<I:Iterable<Item=i32>>(source:I)->Self{{print("construct");val x=2147483647;Broken{{value:x+1}}}}}}
fn main()->i32{{val result:Option<Broken> = {source}.collect();print("unreachable");0}}
fn ready()->i32{{7}}
"#
        ));
        for encoded in [false, true] {
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
            let loaded = rt
                .load_program("fallible-trap", route(&program, encoded))
                .unwrap();
            let mut vm = Vm::new(rt);
            let Err(VmError::RuntimeError(error)) = vm.execute(&loaded, "main") else {
                panic!("expected trap")
            };
            assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
            assert_eq!(effects.borrow().as_slice(), expected);
            assert_eq!(
                vm.execute(&loaded, "ready").unwrap().return_value,
                Value::I32(7)
            );
            clean(&vm);
        }
    }
}

#[test]
fn fallible_collection_stops_polling_and_releases_its_native_source_guard() {
    let program = compile_test_bytecode(
        r#"
struct Cursor{var polls:i32,val short:bool}
impl Iterator for Cursor{type Item=Option<i32>;fn next(self)->Option<Option<i32>>{self.polls+=1;if self.polls==1{if self.short{Some(None)}else{None}}else{std::debug::panic("polled after termination")}}}
fn main()->i32{
 val short=Cursor{polls:0,short:true};val empty=Cursor{polls:0,short:false};
 val absent:Option<ArrayList<i32>> =short.collect();val present:Option<ArrayList<i32>> =empty.collect();
 std::debug::assert(short.polls==1 && empty.polls==1 && absent.is_none() && present.map_or(false,|items|items.is_empty()),"termination");
 val values:ArrayList<Option<i32>> =[Some(42),None];val iterator=values.iter();
 val stopped:Option<ArrayList<i32>> =iterator.collect();
 values.push(Some(7));
 std::debug::assert(stopped.is_none() && values.len()==3usize,"released");
 val remaining:ArrayList<Option<i32>> =[Some(42),None,Some(99)];val cursor=remaining.iter();
 val interrupted:Option<ArrayList<i32>> =cursor.collect();
 std::debug::assert(interrupted.is_none() && cursor.next()==Some(Some(99)) && cursor.next().is_none(),"remaining progress");
 remaining.push(Some(7));
 val view:List<Option<i32>> =[Some(20),Some(22)];
 val result=<Option<ArrayList<i32>> as FromIterator<Option<i32>>>::from_iter(view);
 result.map_or(0,|items|items.iter().sum())
}
"#,
    );
    for encoded in [false, true] {
        let mut rt = runtime();
        let loaded = rt
            .load_program("fallible-termination", route(&program, encoded))
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
fn fallible_collection_guards_alias_mutation_during_lazy_source_callbacks() {
    let program = compile_test_bytecode(
        r#"
fn main()->i32{
 val values:ArrayList<Option<i32>> =[Some(42)];
 val iterator=values.iter().map(|item|{print("mutation");values.push(None);item});
 val result:Option<ArrayList<i32>> =iterator.collect();print("unreachable");0
}fn ready()->i32{7}
"#,
    );
    for encoded in [false, true] {
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
        let loaded = rt
            .load_program("fallible-alias", route(&program, encoded))
            .unwrap();
        let mut vm = Vm::new(rt);
        assert!(vm.execute(&loaded, "main").is_err());
        assert_eq!(effects.borrow().as_slice(), ["mutation"]);
        assert_eq!(
            vm.execute(&loaded, "ready").unwrap().return_value,
            Value::I32(7)
        );
        clean(&vm);
    }
}

#[test]
fn fallible_destination_uses_the_selected_script_factory_for_enum_outputs() {
    let program = compile_test_bytecode(
        r#"
enum Choice{Value(i32)}
impl FromIterator<i32> for Choice{
 fn from_iter<I:Iterable<Item=i32>>(source:I)->Self{print("script enum");Choice::Value(source.iter().sum())}
}
fn main()->i32{
 val result:Option<Choice> =[Some(20),Some(22)].iter().collect();
 result.map_or(0,|choice|match choice{Choice::Value(value)=>value})
}
"#,
    );
    for encoded in [false, true] {
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
        let loaded = rt
            .load_program("script-enum-factory", route(&program, encoded))
            .unwrap();
        let mut vm = Vm::new(rt);
        assert_eq!(
            vm.execute(&loaded, "main").unwrap().return_value,
            Value::I32(42)
        );
        assert_eq!(effects.borrow().as_slice(), ["script enum"]);
        clean(&vm);
    }
}
