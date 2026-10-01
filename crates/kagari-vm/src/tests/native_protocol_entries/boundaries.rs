use super::runtime;
use crate::{
    error::VmError, executor::Executor, reentry::reenter, tests::common::compile_test_bytecode,
    vm::Vm,
};

use kagari_bytecode::{artifact::KbcArtifact, program::BytecodeProgram};
use kagari_common::{cancellation::CancellationToken, host_interface::standard_log};
use kagari_runtime::{error::RuntimeErrorKind, host::HostFunction, value::Value};
use std::{cell::RefCell, rc::Rc};
pub(super) fn route(program: &BytecodeProgram, encoded: bool) -> BytecodeProgram {
    if encoded {
        KbcArtifact::from_bytes(
            &KbcArtifact::from_program(program.clone(), Default::default())
                .unwrap()
                .to_bytes()
                .unwrap(),
        )
        .unwrap()
        .program
    } else {
        program.clone()
    }
}
pub(super) fn clean(vm: &Vm) {
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
    assert!(!vm.runtime().is_quarantined());
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(vm.runtime().gc().allocated_objects(), 0);
}
#[test]
fn protocol_entries_reenter_cancel_and_exhaust_every_allocation_limit() {
    for (name, body) in [
        (
            "parse_scalar",
            "std::debug::assert(text.parse::<i32>()==Ok(42),\"scalar\");",
        ),
        (
            "parse_custom",
            "std::debug::assert(text.parse::<Parsed>().is_ok(),\"custom\");",
        ),
        (
            "assert_primitive",
            "std::debug::assert_eq(left(42),right(42),message());",
        ),
        (
            "assert_nominal",
            "std::debug::assert_eq(left(Leaf{value:42,visits:0}),right(Leaf{value:42,visits:0}),message());",
        ),
        (
            "assert_composed",
            "std::debug::assert_eq(left(Some(Leaf{value:42,visits:0})),right(Some(Leaf{value:42,visits:0})),message());",
        ),
    ] {
        let program = compile_test_bytecode(&format!(
            r#"
struct Parsed {{val value:i32}}
impl FromStr for Parsed {{type Err=String;fn from_str(text:String)->Result<Self,String>{{print("parse");text.parse::<i32>().map(|value|{{print("wrap");Parsed{{value:value}}}}).map_err(|error|"bad")}}}}
struct Leaf {{val value:i32,var visits:i32}}
impl PartialEq for Leaf {{fn eq(self,other:Self)->bool{{print("eq");self.visits+=1;self.value==other.value}}}}
fn left<T>(value:T)->T{{print("left");value}}fn right<T>(value:T)->T{{print("right");value}}fn message()->String{{print("message");"entry"}}
fn setup()->String{{"42"}}
fn main(text:String)->i32{{print("start");{body}print("done");42}}
fn inner()->i32{{val result="42".parse::<i32>();std::debug::assert_eq(result,Ok(42),"inner");42}}
fn fail()->i32{{std::debug::assert_eq(1,2,"inner failure");0}}fn ready()->i32{{7}}
"#
        ));
        let root = &program.modules[program.root.index()];
        let entry = root.functions.iter().find(|f| f.name == "main").unwrap().id;
        let inner = root
            .functions
            .iter()
            .find(|f| f.name == "inner")
            .unwrap()
            .id;
        let fail = root.functions.iter().find(|f| f.name == "fail").unwrap().id;
        for encoded in [false, true] {
            let mut labels = Vec::new();
            let mut maximum = 0;
            for phase in 0..4 {
                let scenarios: Vec<_> = match phase {
                    0 => vec![(0, None)],
                    1 => (1..=labels.len()).map(|i| (i, None)).collect(),
                    2 => (0..=maximum).map(|i| (0, Some(i))).collect(),
                    _ => vec![(0, None)],
                };
                for (cancellation, limit) in scenarios {
                    let token = CancellationToken::default();
                    let cancel = token.clone();
                    let effects = Rc::new(RefCell::new(Vec::new()));
                    let sink = effects.clone();
                    let mut rt = runtime();
                    rt.register_host_function(HostFunction::new(
                        standard_log(),
                        move |context, args| {
                            let Value::Str(label) = &args[0] else {
                                panic!()
                            };
                            sink.borrow_mut().push(label.clone());
                            if cancellation == sink.borrow().len() {
                                cancel.cancel();
                            } else if phase == 3 {
                                let root = context.runtime().execution_root().unwrap();
                                let depth =
                                    context.runtime().resources().counters().current_call_depth;
                                assert_eq!(
                                    reenter(context, &root, inner, &[]).unwrap().value(),
                                    Value::I32(42)
                                );
                                assert!(reenter(context, &root, fail, &[]).is_err());
                                assert_eq!(
                                    context.runtime().resources().counters().current_call_depth,
                                    depth
                                );
                            }
                            context.runtime().collect_garbage().unwrap();
                            Ok(Value::Unit)
                        },
                    ))
                    .unwrap();
                    let loaded = rt.load_program(name, route(&program, encoded)).unwrap();
                    let mut vm = Vm::new(rt);
                    let fixture = vm.execute(&loaded, "setup").unwrap().return_value;
                    let roots = vm.runtime().gc().root_value(fixture.clone()).unwrap();
                    let mut options = vm.runtime().execution_options();
                    options.cancellation = token;
                    options.resources.max_allocation_units = limit;
                    let session = vm.runtime().begin_execution(&loaded, options).unwrap();
                    let result = Executor::new(vm.runtime(), &loaded, entry, &[fixture])
                        .unwrap()
                        .run();

                    if phase == 0 {
                        labels = effects.borrow().clone();
                        maximum = session.counters().allocation_units;
                    }
                    if cancellation > 0 {
                        assert!(
                            matches!(result,Err(ref error) if matches!(error.cause(),VmError::RuntimeError(cause) if cause.kind()==RuntimeErrorKind::Cancelled))
                        );
                        assert_eq!(*effects.borrow(), labels[..cancellation]);
                    } else if limit.is_some_and(|i| i < maximum) {
                        assert!(
                            matches!(result,Err(ref error) if matches!(error.cause(),VmError::RuntimeError(cause) if cause.kind()==RuntimeErrorKind::ResourceLimitExceeded))
                        );
                        assert!(labels.starts_with(&effects.borrow()));
                    } else {
                        assert_eq!(result.unwrap(), Value::I32(42));
                        assert_eq!(*effects.borrow(), labels);
                    }
                    drop(session);
                    assert_eq!(
                        vm.execute(&loaded, "ready").unwrap().return_value,
                        Value::I32(7)
                    );
                    drop(roots);
                    clean(&vm);
                }
            }
        }
    }
}
