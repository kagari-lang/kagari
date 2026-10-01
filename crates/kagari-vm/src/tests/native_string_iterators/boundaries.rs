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
fn string_constructors_reenter_cancel_and_exhaust_every_allocation_limit() {
    for (method, item, text, args, length) in [
        ("bytes", "u8", "é😀", "", 6),
        ("char_indices", "(usize,String)", "é😀", "", 2),
        ("split", "String", "a,,b,", "separator(\",\")", 4),
        (
            "splitn",
            "String",
            "a,b,c",
            "count(2usize),separator(\",\")",
            2,
        ),
        ("split_whitespace", "String", " a　b ", "", 2),
        ("lines", "String", "a\r\nb\n", "", 2),
    ] {
        let program = compile_test_bytecode(&format!(
            r#"
fn source(value:String)->String{{print("source");value}}fn separator(value:String)->String{{print("separator");value}}fn count(value:usize)->usize{{print("count");value}}
fn setup()->String{{{text:?}}}
fn main(text:String)->i32{{val iterator:Iter<{item}> =source(text).{method}({args});print("created");var length=0usize;for item in iterator{{print("item");length+=1usize;}}std::debug::assert(length=={length}usize,"items");print("done");42}}
fn inner()->i32{{"é😀".bytes().count() as i32+36}}
fn fail()->i32{{val x=2147483647;x+1}}fn ready()->i32{{7}}
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
                    let loaded = rt
                        .load_program("string-iterator-boundaries", route(&program, encoded))
                        .unwrap();
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

#[test]
fn returned_string_iterators_keep_typed_state_across_sessions_and_collection() {
    for (method, item, args, length) in [
        ("bytes", "u8", "", 8),
        ("char_indices", "(usize,String)", "", 4),
        ("split", "String", "\",\"", 3),
        ("splitn", "String", "2usize,\",\"", 2),
        ("split_whitespace", "String", "", 1),
        ("lines", "String", "", 1),
    ] {
        let program = compile_test_bytecode(&format!(
            r#"
fn new()->Iter<{item}>{{"é,😀,".{method}({args})}}
fn consume(iterator:Iter<{item}>)->i32{{var length=0usize;for item in iterator{{print("item");length+=1usize;}}std::debug::assert(length=={length}usize && iterator.next()==None,"retained state");42}}
"#
        ));
        let entry = program.modules[program.root.index()]
            .functions
            .iter()
            .find(|f| f.name == "consume")
            .unwrap()
            .id;
        for encoded in [false, true] {
            let mut rt = runtime();
            rt.register_host_function(HostFunction::new(standard_log(), |context, _| {
                context.runtime().collect_garbage().unwrap();
                Ok(Value::Unit)
            }))
            .unwrap();
            let loaded = rt
                .load_program("retained-string-iterators", route(&program, encoded))
                .unwrap();
            let mut vm = Vm::new(rt);
            let value = vm.execute(&loaded, "new").unwrap().return_value;
            assert_eq!(vm.runtime().gc().active_roots(), 0);
            let roots = vm.runtime().gc().root_value(value.clone()).unwrap();
            vm.runtime().collect_garbage().unwrap();
            let session = vm
                .runtime()
                .begin_execution(&loaded, vm.runtime().execution_options())
                .unwrap();
            assert_eq!(
                Executor::new(vm.runtime(), &loaded, entry, &[value])
                    .unwrap()
                    .run()
                    .unwrap(),
                Value::I32(42),
                "{method}"
            );
            drop(session);
            drop(roots);
            clean(&vm);
        }
    }
}
