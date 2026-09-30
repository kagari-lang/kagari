use super::{
    boundaries::{clean, route},
    runtime,
};
use crate::{Vm, VmError, tests::common::compile_test_bytecode};
use kagari_common::host_interface::standard_log;
use kagari_runtime::{RuntimeErrorKind, host::HostFunction, value::Value};
use std::{cell::RefCell, rc::Rc};

#[test]
fn native_parse_preserves_result_error_provenance_across_gc() {
    let program = compile_test_bytecode(
        r#"
struct Failure {val value:i32}
impl FromStr for Failure {type Err=String;fn from_str(text:String)->Result<Self,String>{print("parse");Err("custom parse")}}
fn main()->Result<Failure,String>{"bad".parse::<Failure>()}
fn direct()->Result<Failure,String>{Failure::from_str("bad")}
"#,
    );
    for encoded in [false, true] {
        let mut rt = runtime();
        rt.register_host_function(HostFunction::new(standard_log(), |context, _| {
            context.runtime().collect_garbage().unwrap();
            Ok(Value::Unit)
        }))
        .unwrap();
        let loaded = rt
            .load_program("parse-origin", route(&program, encoded))
            .unwrap();
        let mut vm = Vm::new(rt);
        let mut origins = Vec::new();
        for entry in ["direct", "main"] {
            let value = vm.execute(&loaded, entry).unwrap().return_value;
            let failure = vm.runtime().result_failure(&value).unwrap();
            assert_eq!(failure.message, "custom parse");
            assert_eq!(failure.trace.frames.len(), 2);
            let origin = &failure.trace.frames[0];
            assert!(origin.function_name.contains("from_str"));
            assert!(origin.source_span.is_some());
            assert_eq!(failure.trace.frames[1].function_name, entry);
            let root = vm.runtime().gc().root_value(value.clone()).unwrap();
            vm.runtime().collect_garbage().unwrap();
            assert_eq!(vm.runtime().result_failure(&value).unwrap(), failure);
            origins.push((
                origin.function,
                origin.instruction_offset,
                origin.source_span,
            ));
            drop(root);
            clean(&vm);
        }
        assert_eq!(origins[0], origins[1]);
    }
}

#[test]
fn protocol_callback_traps_preserve_completed_effects_origins_and_cleanup() {
    for (call, method, effects) in [
        (
            "val result=\"42\".parse::<Fault>();",
            "from_str",
            vec!["parse"],
        ),
        (
            "std::debug::assert_eq(Fault{value:1},Fault{value:1},\"equal\");",
            "eq",
            vec!["eq"],
        ),
        (
            "std::debug::assert_eq(Some(Fault{value:1}),Some(Fault{value:1}),\"equal\");",
            "eq",
            vec!["eq"],
        ),
    ] {
        let program = compile_test_bytecode(&format!(
            r#"
struct Fault {{val value:i32}}
impl FromStr for Fault {{type Err=String;fn from_str(text:String)->Result<Self,String>{{print("parse");val x=2147483647;val bad=x+1;Err("unused")}}}}
impl PartialEq for Fault {{fn eq(self,other:Self)->bool{{print("eq");val x=2147483647;val bad=x+1;true}}}}
fn main()->i32{{{call}print("unreachable");0}}fn ready()->i32{{7}}
"#
        ));
        for encoded in [false, true] {
            let saved = Rc::new(RefCell::new(Vec::new()));
            let sink = saved.clone();
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
                .load_program("protocol-trap", route(&program, encoded))
                .unwrap();
            let mut vm = Vm::new(rt);
            let Err(VmError::RuntimeError(error)) = vm.execute(&loaded, "main") else {
                panic!("{call}")
            };
            assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
            assert_eq!(*saved.borrow(), effects);
            let trace = error.trace().unwrap();
            assert!(trace.frames[0].function_name.contains(method));
            assert!(trace.frames[0].source_span.is_some());
            assert_eq!(trace.frames.last().unwrap().function_name, "main");
            clean(&vm);
            assert_eq!(
                vm.execute(&loaded, "ready").unwrap().return_value,
                Value::I32(7)
            );
        }
    }
}
