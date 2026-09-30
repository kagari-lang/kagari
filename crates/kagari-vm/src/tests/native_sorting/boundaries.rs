use super::runtime;
use crate::{Vm, VmError, executor::Executor, tests::common::compile_test_bytecode};
use kagari_bytecode::{BytecodeProgram, KbcArtifact, StructId};
use kagari_common::{cancellation::CancellationToken, host_interface::standard_log};
use kagari_runtime::{
    LoadedModule, Runtime, RuntimeErrorKind,
    gc::{HeapObjectId, RootedValue},
    host::HostFunction,
    value::Value,
};
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
pub(super) fn clean(runtime: &Runtime) {
    assert_eq!(runtime.gc().active_roots(), 0);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert!(!runtime.is_quarantined());
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc().allocated_objects(), 0);
}
fn program(mode: &str, failure: &str) -> BytecodeProgram {
    let action = match mode {
        "sort" => "target.sort();",
        "sort_by" => "target.sort_by(|a,b|{observe(a);a.key.cmp(b.key)});",
        "sort_by_key" => "target.sort_by_key(|item|{observe(item);item.key});",
        _ => "target.dedup();",
    };
    let effect = match failure {
        "trap" => "if item.seen.len()==2usize{val x=2147483647;x+1;}",
        "replace" => "item.target[0usize]=item;",
        "push" => "item.target.push(item);",
        "pop" => "item.target.pop();",
        "clear" => "item.target.clear();",
        "recursive" => "item.target.sort();",
        _ => "",
    };
    let setup = if failure == "guard" {
        "val guard=target.iter();"
    } else {
        ""
    };
    let tail = if failure == "guard" {
        "guard.next();"
    } else {
        ""
    };
    compile_test_bytecode(&format!(
        r#"
struct Rank{{val key:i32,val tag:i32,var visits:i32,val seen:ArrayList<i32>,val target:ArrayList<Rank>}}
fn observe(item:Rank){{std::debug::assert(item.target.len()==3usize,"callback reads");item.visits+=1;item.seen.push(item.tag);print("callback");{effect}}}
impl PartialEq for Rank{{fn eq(self,other:Self)->bool{{observe(self);self.key==other.key}}}}
impl Eq for Rank{{}}
impl PartialOrd for Rank{{fn partial_cmp(self,other:Self)->Option<Ordering>{{self.key.partial_cmp(other.key)}}}}
impl Ord for Rank{{fn cmp(self,other:Self)->Ordering{{observe(self);self.key.cmp(other.key)}}}}
fn main(target:ArrayList<Rank>){{{setup}{action}{tail}}}
fn inner()->i32{{val items=[7,8];items.sort_by(|a,b|b.cmp(a));items.dedup();items[1usize]}}
fn fail()->i32{{val items=[7];items.sort_by_key(|item|{{val x=2147483647;x+1;item}});7}}
fn ready()->i32{{7}}
"#
    ))
}
fn callbacks(mode: &str) -> Vec<Value> {
    match mode {
        "sort_by_key" => vec![0, 1, 2],
        "dedup" => vec![0, 1],
        _ => vec![0, 1, 0],
    }
    .into_iter()
    .map(Value::I32)
    .collect()
}
struct Fixture {
    target: HeapObjectId,
    seen: HeapObjectId,
    original: Vec<Value>,
    _roots: RootedValue,
}
impl Fixture {
    fn new(runtime: &Runtime, loaded: &LoadedModule, program: &BytecodeProgram) -> Self {
        let target = runtime.alloc_array(vec![]).unwrap();
        let target_root = runtime.gc().root_value(Value::Array(target)).unwrap();
        let seen = runtime.alloc_array(vec![]).unwrap();
        let seen_root = runtime.gc().root_value(Value::Array(seen)).unwrap();
        let layout = loaded
            .struct_layout(StructId::new(
                program.modules[program.root.index()]
                    .structures
                    .iter()
                    .position(|layout| layout.name() == "Rank")
                    .unwrap(),
            ))
            .unwrap();
        let mut original = Vec::new();
        for (tag, key) in [2, 1, 1].into_iter().enumerate() {
            let item = Value::Struct(
                runtime
                    .alloc_struct(
                        layout.clone(),
                        vec![
                            Value::I32(key),
                            Value::I32(tag as i32),
                            Value::I32(0),
                            Value::Array(seen),
                            Value::Array(target),
                        ],
                    )
                    .unwrap(),
            );
            runtime.gc().array_push(target, item.clone()).unwrap();
            original.push(item);
        }
        let roots = runtime
            .gc()
            .root_value(Value::Tuple(vec![
                Value::Array(target),
                Value::Array(seen),
                Value::Tuple(original.clone()),
            ]))
            .unwrap();
        drop(target_root);
        drop(seen_root);
        Self {
            target,
            seen,
            original,
            _roots: roots,
        }
    }
    fn expected(&self, mode: &str) -> Vec<Value> {
        if mode == "dedup" {
            self.original[..2].to_vec()
        } else {
            [1, 2, 0]
                .into_iter()
                .map(|index| self.original[index].clone())
                .collect()
        }
    }
    fn visits(&self, runtime: &Runtime) -> i32 {
        self.original
            .iter()
            .map(|value| {
                let Value::Struct(id) = value else { panic!() };
                let fields = runtime.gc().struct_snapshot(*id).unwrap().1;
                let Value::I32(visits) = fields
                    .iter()
                    .find(|field| field.name == "visits")
                    .unwrap()
                    .value
                else {
                    panic!()
                };
                visits
            })
            .sum()
    }
}

