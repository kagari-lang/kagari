use super::{cases::TYPES, runtime};
use crate::{Vm, VmError, executor::Executor, tests::common::compile_test_bytecode};
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
fn array_snapshot_callbacks_reenter_and_cancel_at_each_occurrence() {
    for method in ["from", "from_iter", "copy", "extend"] {
        let source = if method == "from_iter" {
            "val source=Proxy{items:items};"
        } else {
            "val source:List<Cell> =Proxy{items:items};"
        };
        let action = match method {
            "from" => "val output:ArrayList<Cell> =ArrayList::from(source);",
            "from_iter" => "val output:ArrayList<Cell> =ArrayList::from_iter(source);",
            "copy" => "val output=[Cell{value:0},Cell{value:0}];output.copy_from(source);",
            _ => {
                "val output=[Cell{value:0}];val view:MutableList<Cell> =output;view.extend(source);"
            }
        };
        let prefix = usize::from(method == "extend");
        let program = compile_test_bytecode(&format!(
            r#"{TYPES}
fn main()->i32{{val items=[Cell{{value:20}},Cell{{value:22}}];{source}{action}
std::debug::assert(output[{prefix}usize].value+output[{}usize].value==42,"rooted values");
output[{prefix}usize].value=42;std::debug::assert(items[0usize].value==42,"shared slots");
print("snapshot");items.push(Cell{{value:0}});42}}
fn inner()->i32{{ArrayList::from_iter([7].iter())[0usize]}}
fn fail()->i32{{val x=2147483647;x+1}}fn ready()->i32{{7}}
"#,
            prefix + 1
        ));
        let root = &program.modules[program.root.index()];
        let inner = root
            .functions
            .iter()
            .find(|f| f.name == "inner")
            .unwrap()
            .id;
        let fail = root.functions.iter().find(|f| f.name == "fail").unwrap().id;
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
                    .load_program("array-copy-reentry", route(&program, encoded))
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
                clean(&vm);
            }
        }
    }
}

