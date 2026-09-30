use super::{cases::TYPES, runtime};
use crate::{Vm, VmError, executor::Executor, tests::common::compile_test_bytecode};
use kagari_abi::types::PublicAbiItem;
use kagari_bytecode::{BytecodeProgram, KbcArtifact};
use kagari_common::{cancellation::CancellationToken, host_interface::standard_log};
use kagari_runtime::{RuntimeErrorKind, host::HostFunction, value::Value};
use std::{cell::RefCell, rc::Rc};
fn route(program: &BytecodeProgram, encoded: bool) -> BytecodeProgram {
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
fn clean(vm: &Vm) {
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
    assert!(!vm.runtime().is_quarantined());
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(vm.runtime().gc().allocated_objects(), 0);
}
#[test]
fn snapshot_callbacks_reenter_and_cancel_at_each_occurrence() {
    for method in ["keys", "values", "entries"] {
        let program = compile_test_bytecode(&format!(
            r#"{TYPES}
fn main()->i32 {{val items=[("a",[20]),("b",[22])];val source:Map<String,ArrayList<i32>> =Association{{items:items}};
val snapshot=source.{method}();print("snapshot");items.push(("c",[0]));42}}
fn inner()->i32 {{val map=LinkedHashMap::from([(1,7)]);map.values()[0usize]}}
fn fail()->i32 {{val x=2147483647;x+1}}fn ready()->i32{{7}}
"#
        ));
        let root = &program.modules[program.root.index()];
        let inner = root
            .functions
            .iter()
            .find(|function| function.name == "inner")
            .unwrap()
            .id;
        let fail = root
            .functions
            .iter()
            .find(|function| function.name == "fail")
            .unwrap()
            .id;
        let labels = ["iter", "next", "next", "snapshot"];
        for encoded in [false, true] {
            for cancellation in 0..=labels.len() {
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
                        let depth = context.runtime().resources().counters().current_call_depth;
                        if cancellation == sink.borrow().len() {
                            cancel.cancel();
                        } else if cancellation == 0 {
                            let root = context.runtime().execution_root().unwrap();
                            assert_eq!(
                                crate::reenter(context, &root, inner, &[]).unwrap().value(),
                                Value::I32(7)
                            );
                            assert!(crate::reenter(context, &root, fail, &[]).is_err());
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
                    .load_program("map-reentry", route(&program, encoded))
                    .unwrap();
                let mut options = rt.execution_options();
                options.cancellation = token;
                let session = rt.begin_execution(&loaded, options).unwrap();
                let mut vm = Vm::new(rt);
                let result = vm.execute(&loaded, "main");
                if cancellation == 0 {
                    assert_eq!(result.unwrap().return_value, Value::I32(42));
                    assert_eq!(*effects.borrow(), labels);
                } else {
                    assert!(
                        matches!(result,Err(VmError::RuntimeError(error)) if error.kind()==RuntimeErrorKind::Cancelled)
                    );
                    assert_eq!(*effects.borrow(), labels[..cancellation]);
                }
                drop(session);
                clean(&vm);
                assert_eq!(
                    vm.execute(&loaded, "ready").unwrap().return_value,
                    Value::I32(7)
                );
            }
        }
    }
}
#[test]
fn snapshot_traps_keep_selected_frames_and_release_source_guards() {
    for method in ["keys", "values", "entries"] {
        for failure in ["iter", "next", "mutation"] {
            let types = if failure == "mutation" {
                TYPES.replace("print(\"next\")", "{self.items.push(item);}")
            } else {
                TYPES.replace(
                    &format!("print(\"{failure}\")"),
                    "{val x=2147483647;val bad=x+1;}",
                )
            };
            let program = compile_test_bytecode(&format!(
                r#"{types}
fn main()->i32 {{val source:Map<i32,i32> =Association{{items:[(1,42)]}};val snapshot=source.{method}();0}}
fn ready()->i32{{7}}"#
            ));
            for encoded in [false, true] {
                let mut rt = runtime();
                rt.register_host_function(HostFunction::new(standard_log(), |_, _| {
                    Ok(Value::Unit)
                }))
                .unwrap();
                let loaded = rt
                    .load_program("map-trap", route(&program, encoded))
                    .unwrap();
                let mut vm = Vm::new(rt);
                let error = vm.execute(&loaded, "main").unwrap_err();
                assert_eq!(
                    error.trace().unwrap().frames.last().unwrap().function_name,
                    "main"
                );
                if failure == "mutation" {
                    let VmError::BuiltinError(cause) = error.cause() else {
                        panic!("{error:?}")
                    };
                    assert_eq!(
                        cause.message(),
                        "std::array::ArrayList::push: structural modification during iteration"
                    );
                } else {
                    let VmError::RuntimeError(cause) = error.cause() else {
                        panic!("{error:?}")
                    };
                    assert_eq!(cause.kind(), RuntimeErrorKind::ScriptTrap);
                    if failure == "iter" {
                        assert_eq!(error.trace().unwrap().frames.len(), 2);
                    }
                }
                clean(&vm);
                assert_eq!(
                    vm.execute(&loaded, "ready").unwrap().return_value,
                    Value::I32(7)
                );
            }
        }
    }
}
#[test]
fn direct_snapshot_allocation_failure_preserves_entry_charge_and_cleanup() {
    for method in ["keys", "values", "entries"] {
        let item = if method == "entries" {
            "(i32,i32)"
        } else {
            "i32"
        };
        let program = compile_test_bytecode(&format!(
            "fn main(source:LinkedHashMap<i32,i32>)->List<{item}> {{source.{method}()}}"
        ));
        let list_slots = program
            .modules
            .iter()
            .flat_map(|module| &module.public_items)
            .find_map(|item| match item {
                PublicAbiItem::Trait(trait_) if trait_.name == "List" => Some(trait_.methods.len()),
                _ => None,
            })
            .unwrap();
        // Two units for the one-element array, two for the List object, plus its method slots.
        let allocation_units = 4 + list_slots;
        let entry = program.modules[program.root.index()]
            .functions
            .iter()
            .find(|function| function.name == "main")
            .unwrap()
            .id;
        for encoded in [false, true] {
            for limit in 0..=allocation_units {
                let mut rt = runtime();
                let loaded = rt
                    .load_program("snapshot-allocation", route(&program, encoded))
                    .unwrap();
                let map = Value::Map(rt.alloc_map(vec![(Value::I32(1), Value::I32(42))]).unwrap());
                let mut options = rt.execution_options();
                options.resources.max_allocation_units = Some(limit);
                let session = rt.begin_execution(&loaded, options).unwrap();
                let result = Executor::new(&rt, &loaded, entry, &[map]).unwrap().run();
                if limit == allocation_units {
                    assert!(result.is_ok(), "{method} {encoded}: {result:?}");
                }
                match result {
                    Ok(value) => {
                        assert!(matches!(value, Value::Interface(_)));
                        assert_eq!(limit, allocation_units);
                        assert_eq!(session.counters().allocation_units, allocation_units);
                    }
                    Err(error) => {
                        let VmError::RuntimeError(cause) = error.cause() else {
                            panic!("{error:?}")
                        };
                        assert_eq!(
                            cause.kind(),
                            RuntimeErrorKind::ResourceLimitExceeded,
                            "{method} {encoded} {limit}: {error:?}"
                        );
                        assert_eq!(error.trace().unwrap().frames.len(), 1);
                        assert_eq!(error.trace().unwrap().frames[0].function_name, "main");
                        if limit == 0 {
                            // The old LoadLocal and storage snapshot entry, with no later interface charge.
                            assert_eq!(session.counters().instruction_steps, 2);
                        }
                    }
                }
                drop(session);
                clean(&Vm::new(rt));
            }
        }
    }
}

#[test]
fn foreign_map_snapshots_use_selected_methods_and_caller_result_tables() {
    use kagari_common::{
        identity::{ModuleIdentity, PackageId},
        source_database::{SourceDatabase, SourceLayer},
    };
    use kagari_compiler::{
        bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir,
    };
    use kagari_hir::analysis::AnalysisDatabase;
    let mut sources = SourceDatabase::default();
    let mut root = None;
    for (name, text) in [
        (
            "model",
            r#"
pub struct Association<K,V> {pub val items:ArrayList<(K,V)>}
impl<K,V> Iterable for Association<K,V> {type Item=(K,V);type Iter=Iter<(K,V)>;fn iter(self)->Iter<(K,V)>{self.items.iter()}}
impl<K,V> Map<K,V> for Association<K,V> {fn len(self)->usize{self.items.len()}fn is_empty(self)->bool{self.items.is_empty()}fn contains_key(self,key:K)->bool{true}fn get(self,key:K)->Option<V>{self.items.get(0usize).map(|pair|pair[1])}}

"#,
        ),
        (
            "root",
            r#"
use pkg::model::Association;
fn snapshot<M:Map<f64,ArrayList<i32>>>(source:M)->List<ArrayList<i32>>{source.values()}
fn main()->i32{
val source=Association{items:[(1.0,[20]),(2.0,[22])]};val dynamic:Map<f64,ArrayList<i32>> =source;
std::debug::assert_eq(dynamic.keys()[1usize],2.0,"foreign keys");
std::debug::assert_eq(dynamic.entries()[0usize][1][0usize],20,"foreign entries");
val values=snapshot(source);values[0usize][0usize]+dynamic.values()[1usize][0usize]}

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
        let mut runtime = runtime();
        let loaded = runtime
            .load_program("foreign-list", route(&program, encoded))
            .unwrap();
        let mut vm = Vm::new(runtime);
        assert_eq!(
            vm.execute(&loaded, "main").unwrap().return_value,
            Value::I32(42)
        );
        clean(&vm);
    }
}
