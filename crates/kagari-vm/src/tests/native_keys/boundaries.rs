use super::{cases::OPERATIONS, runtime};
use crate::{
    error::VmError, executor::Executor, reentry::reenter, tests::common::compile_test_bytecode,
    vm::Vm,
};

use kagari_bytecode::{artifact::KbcArtifact, program::BytecodeProgram};
use kagari_common::{cancellation::CancellationToken, host_interface::standard_log};
use kagari_runtime::{
    Runtime,
    error::RuntimeErrorKind,
    gc::{HeapObjectId, RootedValue},
    host::HostFunction,
    module::LoadedModule,
    value::Value,
};
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
pub(super) fn clean(runtime: &Runtime) {
    assert_eq!(runtime.gc().active_roots(), 0);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert!(!runtime.is_quarantined());
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc().allocated_objects(), 0);
}
fn set(mode: &str) -> bool {
    matches!(mode, "contains" | "set_insert" | "set_remove")
}
fn source(mode: &str, failure: &str) -> BytecodeProgram {
    let storage = if set(mode) {
        "LinkedHashSet<Key>"
    } else {
        "LinkedHashMap<Key,i32>"
    };
    let constructor = if set(mode) {
        "LinkedHashSet::new()"
    } else {
        "LinkedHashMap::new()"
    };
    let inserts = if set(mode) {
        "target.insert(key);"
    } else {
        "target.insert(key,id);"
    };
    let action = match mode {
        "get" => "bundle.target.get(bundle.query);",
        "contains_key" => "bundle.target.contains_key(bundle.query);",
        "map_insert" => "bundle.target.insert(bundle.query,42);",
        "map_remove" => "bundle.target.remove(bundle.query);",
        "contains" => "bundle.target.contains(bundle.query);",
        "set_insert" => "bundle.target.insert(bundle.query);",
        "set_remove" => "bundle.target.remove(bundle.query);",
        "get_or_insert_with" => {
            "bundle.target.get_or_insert_with(bundle.query,||{observe(bundle.query,2);42});"
        }
        "update" => {
            "bundle.target.update(bundle.query,|previous|{observe(bundle.query,2);previous.unwrap_or(0)+42});"
        }
        _ => panic!(),
    };
    let effect = match failure {
        "trap" => "if item.seen.len()==2usize{val x=2147483647;x+1;}",
        "clear" => "item.target.clear();",
        "remove" => {
            "val inert=Key{id:item.id,visits:0,armed:false,seen:item.seen,target:item.target};item.target.remove(inert);"
        }
        "replace" if set(mode) => {
            "val inert=Key{id:item.id,visits:0,armed:false,seen:item.seen,target:item.target};item.target.insert(inert);"
        }
        "replace" => {
            "val inert=Key{id:item.id,visits:0,armed:false,seen:item.seen,target:item.target};item.target.insert(inert,99);"
        }
        _ => "",
    };
    let guard = if failure == "guard" {
        "val guard=bundle.target.iter();"
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
struct Key{{val id:i32,var visits:i32,val armed:bool,val seen:ArrayList<i32>,val target:{storage}}}
struct Bundle{{val target:{storage},val query:Key,val seen:ArrayList<i32>}}
fn observe(item:Key,kind:i32){{std::debug::assert(item.target.len()==3usize,"callback reads");item.visits+=1;item.seen.push(kind);print("callback");{effect}}}
impl PartialEq for Key{{fn eq(self,other:Self)->bool{{if self.armed{{observe(self,1);}}self.id==other.id}}}}
impl Eq for Key{{}}
impl Hash for Key{{fn hash(self)->i64{{if self.armed{{observe(self,0);}}0i64}}}}
fn create()->Bundle{{val target:{storage} ={constructor};val seen:ArrayList<i32> =[];for id in 20..23{{val key=Key{{id:id,visits:0,armed:false,seen:seen,target:target}};{inserts}}}Bundle{{target:target,seen:seen,query:Key{{id:{query},visits:0,armed:true,seen:seen,target:target}}}}}}
fn main(bundle:Bundle){{{guard}{action}{tail}}}
fn inner()->i32{{val map:LinkedHashMap<i32,i32> =LinkedHashMap::new();map.insert(0,30);map.update(0,|old|old.unwrap_or(0)+12)}}
fn fail()->i32{{val map:LinkedHashMap<i32,i32> =LinkedHashMap::new();map.get_or_insert_with(0,||{{val x=2147483647;x+1}})}}
fn ready()->i32{{7}}
"#,
        query = if mode == "get_or_insert_with" { 42 } else { 22 }
    ))
}
fn callbacks(mode: &str) -> Vec<Value> {
    if matches!(mode, "get_or_insert_with" | "update") {
        vec![0, 1, 1, 1, 2, 0, 1, 1, 1]
    } else {
        vec![0, 1, 1, 1]
    }
    .into_iter()
    .map(Value::I32)
    .collect()
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
struct Fixture {
    bundle: Value,
    target: Value,
    query: Value,
    seen: HeapObjectId,
    original: Vec<Value>,
    _roots: RootedValue,
}
impl Fixture {
    fn new(runtime: &Runtime, loaded: &LoadedModule, program: &BytecodeProgram) -> Self {
        let bundle = Executor::new(
            runtime,
            loaded,
            program.modules[program.root.index()]
                .functions
                .iter()
                .find(|function| function.name == "create")
                .unwrap()
                .id,
            &[],
        )
        .unwrap()
        .run()
        .unwrap();
        let roots = runtime.gc().root_value(bundle.clone()).unwrap();
        let target = field(runtime, &bundle, "target");
        let query = field(runtime, &bundle, "query");
        let Value::Array(seen) = field(runtime, &bundle, "seen") else {
            panic!()
        };
        let original = snapshot(runtime, &target);
        Self {
            bundle,
            target,
            query,
            seen,
            original,
            _roots: roots,
        }
    }
    fn expected(&self, mode: &str) -> Vec<Value> {
        let mut items = self.original.clone();
        match mode {
            "map_remove" | "set_remove" => {
                items.pop();
            }
            "map_insert" | "update" => {
                let Value::Tuple(pair) = items.last_mut().unwrap() else {
                    panic!()
                };
                pair[1] = Value::I32(if mode == "update" { 64 } else { 42 });
            }
            "get_or_insert_with" => {
                items.push(Value::Tuple(vec![self.query.clone(), Value::I32(42)]))
            }
            _ => {}
        }
        items
    }
}
fn snapshot(runtime: &Runtime, target: &Value) -> Vec<Value> {
    match target {
        Value::Map(id) => runtime
            .gc()
            .map_snapshot(*id)
            .unwrap()
            .into_iter()
            .map(|(key, value)| Value::Tuple(vec![key, value]))
            .collect(),
        Value::Set(id) => runtime.gc().set_snapshot(*id).unwrap(),
        _ => panic!(),
    }
}
#[test]
fn key_callbacks_reenter_and_cancel_at_every_hash_equality_and_factory_occurrence() {
    for mode in OPERATIONS {
        let program = source(mode, "");
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
                let calls = Rc::new(RefCell::new(0usize));
                let sink = calls.clone();
                let mut rt = runtime();
                rt.register_host_function(HostFunction::new(
                    standard_log(),
                    move |context, args| {
                        assert_eq!(args, [Value::Str("callback".into())]);
                        *sink.borrow_mut() += 1;
                        if cancellation == *sink.borrow() {
                            cancel.cancel();
                        } else if cancellation == 0 {
                            let depth = context.runtime().resources().counters().current_call_depth;
                            let root = context.runtime().execution_root().unwrap();
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
                    .load_program("keys-reentry", route(&program, encoded))
                    .unwrap();
                let fixture = Fixture::new(&rt, &loaded, &program);
                let mut options = rt.execution_options();
                options.cancellation = token;
                let session = rt.begin_execution(&loaded, options).unwrap();
                let result = Executor::new(&rt, &loaded, entry, slice::from_ref(&fixture.bundle))
                    .unwrap()
                    .run();
                let count = if cancellation == 0 {
                    expected.len()
                } else {
                    cancellation
                };
                assert_eq!(
                    rt.gc().array_snapshot(fixture.seen).unwrap(),
                    expected[..count],
                    "{mode} {cancellation}"
                );
                assert_eq!(
                    field(&rt, &fixture.query, "visits"),
                    Value::I32(count as i32)
                );
                if cancellation == 0 {
                    assert_eq!(result.unwrap(), Value::Unit);
                    assert_eq!(snapshot(&rt, &fixture.target), fixture.expected(mode));
                } else {
                    let error = result.unwrap_err();
                    assert!(
                        matches!(error.cause(),VmError::RuntimeError(e) if e.kind()==RuntimeErrorKind::Cancelled)
                    );
                    assert_eq!(snapshot(&rt, &fixture.target), fixture.original);
                }
                drop(session);
                let mut vm = Vm::new(rt);
                assert_eq!(
                    vm.execute(&loaded, "ready").unwrap().return_value,
                    Value::I32(7)
                );
                drop(fixture);
                clean(vm.runtime());
            }
        }
    }
}
#[test]
fn key_traps_and_alias_writes_preserve_original_tokens_slots_and_completed_effects() {
    for mode in OPERATIONS {
        for failure in ["trap", "replace", "remove", "clear", "guard"] {
            if failure == "guard"
                && !matches!(*mode, "map_remove" | "set_remove" | "get_or_insert_with")
            {
                continue;
            }
            let program = source(mode, failure);
            for encoded in [false, true] {
                let mut rt = runtime();
                rt.register_host_function(HostFunction::new(standard_log(), |context, _| {
                    context.runtime().collect_garbage().unwrap();
                    Ok(Value::Unit)
                }))
                .unwrap();
                let loaded = rt
                    .load_program("keys-trap", route(&program, encoded))
                    .unwrap();
                let fixture = Fixture::new(&rt, &loaded, &program);
                let result = Executor::new(
                    &rt,
                    &loaded,
                    program.modules[program.root.index()]
                        .functions
                        .iter()
                        .find(|function| function.name == "main")
                        .unwrap()
                        .id,
                    slice::from_ref(&fixture.bundle),
                )
                .unwrap()
                .run();
                let error = result.unwrap_err();
                assert!(
                    matches!(
                        error.cause(),
                        VmError::RuntimeError(_) | VmError::BuiltinError(_)
                    ),
                    "{mode} {failure}: {error:?}"
                );
                assert_eq!(
                    snapshot(&rt, &fixture.target),
                    fixture.original,
                    "{mode} {failure}"
                );
                let count = if failure == "trap" {
                    2
                } else if failure == "guard" {
                    callbacks(mode).len()
                } else {
                    1
                };
                assert_eq!(
                    rt.gc().array_snapshot(fixture.seen).unwrap(),
                    callbacks(mode)[..count]
                );
                let mut vm = Vm::new(rt);
                assert_eq!(
                    vm.execute(&loaded, "ready").unwrap().return_value,
                    Value::I32(7)
                );
                drop(fixture);
                clean(vm.runtime());
            }
        }
    }
}
#[test]
fn every_key_lookup_and_map_callback_allocation_limit_commits_only_complete_results() {
    for mode in OPERATIONS {
        let program = source(mode, "");
        for encoded in [false, true] {
            let mut maximum = 0;
            for phase in 0..2 {
                let limits = if phase == 0 {
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
                        .load_program("keys-allocation", route(&program, encoded))
                        .unwrap();
                    let fixture = Fixture::new(&rt, &loaded, &program);
                    let mut options = rt.execution_options();
                    options.resources.max_allocation_units = limit;
                    let session = rt.begin_execution(&loaded, options).unwrap();
                    let result = Executor::new(
                        &rt,
                        &loaded,
                        program.modules[program.root.index()]
                            .functions
                            .iter()
                            .find(|function| function.name == "main")
                            .unwrap()
                            .id,
                        slice::from_ref(&fixture.bundle),
                    )
                    .unwrap()
                    .run();
                    if limit.is_none() {
                        maximum = session.counters().allocation_units;
                        assert!(maximum > 0);
                    }
                    if limit.is_none() || limit == Some(maximum) {
                        assert_eq!(result.unwrap(), Value::Unit);
                        assert_eq!(snapshot(&rt, &fixture.target), fixture.expected(mode));
                    } else {
                        let error = result.unwrap_err();
                        assert!(
                            matches!(error.cause(),VmError::RuntimeError(e) if e.kind()==RuntimeErrorKind::ResourceLimitExceeded)
                        );
                        assert_eq!(snapshot(&rt, &fixture.target), fixture.original);
                    }
                    drop(session);
                    drop(fixture);
                    clean(&rt);
                }
            }
        }
    }
}
