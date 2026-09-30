use super::runtime;
use crate::{Vm, VmError, executor::Executor, tests::common::compile_test_bytecode};
use kagari_bytecode::{BytecodeProgram, KbcArtifact};
use kagari_common::{
    cancellation::CancellationToken,
    host_interface::standard_log,
    identity::{ModuleIdentity, PackageId},
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{Runtime, RuntimeErrorKind, host::HostFunction, value::Value};
use std::{cell::RefCell, rc::Rc, slice};

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
fn clean(runtime: &Runtime) {
    assert_eq!(runtime.gc().active_roots(), 0);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert!(!runtime.is_quarantined());
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc().allocated_objects(), 0);
}
fn storage(kind: &str) -> &str {
    match kind {
        "array" => "ArrayList<i32>",
        "map" => "LinkedHashMap<i32,i32>",
        _ => "LinkedHashSet<i32>",
    }
}
fn parameters(kind: &str) -> &str {
    if kind == "map" { "key,value" } else { "key" }
}
fn target(runtime: &Runtime, kind: &str) -> Value {
    match kind {
        "array" => Value::Array(
            runtime
                .alloc_array(vec![Value::I32(20), Value::I32(21), Value::I32(22)])
                .unwrap(),
        ),
        "map" => Value::Map(
            runtime
                .alloc_map(
                    (20..23)
                        .map(|key| (Value::I32(key), Value::I32(key + 80)))
                        .collect(),
                )
                .unwrap(),
        ),
        _ => Value::Set(
            runtime
                .alloc_set((20..23).map(Value::I32).collect())
                .unwrap(),
        ),
    }
}
fn snapshot(runtime: &Runtime, value: &Value) -> Vec<Value> {
    match value {
        Value::Array(id) => runtime.gc().array_snapshot(*id).unwrap(),
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
fn retention_reentry_and_each_predicate_cancellation_preserve_atomic_commit() {
    for kind in ["array", "map", "set"] {
        let program = compile_test_bytecode(&format!(
            r#"
fn main(target:{},seen:ArrayList<i32>){{target.retain(|{}|{{seen.push(key);print("predicate");key%2==0}});print("committed");}}
fn inner()->i32{{val items=[7,8];items.retain(|key|key==7);items[0usize]}}
fn fail()->i32{{val x=2147483647;x+1}}fn ready()->i32{{7}}
"#,
            storage(kind),
            parameters(kind)
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
            for cancellation in 0..=3 {
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
                    .load_program("retain-reentry", route(&program, encoded))
                    .unwrap();
                let target = target(&rt, kind);
                let original = snapshot(&rt, &target);
                let target_root = rt.gc().root_value(target.clone()).unwrap();
                let seen = rt.alloc_array(vec![]).unwrap();
                let seen_root = rt.gc().root_value(Value::Array(seen)).unwrap();
                let mut options = rt.execution_options();
                options.cancellation = token;
                let session = rt.begin_execution(&loaded, options).unwrap();
                let result =
                    Executor::new(&rt, &loaded, entry, &[target.clone(), Value::Array(seen)])
                        .unwrap()
                        .run();
                if cancellation == 0 {
                    assert_eq!(result.unwrap(), Value::Unit);
                    assert_eq!(
                        snapshot(&rt, &target),
                        vec![original[0].clone(), original[2].clone()]
                    );
                    assert_eq!(
                        *effects.borrow(),
                        ["predicate", "predicate", "predicate", "committed"]
                    );
                } else {
                    let error = result.unwrap_err();
                    let VmError::RuntimeError(cause) = error.cause() else {
                        panic!("{error:?}")
                    };
                    assert_eq!(cause.kind(), RuntimeErrorKind::Cancelled);
                    assert_eq!(snapshot(&rt, &target), original);
                    assert_eq!(*effects.borrow(), vec!["predicate"; cancellation]);
                }
                assert_eq!(
                    rt.gc().array_snapshot(seen).unwrap(),
                    (20..20
                        + if cancellation == 0 {
                            3
                        } else {
                            cancellation as i32
                        })
                        .map(Value::I32)
                        .collect::<Vec<_>>()
                );
                drop(session);
                drop(target_root);
                drop(seen_root);
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
fn retention_allocation_failures_leave_original_storage() {
    for kind in ["array", "map", "set"] {
        let program = compile_test_bytecode(&format!(
            "fn main(target:{}){{target.retain(|{}|key%2==0);}}",
            storage(kind),
            parameters(kind)
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
                        .load_program("retain-allocation", route(&program, encoded))
                        .unwrap();
                    let target = target(&rt, kind);
                    let original = snapshot(&rt, &target);
                    let roots = rt.gc().root_value(target.clone()).unwrap();
                    let mut options = rt.execution_options();
                    options.resources.max_allocation_units = limit;
                    let session = rt.begin_execution(&loaded, options).unwrap();
                    let result = Executor::new(&rt, &loaded, entry, slice::from_ref(&target))
                        .unwrap()
                        .run();
                    if limit.is_none() {
                        maximum = session.counters().allocation_units;
                        assert!(maximum > 3);
                    }
                    if limit.is_none() || limit == Some(maximum) {
                        assert_eq!(result.unwrap(), Value::Unit);
                        assert_eq!(
                            snapshot(&rt, &target),
                            vec![original[0].clone(), original[2].clone()]
                        );
                        assert_eq!(session.counters().allocation_units, maximum);
                    } else {
                        let error = result.unwrap_err();
                        let VmError::RuntimeError(cause) = error.cause() else {
                            panic!("{error:?}")
                        };
                        assert_eq!(cause.kind(), RuntimeErrorKind::ResourceLimitExceeded);
                        assert_eq!(snapshot(&rt, &target), original);
                    }
                    drop(session);
                    drop(roots);
                    clean(&rt);
                }
            }
        }
    }
}

#[test]
fn retention_traps_alias_writes_and_live_iteration_never_commit_a_prefix() {
    for kind in ["array", "map", "set"] {
        for failure in [
            "trap",
            "insert",
            "replace",
            "remove",
            "clear",
            "recursive",
            "guard",
        ] {
            let action = match failure {
                "trap" => "if key==21{val x=2147483647;x+1;}".to_owned(),
                "insert" => match kind {
                    "array" => "target.push(0);",
                    "map" => "target.insert(0,0);",
                    _ => "target.insert(0);",
                }
                .into(),
                "replace" => match kind {
                    "array" => "target[0usize]=42;",
                    "map" => "target.insert(20,42);",
                    _ => "target.insert(20);",
                }
                .into(),
                "remove" => if kind == "array" {
                    "target.pop();"
                } else {
                    "target.remove(20);"
                }
                .into(),
                "clear" => "target.clear();".into(),
                "recursive" => format!("target.retain(|{}|true);", parameters(kind)),
                _ => String::new(),
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
            let program = compile_test_bytecode(&format!(
                "fn main(target:{},seen:ArrayList<i32>){{{setup}target.retain(|{}|{{seen.push(key);{action}false}});{tail}}}",
                storage(kind),
                parameters(kind)
            ));
            let entry = program.modules[program.root.index()]
                .functions
                .iter()
                .find(|f| f.name == "main")
                .unwrap()
                .id;
            for encoded in [false, true] {
                let mut rt = runtime();
                let loaded = rt
                    .load_program("retain-failure", route(&program, encoded))
                    .unwrap();
                let target = target(&rt, kind);
                let original = snapshot(&rt, &target);
                let target_root = rt.gc().root_value(target.clone()).unwrap();
                let seen = rt.alloc_array(vec![]).unwrap();
                let seen_root = rt.gc().root_value(Value::Array(seen)).unwrap();
                let session = rt.begin_execution(&loaded, rt.execution_options()).unwrap();
                let error =
                    Executor::new(&rt, &loaded, entry, &[target.clone(), Value::Array(seen)])
                        .unwrap()
                        .run()
                        .unwrap_err();
                assert_eq!(snapshot(&rt, &target), original, "{kind} {failure}");
                assert_eq!(
                    error.trace().unwrap().frames.last().unwrap().function_name,
                    "main"
                );
                if matches!(failure, "trap" | "recursive")
                    || (kind == "array" && failure == "replace")
                {
                    let VmError::RuntimeError(cause) = error.cause() else {
                        panic!("{kind} {failure} {error:?}")
                    };
                    assert_eq!(cause.kind(), RuntimeErrorKind::ScriptTrap);
                } else {
                    let VmError::BuiltinError(cause) = error.cause() else {
                        panic!("{kind} {failure} {error:?}")
                    };
                    assert!(
                        cause.message().contains("during iteration")
                            || cause.message().contains("callback"),
                        "{error:?}"
                    );
                }
                let calls = match failure {
                    "trap" => 2,
                    "guard" => 3,
                    _ => 1,
                };
                assert_eq!(
                    rt.gc().array_snapshot(seen).unwrap(),
                    (20..20 + calls).map(Value::I32).collect::<Vec<_>>(),
                    "{kind} {failure}"
                );
                drop(session);
                drop(target_root);
                drop(seen_root);
                clean(&rt);
            }
        }
    }
}

#[test]
fn tuple_elements_keep_one_predicate_argument_and_shared_payloads() {
    let program = compile_test_bytecode(
        r#"
struct Cell{var value:i32}
fn keep<T>(items:ArrayList<T>,p:fn(T)->bool){items.retain(p);}
fn main()->i32{
 val first=Cell{value:20};val second=Cell{value:21};val capture=[Cell{value:0}];
 val a=[(first,[first]),(second,[second])];
 keep(a,|item|{capture[0usize].value+=1;item[0].value+=1;item[1].push(item[0]);print("predicate");item[0].value==21});
 std::debug::assert(a.len()==1usize && a[0usize][1].len()==2usize,"one tuple argument");
 std::debug::assert(first.value==21 && second.value==22 && capture[0usize].value==2,"shared payload effects");
 first.value+a[0usize][0].value
}

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
            .load_program("retain-tuple", route(&program, encoded))
            .unwrap();
        let mut vm = Vm::new(rt);
        assert_eq!(
            vm.execute(&loaded, "main").unwrap().return_value,
            Value::I32(42)
        );
        clean(vm.runtime());
    }
}

#[test]
fn foreign_generic_predicates_use_selected_private_implementations() {
    let mut sources = SourceDatabase::default();
    let mut root = None;
    for (name, text) in [
        (
            "model",
            r#"
pub trait Keep {fn keep(self)->bool;}
pub trait Filter {fn apply(self);}
pub struct ArrayFilter<T>{pub val items:ArrayList<T>,pub val predicate:fn(T)->bool}
impl<T> Filter for ArrayFilter<T>{fn apply(self){self.items.retain(|item|(self.predicate)(item));}}
pub struct MapFilter<K:Eq+Hash,V>{pub val items:LinkedHashMap<K,V>,pub val predicate:fn(K,V)->bool}
impl<K:Eq+Hash,V> Filter for MapFilter<K,V>{fn apply(self){self.items.retain(|key,value|(self.predicate)(key,value));}}
pub struct SetFilter<T:Eq+Hash>{pub val items:LinkedHashSet<T>,pub val predicate:fn(T)->bool}
impl<T:Eq+Hash> Filter for SetFilter<T>{fn apply(self){self.items.retain(|item|(self.predicate)(item));}}
"#,
        ),
        (
            "root",
            r#"
use pkg::model::{Keep,Filter,ArrayFilter,MapFilter,SetFilter};
struct Cell{var value:i32}
impl Keep for Cell{fn keep(self)->bool{print("predicate");self.value%2==0}}
struct Key{val id:i32,var tag:i32}
impl PartialEq for Key{fn eq(self,other:Self)->bool{self.id==other.id}}
impl Eq for Key{}impl Hash for Key{fn hash(self)->i64{0i64}}
impl Keep for Key{fn keep(self)->bool{print("predicate");self.tag+=1;self.id%2==0}}
fn main()->i32{
 val items=[Cell{value:20},Cell{value:21},Cell{value:22}];val array:ArrayFilter<Cell> =ArrayFilter{items:items,predicate:|item|item.keep()};array.apply();
 val keys=[Key{id:20,tag:0},Key{id:21,tag:0},Key{id:22,tag:0}];
 val map=LinkedHashMap::from([(keys[0usize],items[0usize]),(keys[1usize],items[0usize]),(keys[2usize],items[1usize])]);val mapping:MapFilter<Key,Cell> =MapFilter{items:map,predicate:|key,value|key.keep()};mapping.apply();
 val set=LinkedHashSet::from(keys);val setting:SetFilter<Key> =SetFilter{items:set,predicate:|item|item.keep()};setting.apply();
 std::debug::assert(map.len()==2usize && set.len()==2usize,"foreign retention");
 std::debug::assert(keys[0usize].tag==2 && keys[1usize].tag==2 && keys[2usize].tag==2,"selected private callbacks");
 items[0usize].value+items[1usize].value
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
        let mut rt = runtime();
        rt.register_host_function(HostFunction::new(standard_log(), |context, _| {
            context.runtime().collect_garbage().unwrap();
            Ok(Value::Unit)
        }))
        .unwrap();
        let loaded = rt
            .load_program("foreign-retain", route(&program, encoded))
            .unwrap();
        let mut vm = Vm::new(rt);
        assert_eq!(
            vm.execute(&loaded, "main").unwrap().return_value,
            Value::I32(42)
        );
        clean(vm.runtime());
    }
}
