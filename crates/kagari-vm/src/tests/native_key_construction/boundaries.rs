use super::{cases::TYPES, runtime};
use crate::{Vm, VmError, executor::Executor, tests::common::compile_test_bytecode};
use kagari_bytecode::{BytecodeProgram, KbcArtifact};
use kagari_common::{cancellation::CancellationToken, host_interface::standard_log};
use kagari_runtime::{RuntimeErrorKind, host::HostFunction, value::Value};
use std::{cell::RefCell, rc::Rc, slice};
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
fn construction_callbacks_reenter_cancel_and_exhaust_every_allocation_limit() {
    for map in [false, true] {
        let item = if map { "(Key<i32>,i32)" } else { "Key<i32>" };
        let owner = if map {
            "LinkedHashMap"
        } else {
            "LinkedHashSet"
        };
        let storage = if map {
            "LinkedHashMap<Key<i32>,i32>"
        } else {
            "LinkedHashSet<Key<i32>>"
        };
        let elements = if map {
            "(a,20),(b,21),(same,42),(c,22)"
        } else {
            "a,b,same,c"
        };
        for method in ["from", "from_iter"] {
            for custom in [false, true] {
                if method == "from" && custom {
                    continue;
                }
                let source = if custom {
                    "Span{items:items}"
                } else {
                    "Proxy{items:items}"
                };
                let program = compile_test_bytecode(&format!(
                    r#"{TYPES}
fn setup()->ArrayList<{item}>{{val a=Key{{id:20,visits:0}};val b=Key{{id:21,visits:0}};val same=Key{{id:20,visits:0}};val c=Key{{id:22,visits:0}};[{elements}]}}
fn main(items:ArrayList<{item}>)->i32{{val output:{storage} ={owner}::{method}({source});std::debug::assert(output.len()==3usize,"duplicates");print("constructed");42}}
fn inner()->i32{{LinkedHashMap::from_iter([(7,42)].iter()).get(7).unwrap_or(0)}}
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
                                        let depth = context
                                            .runtime()
                                            .resources()
                                            .counters()
                                            .current_call_depth;
                                        assert_eq!(
                                            crate::reenter(context, &root, inner, &[])
                                                .unwrap()
                                                .value(),
                                            Value::I32(42)
                                        );
                                        assert!(crate::reenter(context, &root, fail, &[]).is_err());
                                        assert_eq!(
                                            context
                                                .runtime()
                                                .resources()
                                                .counters()
                                                .current_call_depth,
                                            depth
                                        );
                                    }
                                    context.runtime().collect_garbage().unwrap();
                                    Ok(Value::Unit)
                                },
                            ))
                            .unwrap();
                            let loaded = rt
                                .load_program(
                                    "key-construction-boundaries",
                                    route(&program, encoded),
                                )
                                .unwrap();
                            let mut vm = Vm::new(rt);
                            let fixture = vm.execute(&loaded, "setup").unwrap().return_value;
                            let Value::Array(array) = fixture else {
                                panic!()
                            };
                            let roots = vm.runtime().gc().root_value(fixture.clone()).unwrap();
                            let original = vm.runtime().gc().array_snapshot(array).unwrap();
                            let mut options = vm.runtime().execution_options();
                            options.cancellation = token;
                            options.resources.max_allocation_units = limit;
                            let session = vm.runtime().begin_execution(&loaded, options).unwrap();
                            let result = Executor::new(vm.runtime(), &loaded, entry, &[fixture])
                                .unwrap()
                                .run();
                            assert_eq!(vm.runtime().gc().array_snapshot(array).unwrap(), original);
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
    }
}
#[test]
fn construction_traps_preserve_key_effects_and_release_source_iteration() {
    for map in [false, true] {
        for failure in ["hash", "eq", "source"] {
            let item = if map { "(Key,i32)" } else { "Key" };
            let owner = if map {
                "LinkedHashMap"
            } else {
                "LinkedHashSet"
            };
            let storage = if map {
                "LinkedHashMap<Key,i32>"
            } else {
                "LinkedHashSet<Key>"
            };
            let elements = if map { "(a,20),(b,21)" } else { "a,b" };
            let hash = if failure == "hash" {
                "if self.id==21{val x=2147483647;x+1;}"
            } else {
                ""
            };
            let eq = if failure == "eq" {
                "val x=2147483647;x+1;"
            } else {
                ""
            };
            let mutate = if failure == "source" {
                "self.items.clear();"
            } else {
                ""
            };
            let program = compile_test_bytecode(&format!(
                r#"
struct Key{{val id:i32,var visits:i32}}
impl PartialEq for Key{{fn eq(self,other:Self)->bool{{self.visits+=1;print("eq");{eq}self.id==other.id}}}}
impl Eq for Key{{}}impl Hash for Key{{fn hash(self)->i64{{self.visits+=1;print("hash");{hash}0i64}}}}
struct Proxy{{val items:ArrayList<{item}>}}
impl Index<usize> for Proxy{{type Output={item};fn index(self,i:usize)->{item}{{self.items[i]}}}}
impl Iterable for Proxy{{type Item={item};type Iter=Iter<{item}>;fn iter(self)->Iter<{item}>{{self.items.iter().inspect(|value|{{print("next");{mutate}}})}}}}
impl List<{item}> for Proxy{{fn len(self)->usize{{self.items.len()}}fn is_empty(self)->bool{{self.items.is_empty()}}fn get(self,i:usize)->Option<{item}>{{self.items.get(i)}}}}
fn setup()->ArrayList<{item}>{{val a=Key{{id:20,visits:0}};val b=Key{{id:21,visits:0}};[{elements}]}}
fn main(items:ArrayList<{item}>){{val output:{storage} ={owner}::from(Proxy{{items:items}});}}
fn inspect(items:ArrayList<{item}>)->i32{{items.push(items[0usize]);{} }}
"#,
                if map {
                    "items[1usize][0].visits"
                } else {
                    "items[1usize].visits"
                }
            ));
            let entry = program.modules[program.root.index()]
                .functions
                .iter()
                .find(|f| f.name == "main")
                .unwrap()
                .id;
            let inspect = program.modules[program.root.index()]
                .functions
                .iter()
                .find(|f| f.name == "inspect")
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
                    .load_program("construction-traps", route(&program, encoded))
                    .unwrap();
                let mut vm = Vm::new(rt);
                let fixture = vm.execute(&loaded, "setup").unwrap().return_value;
                let Value::Array(array) = fixture else {
                    panic!()
                };
                let roots = vm.runtime().gc().root_value(fixture.clone()).unwrap();
                let original = vm.runtime().gc().array_snapshot(array).unwrap();
                let session = vm
                    .runtime()
                    .begin_execution(&loaded, vm.runtime().execution_options())
                    .unwrap();
                let error = Executor::new(vm.runtime(), &loaded, entry, slice::from_ref(&fixture))
                    .unwrap()
                    .run()
                    .unwrap_err();
                assert_eq!(vm.runtime().gc().array_snapshot(array).unwrap(), original);
                if failure == "source" {
                    assert!(
                        matches!(error.cause(),VmError::BuiltinError(cause) if cause.message().contains("during iteration"))
                    );
                } else {
                    assert!(
                        matches!(error.cause(),VmError::RuntimeError(cause) if cause.kind()==RuntimeErrorKind::ScriptTrap)
                    );
                }
                drop(session);
                let session = vm
                    .runtime()
                    .begin_execution(&loaded, vm.runtime().execution_options())
                    .unwrap();
                let seen = Executor::new(vm.runtime(), &loaded, inspect, &[fixture])
                    .unwrap()
                    .run()
                    .unwrap();
                assert_eq!(
                    seen,
                    Value::I32(if failure == "eq" {
                        2
                    } else if failure == "hash" {
                        1
                    } else {
                        0
                    })
                );
                drop(session);
                drop(roots);
                clean(&vm);
            }
        }
    }
}