#[test]
fn sorting_and_dedup_reenter_and_cancel_at_each_callback_without_partial_commit() {
    for mode in ["sort", "sort_by", "sort_by_key", "dedup"] {
        let program = program(mode, "");
        let root = &program.modules[program.root.index()];
        let entry = root.functions.iter().find(|f| f.name == "main").unwrap().id;
        let inner = root
            .functions
            .iter()
            .find(|f| f.name == "inner")
            .unwrap()
            .id;
        let fail = root.functions.iter().find(|f| f.name == "fail").unwrap().id;
        let expected = callbacks(mode);
        for encoded in [false, true] {
            for cancellation in 0..=expected.len() {
                let token = CancellationToken::default();
                let cancel = token.clone();
                let effects = Rc::new(RefCell::new(0usize));
                let sink = effects.clone();
                let mut rt = runtime();
                rt.register_host_function(HostFunction::new(
                    standard_log(),
                    move |context, args| {
                        assert_eq!(args, [Value::Str("callback".into())]);
                        *sink.borrow_mut() += 1;
                        let depth = context.runtime().resources().counters().current_call_depth;
                        if cancellation == *sink.borrow() {
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
                    .load_program("sorting-reentry", route(&program, encoded))
                    .unwrap();
                let fixture = Fixture::new(&rt, &loaded, &program);
                let mut options = rt.execution_options();
                options.cancellation = token;
                let session = rt.begin_execution(&loaded, options).unwrap();
                let result = Executor::new(&rt, &loaded, entry, &[Value::Array(fixture.target)])
                    .unwrap()
                    .run();
                let calls = if cancellation == 0 {
                    expected.len()
                } else {
                    cancellation
                };
                assert_eq!(
                    rt.gc().array_snapshot(fixture.seen).unwrap(),
                    expected[..calls],
                    "{mode} {cancellation}"
                );
                assert_eq!(fixture.visits(&rt), calls as i32);
                if cancellation == 0 {
                    assert_eq!(result.unwrap(), Value::Unit);
                    assert_eq!(
                        rt.gc().array_snapshot(fixture.target).unwrap(),
                        fixture.expected(mode)
                    );
                } else {
                    let error = result.unwrap_err();
                    let VmError::RuntimeError(cause) = error.cause() else {
                        panic!("{error:?}")
                    };
                    assert_eq!(cause.kind(), RuntimeErrorKind::Cancelled);
                    assert_eq!(
                        rt.gc().array_snapshot(fixture.target).unwrap(),
                        fixture.original
                    );
                }
                drop(session);
                drop(fixture);
                clean(&rt);
                let mut vm = Vm::new(rt);
                assert_eq!(
                    vm.execute(&loaded, "ready").unwrap().return_value,
                    Value::I32(7)
                );
                clean(vm.runtime());
            }
        }
    }
}

#[test]
fn failed_preparation_and_live_iteration_preserve_original_slots_and_completed_effects() {
    for mode in ["sort", "sort_by", "sort_by_key", "dedup"] {
        for failure in [
            "trap",
            "replace",
            "push",
            "pop",
            "clear",
            "recursive",
            "guard",
        ] {
            let program = program(mode, failure);
            let entry = program.modules[program.root.index()]
                .functions
                .iter()
                .find(|f| f.name == "main")
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
                    .load_program("sorting-failure", route(&program, encoded))
                    .unwrap();
                let fixture = Fixture::new(&rt, &loaded, &program);
                let session = rt.begin_execution(&loaded, rt.execution_options()).unwrap();
                let error = Executor::new(&rt, &loaded, entry, &[Value::Array(fixture.target)])
                    .unwrap()
                    .run()
                    .unwrap_err();
                assert_eq!(
                    rt.gc().array_snapshot(fixture.target).unwrap(),
                    fixture.original,
                    "{mode} {failure}"
                );
                assert_eq!(
                    error.trace().unwrap().frames.last().unwrap().function_name,
                    "main"
                );
                if matches!(failure, "trap" | "replace" | "recursive") {
                    let VmError::RuntimeError(cause) = error.cause() else {
                        panic!("{error:?}")
                    };
                    assert_eq!(cause.kind(), RuntimeErrorKind::ScriptTrap);
                } else {
                    let VmError::BuiltinError(cause) = error.cause() else {
                        panic!("{error:?}")
                    };
                    assert!(
                        cause.message().contains("during iteration")
                            || cause.message().contains("callback"),
                        "{error:?}"
                    );
                }
                let expected = callbacks(mode);
                let calls = match failure {
                    "trap" => 2,
                    "guard" => expected.len(),
                    _ => 1,
                };
                assert_eq!(
                    rt.gc().array_snapshot(fixture.seen).unwrap(),
                    expected[..calls]
                );
                assert_eq!(fixture.visits(&rt), calls as i32);
                drop(session);
                drop(fixture);
                clean(&rt);
            }
        }
    }
}

#[test]
fn every_prepared_array_allocation_limit_keeps_original_storage_until_commit() {
    for mode in ["sort", "sort_by", "sort_by_key", "dedup"] {
        let program = program(mode, "");
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
                    rt.register_host_function(HostFunction::new(standard_log(), |context, _| {
                        context.runtime().collect_garbage().unwrap();
                        Ok(Value::Unit)
                    }))
                    .unwrap();
                    let loaded = rt
                        .load_program("sorting-allocation", route(&program, encoded))
                        .unwrap();
                    let fixture = Fixture::new(&rt, &loaded, &program);
                    let mut options = rt.execution_options();
                    options.resources.max_allocation_units = limit;
                    let session = rt.begin_execution(&loaded, options).unwrap();
                    let result =
                        Executor::new(&rt, &loaded, entry, &[Value::Array(fixture.target)])
                            .unwrap()
                            .run();
                    if limit.is_none() {
                        maximum = session.counters().allocation_units;
                        assert!(maximum > 3);
                    }
                    if limit.is_none() || limit == Some(maximum) {
                        assert_eq!(result.unwrap(), Value::Unit);
                        assert_eq!(
                            rt.gc().array_snapshot(fixture.target).unwrap(),
                            fixture.expected(mode)
                        );
                        assert_eq!(session.counters().allocation_units, maximum);
                        assert_eq!(
                            rt.gc().array_snapshot(fixture.seen).unwrap(),
                            callbacks(mode)
                        );
                    } else {
                        let error = result.unwrap_err();
                        let VmError::RuntimeError(cause) = error.cause() else {
                            panic!("{error:?}")
                        };
                        assert_eq!(cause.kind(), RuntimeErrorKind::ResourceLimitExceeded);
                        assert_eq!(
                            rt.gc().array_snapshot(fixture.target).unwrap(),
                            fixture.original
                        );
                    }
                    drop(session);
                    drop(fixture);
                    clean(&rt);
                }
            }
        }
    }
}