#[test]
fn snapshot_failures_preserve_completed_source_effects_without_partial_commit() {
    for method in ["copy", "extend"] {
        for failure in ["iter", "next", "length", "guard", "source_mutation"] {
            let setup = if failure == "guard" {
                "val guard=target.iter();"
            } else {
                ""
            };
            let iteration = if failure == "iter" {
                "val x=2147483647;x+1;"
            } else {
                ""
            };
            let callback = match failure {
                "next" => "if item==21{val x=2147483647;x+1;}",
                "length" => "if item==20{self.target.pop();}",
                "source_mutation" => "self.items.push(0);",
                _ => "",
            };
            let action = if method == "copy" {
                "target.copy_from(source);"
            } else {
                "target.extend(source);"
            };
            let tail = if failure == "guard" {
                "std::debug::assert(guard.next().unwrap_or(0)==20,\"live replacement\");"
            } else {
                ""
            };
            let program = compile_test_bytecode(&format!(
                r#"
struct Proxy{{val items:ArrayList<i32>,val target:ArrayList<i32>}}
impl Index<usize> for Proxy{{type Output=i32;fn index(self,i:usize)->i32{{self.items[i]}}}}
impl Iterable for Proxy{{type Item=i32;type Iter=Iter<i32>;fn iter(self)->Iter<i32>{{print("iter");{iteration}self.items.iter().inspect(|item|{{self.target[0usize]=42;print("next");{callback}}})}}}}
impl List<i32> for Proxy{{fn len(self)->usize{{self.items.len()}}fn is_empty(self)->bool{{self.items.is_empty()}}fn get(self,i:usize)->Option<i32>{{self.items.get(i)}}}}
fn main(target:ArrayList<i32>,items:ArrayList<i32>){{{setup}val source:List<i32> =Proxy{{items:items,target:target}};{action}{tail}}}
"#
            ));
            let entry = program.modules[program.root.index()]
                .functions
                .iter()
                .find(|f| f.name == "main")
                .unwrap()
                .id;
            for encoded in [false, true] {
                let effects = Rc::new(RefCell::new(Vec::new()));
                let sink = effects.clone();
                let mut rt = runtime();
                rt.register_host_function(HostFunction::new(standard_log(), move |_, args| {
                    let Value::Str(label) = &args[0] else {
                        panic!()
                    };
                    sink.borrow_mut().push(label.clone());
                    Ok(Value::Unit)
                }))
                .unwrap();
                let loaded = rt
                    .load_program("array-copy-failure", route(&program, encoded))
                    .unwrap();
                let target = rt.alloc_array(vec![Value::I32(9); 3]).unwrap();
                let items = rt
                    .alloc_array(vec![Value::I32(20), Value::I32(21), Value::I32(22)])
                    .unwrap();
                let roots = rt
                    .gc()
                    .root_value(Value::Tuple(vec![
                        Value::Array(target),
                        Value::Array(items),
                    ]))
                    .unwrap();
                let session = rt.begin_execution(&loaded, rt.execution_options()).unwrap();
                let result = Executor::new(
                    &rt,
                    &loaded,
                    entry,
                    &[Value::Array(target), Value::Array(items)],
                )
                .unwrap()
                .run();
                let expected = if failure == "guard" && method == "copy" {
                    vec![20, 21, 22]
                } else if failure == "iter" {
                    vec![9, 9, 9]
                } else if failure == "length" && method == "extend" {
                    vec![42, 9, 20, 21, 22]
                } else if failure == "length" {
                    vec![42, 9]
                } else {
                    vec![42, 9, 9]
                };
                assert_eq!(
                    rt.gc().array_snapshot(target).unwrap(),
                    expected.into_iter().map(Value::I32).collect::<Vec<_>>(),
                    "{method} {failure}"
                );
                if (failure == "length" && method == "extend")
                    || (failure == "guard" && method == "copy")
                {
                    assert_eq!(result.unwrap(), Value::Unit);
                } else {
                    let error = result.unwrap_err();
                    assert_eq!(
                        error.trace().unwrap().frames.last().unwrap().function_name,
                        "main"
                    );
                    if matches!(failure, "iter" | "next") {
                        let VmError::RuntimeError(cause) = error.cause() else {
                            panic!("{error:?}")
                        };
                        assert_eq!(cause.kind(), RuntimeErrorKind::ScriptTrap);
                    } else {
                        let VmError::BuiltinError(cause) = error.cause() else {
                            panic!("{error:?}")
                        };
                        assert!(
                            cause.message().contains(if failure == "length" {
                                "equal lengths"
                            } else {
                                "during iteration"
                            }),
                            "{error:?}"
                        );
                    }
                }
                let calls = match failure {
                    "iter" => 0,
                    "next" => 2,
                    "source_mutation" => 1,
                    _ => 3,
                };
                assert_eq!(
                    effects.borrow().as_slice(),
                    [vec!["iter".to_owned()], vec!["next".to_owned(); calls]].concat()
                );
                drop(session);
                drop(roots);
                clean(&Vm::new(rt));
            }
        }
    }
}

