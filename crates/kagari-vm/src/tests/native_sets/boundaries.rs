use super::{
    cases::{OPERATIONS, TYPES},
    runtime,
};
use crate::{Vm, VmError, executor::Executor, tests::common::compile_test_bytecode};
use kagari_bytecode::{BytecodeProgram, KbcArtifact};
use kagari_common::{cancellation::CancellationToken, host_interface::standard_log};
use kagari_runtime::{Runtime, RuntimeErrorKind, host::HostFunction, value::Value};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    slice,
};
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
pub(super) fn clean(runtime: &Runtime) {
    assert_eq!(runtime.gc().active_roots(), 0);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert!(!runtime.is_quarantined());
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc().allocated_objects(), 0);
}
fn field(runtime: &Runtime, value: &Value, name: &str) -> Value {
    let Value::Struct(id) = value else { panic!() };
    runtime
        .gc()
        .struct_snapshot(*id)
        .unwrap()
        .1
        .into_iter()
        .find(|field| field.name == name)
        .unwrap()
        .value
}
fn snapshot(runtime: &Runtime, value: &Value) -> Vec<Value> {
    match value {
        Value::Array(id) => runtime.gc().array_snapshot(*id).unwrap(),
        Value::Set(id) => runtime.gc().set_snapshot(*id).unwrap(),
        _ => panic!(),
    }
}
#[test]
fn set_callbacks_reenter_cancel_and_exhaust_every_allocation_limit() {
    for operation in OPERATIONS {
        for mode in ["native", "proxy", "dynamic", "same"] {
            let result_type = if operation.starts_with("is_") {
                "bool"
            } else {
                "LinkedHashSet<Key>"
            };
            let expected = match *operation {
                "union" => 3,
                "intersection" => 1,
                "difference" => 1,
                _ => 2,
            };
            let expected = if mode == "same" {
                match *operation {
                    "union" | "intersection" => 2,
                    _ => 0,
                }
            } else {
                expected
            };
            let setup = if mode == "native" || mode == "same" {
                "val left=fixture.left;"
            } else {
                "val left=Policy{items:fixture.left_items,side:\"l\"};"
            };
            let other = if mode == "same" {
                "left"
            } else {
                "fixture.right"
            };
            let call = if mode == "dynamic" {
                format!(
                    "val view:Set<Key> =left;val output:{result_type} =view.{operation}(right);"
                )
            } else {
                format!("val output:{result_type} =left.{operation}(right);")
            };
            let check = if operation.starts_with("is_") {
                format!(
                    "std::debug::assert(output=={},\"relation\");",
                    if mode == "same" {
                        *operation != "is_disjoint"
                    } else {
                        false
                    }
                )
            } else {
                format!("std::debug::assert(output.len()=={expected}usize,\"elements\");")
            };
            let program = compile_test_bytecode(&format!(
                r#"{TYPES}
struct Fixture{{val left:LinkedHashSet<Key>,val right:LinkedHashSet<Key>,val left_items:ArrayList<Key>,val right_items:ArrayList<Key>}}
fn setup()->Fixture{{val a=Key{{id:20,visits:0}};val b=Key{{id:21,visits:0}};val c=Key{{id:22,visits:0}};val l=[a,b];val r=[b,c];Fixture{{left:LinkedHashSet::from(l),right:LinkedHashSet::from(r),left_items:l,right_items:r}}}}
fn main(fixture:Fixture)->i32{{{setup}val right:Set<Key> ={other};{call}{check}print("result");42}}
fn inner()->i32{{val a:LinkedHashSet<i32> =LinkedHashSet::from([1,2]);val b:Set<i32> =LinkedHashSet::from([2,3]);std::debug::assert(a.union(b).len()==3usize,"nested set");42}}
fn fail()->i32{{val x=2147483647;x+1}}fn ready()->i32{{7}}
"#
            ));
            let module = &program.modules[program.root.index()];
            let entry = module
                .functions
                .iter()
                .find(|f| f.name == "main")
                .unwrap()
                .id;
            let inner = module
                .functions
                .iter()
                .find(|f| f.name == "inner")
                .unwrap()
                .id;
            let fail = module
                .functions
                .iter()
                .find(|f| f.name == "fail")
                .unwrap()
                .id;
            for encoded in [false, true] {
                let mut labels = Vec::new();
                let mut maximum = 0;
                for phase in 0..4 {
                    let scenarios: Vec<_> = match phase {
                        0 | 3 => vec![(0, None)],
                        1 => (1..=labels.len()).map(|i| (i, None)).collect(),
                        _ => (0..=maximum).map(|i| (0, Some(i))).collect(),
                    };
                    for (cancellation, limit) in scenarios {
                        let effects = Rc::new(RefCell::new(Vec::new()));
                        let sink = effects.clone();
                        let active = Rc::new(Cell::new(false));
                        let flag = active.clone();
                        let token = CancellationToken::default();
                        let cancel = token.clone();
                        let mut rt = runtime();
                        rt.register_host_function(HostFunction::new(
                            standard_log(),
                            move |context, args| {
                                context.runtime().collect_garbage().unwrap();
                                if !flag.get() {
                                    return Ok(Value::Unit);
                                }
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
                                        crate::reenter(context, &root, inner, &[]).unwrap().value(),
                                        Value::I32(42)
                                    );
                                    assert!(crate::reenter(context, &root, fail, &[]).is_err());
                                    assert_eq!(
                                        context.runtime().resources().counters().current_call_depth,
                                        depth
                                    );
                                }
                                Ok(Value::Unit)
                            },
                        ))
                        .unwrap();
                        let loaded = rt
                            .load_program("sets-boundaries", route(&program, encoded))
                            .unwrap();
                        let mut vm = Vm::new(rt);
                        let fixture = vm.execute(&loaded, "setup").unwrap().return_value;
                        let roots = vm.runtime().gc().root_value(fixture.clone()).unwrap();
                        let originals: Vec<_> = ["left", "right", "left_items", "right_items"]
                            .into_iter()
                            .map(|name| {
                                let value = field(vm.runtime(), &fixture, name);
                                let original = snapshot(vm.runtime(), &value);
                                (value, original)
                            })
                            .collect();
                        active.set(true);
                        let mut options = vm.runtime().execution_options();
                        options.cancellation = token;
                        options.resources.max_allocation_units = limit;
                        let session = vm.runtime().begin_execution(&loaded, options).unwrap();
                        let result =
                            Executor::new(vm.runtime(), &loaded, entry, slice::from_ref(&fixture))
                                .unwrap()
                                .run();
                        for (value, original) in &originals {
                            assert_eq!(
                                snapshot(vm.runtime(), value),
                                *original,
                                "{operation} {mode}"
                            );
                        }
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
                            assert_eq!(result.unwrap(), Value::I32(42), "{operation} {mode}");
                            assert_eq!(*effects.borrow(), labels);
                        }
                        drop(session);
                        assert_eq!(
                            vm.execute(&loaded, "ready").unwrap().return_value,
                            Value::I32(7)
                        );
                        drop(roots);
                        clean(vm.runtime());
                    }
                }
            }
        }
    }
}
#[test]
fn set_source_alias_writes_and_traps_keep_completed_effects_and_release_dual_guards() {
    for operation in OPERATIONS {
        for failure in [
            "left_iter",
            "right_iter",
            "next",
            "contains",
            "alias_left",
            "alias_right",
        ] {
            if *operation == "union" && failure == "contains" {
                continue;
            }
            let result = if operation.starts_with("is_") {
                "bool"
            } else {
                "LinkedHashSet<i32>"
            };
            let mutation = match failure {
                "alias_left" => "self.left.clear();",
                "alias_right" => "self.right.clear();",
                _ => "",
            };
            let iter = if failure == "left_iter" {
                "if self.left_side{val x=2147483647;x+1;}"
            } else if failure == "right_iter" {
                "if !self.left_side{self.left.clear();}"
            } else {
                ""
            };
            let next = if failure == "next" {
                "val x=2147483647;x+1;"
            } else {
                ""
            };
            let membership = if failure == "contains" {
                "val x=2147483647;x+1;"
            } else {
                ""
            };
            let program = compile_test_bytecode(&format!(
                r#"
struct Fixture{{val left:ArrayList<i32>,val right:ArrayList<i32>,val effects:ArrayList<i32>}}
struct Policy{{val items:ArrayList<i32>,val left:ArrayList<i32>,val right:ArrayList<i32>,val effects:ArrayList<i32>,val left_side:bool}}
impl Iterable for Policy{{type Item=i32;type Iter=Iter<i32>;fn iter(self)->Iter<i32>{{self.effects.push(0);print("iter");{iter}self.items.iter().inspect(|item|{{self.effects.push(1);print("next");{next}{mutation}}})}}}}
impl Set<i32> for Policy{{fn len(self)->usize{{self.items.len()}}fn is_empty(self)->bool{{self.items.is_empty()}}fn contains(self,value:i32)->bool{{self.effects.push(2);print("contains");{membership}self.items.contains(value)}}}}
fn setup()->Fixture{{Fixture{{left:[20,21],right:[21,22],effects:[]}}}}
fn main(fixture:Fixture){{val left=Policy{{items:fixture.left,left:fixture.left,right:fixture.right,effects:fixture.effects,left_side:true}};val right:Set<i32> =Policy{{items:fixture.right,left:fixture.left,right:fixture.right,effects:fixture.effects,left_side:false}};val output:{result} =left.{operation}(right);}}
fn ready(fixture:Fixture)->i32{{fixture.left.push(42);fixture.right.push(42);fixture.effects.len() as i32}}
"#
            ));
            let module = &program.modules[program.root.index()];
            let entry = module
                .functions
                .iter()
                .find(|f| f.name == "main")
                .unwrap()
                .id;
            let ready = module
                .functions
                .iter()
                .find(|f| f.name == "ready")
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
                    .load_program("set-traps", route(&program, encoded))
                    .unwrap();
                let mut vm = Vm::new(rt);
                let fixture = vm.execute(&loaded, "setup").unwrap().return_value;
                let roots = vm.runtime().gc().root_value(fixture.clone()).unwrap();
                let session = vm
                    .runtime()
                    .begin_execution(&loaded, vm.runtime().execution_options())
                    .unwrap();
                let error = Executor::new(vm.runtime(), &loaded, entry, slice::from_ref(&fixture))
                    .unwrap()
                    .run()
                    .unwrap_err();
                let trap = matches!(failure, "left_iter" | "next" | "contains");
                if trap {
                    assert!(
                        matches!(error.cause(),VmError::RuntimeError(cause) if cause.kind()==RuntimeErrorKind::ScriptTrap)
                    );
                } else {
                    assert!(
                        matches!(error.cause(),VmError::BuiltinError(cause) if cause.message().contains("during iteration"))
                    );
                }
                assert_eq!(
                    snapshot(vm.runtime(), &field(vm.runtime(), &fixture, "left")),
                    vec![Value::I32(20), Value::I32(21)]
                );
                assert_eq!(
                    snapshot(vm.runtime(), &field(vm.runtime(), &fixture, "right")),
                    vec![Value::I32(21), Value::I32(22)]
                );
                let effects = snapshot(vm.runtime(), &field(vm.runtime(), &fixture, "effects"));
                assert!(!effects.is_empty());
                drop(session);
                let session = vm
                    .runtime()
                    .begin_execution(&loaded, vm.runtime().execution_options())
                    .unwrap();
                assert_eq!(
                    Executor::new(vm.runtime(), &loaded, ready, &[fixture])
                        .unwrap()
                        .run()
                        .unwrap(),
                    Value::I32(effects.len() as i32)
                );
                drop(session);
                drop(roots);
                clean(vm.runtime());
            }
        }
    }
}