#[test]
fn array_snapshot_allocation_failures_leave_destination_uncommitted() {
    for method in ["copy_from", "extend"] {
        let program = compile_test_bytecode(&format!(
            "fn main(target:ArrayList<i32>,source:ArrayList<i32>){{target.{method}(source);}}"
        ));
        let entry = program.modules[program.root.index()]
            .functions
            .iter()
            .find(|f| f.name == "main")
            .unwrap()
            .id;
        for encoded in [false, true] {
            let mut maximum = 0;
            for phase in 0..2 {
                let limits: Vec<_> = if phase == 0 {
                    vec![None]
                } else {
                    (0..=maximum).map(Some).collect()
                };
                for limit in limits {
                    let mut rt = runtime();
                    let loaded = rt
                        .load_program("array-copy-allocation", route(&program, encoded))
                        .unwrap();
                    let target = rt.alloc_array(vec![Value::I32(9); 3]).unwrap();
                    let source = rt
                        .alloc_array(vec![Value::I32(20), Value::I32(21), Value::I32(22)])
                        .unwrap();
                    let roots = rt
                        .gc()
                        .root_value(Value::Tuple(vec![
                            Value::Array(target),
                            Value::Array(source),
                        ]))
                        .unwrap();
                    let mut options = rt.execution_options();
                    options.resources.max_allocation_units = limit;
                    let session = rt.begin_execution(&loaded, options).unwrap();
                    let result = Executor::new(
                        &rt,
                        &loaded,
                        entry,
                        &[Value::Array(target), Value::Array(source)],
                    )
                    .unwrap()
                    .run();
                    if limit.is_none() {
                        maximum = session.counters().allocation_units;
                        assert!(maximum > 3);
                    }
                    if limit.is_none() || limit == Some(maximum) {
                        assert_eq!(result.unwrap(), Value::Unit);
                        let mut expected = if method == "extend" {
                            vec![Value::I32(9); 3]
                        } else {
                            vec![]
                        };
                        expected.extend([Value::I32(20), Value::I32(21), Value::I32(22)]);
                        assert_eq!(rt.gc().array_snapshot(target).unwrap(), expected);
                        assert_eq!(session.counters().allocation_units, maximum);
                    } else {
                        let error = result.unwrap_err();
                        let VmError::RuntimeError(cause) = error.cause() else {
                            panic!("{error:?}")
                        };
                        assert_eq!(cause.kind(), RuntimeErrorKind::ResourceLimitExceeded);
                        assert_eq!(
                            rt.gc().array_snapshot(target).unwrap(),
                            vec![Value::I32(9); 3]
                        );
                    }
                    drop(session);
                    drop(roots);
                    clean(&Vm::new(rt));
                }
            }
        }
    }
}

#[test]
fn foreign_generic_array_sources_materialize_their_selected_methods() {
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
pub struct Cursor<T> {pub val items:ArrayList<T>,pub var index:usize}
impl<T> Iterator for Cursor<T> {type Item=T;fn next(self)->Option<T>{val item=self.items.get(self.index);self.index+=1usize;item}}
pub struct Span<T> {pub val items:ArrayList<T>}
impl<T> Iterable for Span<T> {type Item=T;type Iter=Cursor<T>;fn iter(self)->Cursor<T>{Cursor{items:self.items,index:0usize}}}

"#,
        ),
        (
            "root",
            r#"
use pkg::model::{Span,Cursor};
struct Cell {var value:i32}
enum Payload {Item(i32)}
fn gather<T,I:Iterable<Item=T>>(source:I)->ArrayList<T>{ArrayList::from_iter(source)}
fn main()->i32{
 val items=[Cell{value:20},Cell{value:22}];
 val source=Span{items:items};
 val dynamic:Iterable<Item=Cell,Iter=Cursor<Cell>> =source;
 val first:ArrayList<Cell> =gather(source);
 val second:ArrayList<Cell> =ArrayList::from_iter(dynamic);
 val copied:ArrayList<Cell> =ArrayList::from(first);
 val target=[Cell{value:0},Cell{value:0}];target.copy_from(second);
 val view:MutableList<Cell> =target;view.extend(copied);
 target[0usize].value=19;
 std::debug::assert(items[0usize].value==19 && target[2usize].value==19,"shared foreign values");
 val payloads:ArrayList<Payload> =gather(Span{items:[Payload::Item(1)]});
 val extra=match payloads[0usize] {Payload::Item(value)=>value};
 target[0usize].value+target[3usize].value+extra
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
        let mut runtime = runtime();
        let loaded = runtime
            .load_program("foreign-array-copy", route(&program, encoded))
            .unwrap();
        let mut vm = Vm::new(runtime);
        assert_eq!(
            vm.execute(&loaded, "main").unwrap().return_value,
            Value::I32(42)
        );
        clean(&vm);
    }
}
