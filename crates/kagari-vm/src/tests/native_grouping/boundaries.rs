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
fn grouping_callbacks_reenter_cancel_and_exhaust_every_allocation_limit() {
    for mode in ["native", "inspect", "custom"] {
        let source = match mode {
            "native" => "items.iter()",
            "inspect" => "items.iter().inspect(|item|print(\"next\"))",
            _ => "Cursor{items:items,index:0usize}",
        };
        let program = compile_test_bytecode(&format!(
            r#"{TYPES}
fn setup()->ArrayList<Item<Key>>{{[Item{{key:Key{{id:20,visits:0}},id:0,visits:0}},Item{{key:Key{{id:21,visits:0}},id:1,visits:0}},Item{{key:Key{{id:20,visits:0}},id:2,visits:0}},Item{{key:Key{{id:22,visits:0}},id:3,visits:0}}]}}
fn main(items:ArrayList<Item<Key>>)->i32{{val output={source}.group_by(|item|{{print("key");item.visits+=1;item.key}});std::debug::assert(output.len()==3usize,"groups");print("grouped");42}}
fn inner()->i32{{val groups=[20,1,22,3].iter().group_by(|item|item%2);std::debug::assert(groups.len()==2usize,"inner");42}}
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
                                    crate::reenter(context, &root, inner, &[]).unwrap().value(),
                                    Value::I32(42)
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
                        .load_program("grouping-boundaries", route(&program, encoded))
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

fn field(vm: &Vm, value: &Value, name: &str) -> Value {
    let Value::Struct(id) = value else { panic!() };
    vm.runtime()
        .gc()
        .struct_snapshot(*id)
        .unwrap()
        .1
        .into_iter()
        .find(|field| field.name == name)
        .unwrap()
        .value
}
fn snapshot(vm: &Vm, value: &Value) -> Vec<Value> {
    let Value::Array(id) = value else { panic!() };
    vm.runtime().gc().array_snapshot(*id).unwrap()
}

#[test]
fn grouping_traps_preserve_effects_and_native_alias_guards() {
    for custom in [false, true] {
        for failure in [
            "next",
            "key",
            "hash_read",
            "hash_insert",
            "eq_read",
            "eq_insert",
            "alias_key",
            "alias_hash",
            "alias_eq",
        ] {
            if custom && failure.starts_with("alias_") {
                continue;
            }
            let trap = "val x=2147483647;x+1;";
            let next = if failure == "next" { trap } else { "" };
            let key = match failure {
                "key" => trap,
                "alias_key" => "state.items.clear();",
                _ => "",
            };
            let hash = match failure {
                "hash_read" => "if self.state.hashes==1{val x=2147483647;x+1;}",
                "hash_insert" => "if self.state.hashes==2{val x=2147483647;x+1;}",
                "alias_hash" => "self.state.items.clear();",
                _ => "",
            };
            let equal = match failure {
                "eq_read" => "if self.state.equals==1{val x=2147483647;x+1;}",
                "eq_insert" => "if self.state.equals==2{val x=2147483647;x+1;}",
                "alias_eq" => "self.state.items.clear();",
                _ => "",
            };
            let source = if custom {
                "Cursor{state:state,index:0usize}"
            } else {
                "state.items.iter().inspect(|item|{state.effects.push(3);print(\"next\");NEXT})"
            };
            let source = source.replace("NEXT", next);
            let program = compile_test_bytecode(&format!(
                r#"
struct State{{val items:ArrayList<Key>,val effects:ArrayList<i32>,var hashes:i32,var equals:i32}}
struct Key{{val id:i32,val state:State}}
impl PartialEq for Key{{fn eq(self,other:Self)->bool{{self.state.effects.push(2);self.state.equals+=1;print("eq");{equal}self.id==other.id}}}}impl Eq for Key{{}}
impl Hash for Key{{fn hash(self)->i64{{self.state.effects.push(1);self.state.hashes+=1;print("hash");{hash}0i64}}}}
struct Cursor{{val state:State,var index:usize}}
impl Iterator for Cursor{{type Item=Key;fn next(self)->Option<Key>{{self.state.effects.push(3);print("next");{next}val item=self.state.items.get(self.index);self.index+=1usize;item}}}}
fn setup()->State{{val state=State{{items:[],effects:[],hashes:0,equals:0}};state.items.push(Key{{id:20,state:state}});state.items.push(Key{{id:21,state:state}});state.items.push(Key{{id:20,state:state}});state}}
fn main(state:State){{val groups={source}.group_by(|item|{{state.effects.push(0);print("key");{key}item}});}}
fn ready(state:State)->i32{{state.items.push(Key{{id:42,state:state}});state.effects.len() as i32}}
"#
            ));
            let root = &program.modules[program.root.index()];
            let entry = root.functions.iter().find(|f| f.name == "main").unwrap().id;
            let ready = root
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
                    .load_program("grouping-traps", route(&program, encoded))
                    .unwrap();
                let mut vm = Vm::new(rt);
                let state = vm.execute(&loaded, "setup").unwrap().return_value;
                let roots = vm.runtime().gc().root_value(state.clone()).unwrap();
                let items = field(&vm, &state, "items");
                let original = snapshot(&vm, &items);
                let session = vm
                    .runtime()
                    .begin_execution(&loaded, vm.runtime().execution_options())
                    .unwrap();
                let error = Executor::new(vm.runtime(), &loaded, entry, slice::from_ref(&state))
                    .unwrap()
                    .run()
                    .unwrap_err();
                if failure.starts_with("alias_") {
                    assert!(
                        matches!(error.cause(),VmError::BuiltinError(cause) if cause.message().contains("during iteration")),
                        "{failure}"
                    );
                } else {
                    assert!(
                        matches!(error.cause(),VmError::RuntimeError(cause) if cause.kind()==RuntimeErrorKind::ScriptTrap),
                        "{failure}"
                    );
                }
                assert_eq!(snapshot(&vm, &items), original, "{failure}");
                let effects = snapshot(&vm, &field(&vm, &state, "effects"));
                assert!(!effects.is_empty());
                if failure == "hash_insert" {
                    assert_eq!(field(&vm, &state, "hashes"), Value::I32(2));
                }
                if failure == "eq_insert" {
                    assert_eq!(field(&vm, &state, "equals"), Value::I32(2));
                }
                drop(session);
                let session = vm
                    .runtime()
                    .begin_execution(&loaded, vm.runtime().execution_options())
                    .unwrap();
                assert_eq!(
                    Executor::new(vm.runtime(), &loaded, ready, &[state])
                        .unwrap()
                        .run()
                        .unwrap(),
                    Value::I32(effects.len() as i32)
                );
                drop(session);
                drop(roots);
                clean(&vm);
            }
        }
    }
}

#[test]
fn grouping_consumes_remaining_state_and_stops_at_each_custom_none() {
    let program = compile_test_bytecode(
        r#"
struct Cursor{var index:i32}
impl Iterator for Cursor{type Item=i32;fn next(self)->Option<i32>{print("next");val index=self.index;self.index+=1;match index{0=>Some(20),2=>Some(22),_=>None}}}
fn main()->i32{
 val calls:ArrayList<i32> =[];val custom=Cursor{index:0};
 val first=custom.group_by(|item|{calls.push(item);print("key");item});
 val second=custom.group_by(|item|{calls.push(item);print("key");item});
 std::debug::assert(first.len()==1usize && first.get(20).unwrap_or([])[0usize]==20 && second.len()==1usize && second.get(22).unwrap_or([])[0usize]==22 && custom.index==4,"custom none");
 val items=[20,21,22,23];val source=items.iter();val alias=source;
 std::debug::assert(source.next()==Some(20),"already consumed");
 val remaining=alias.group_by(|item|{calls.push(item);print("key");item%2});
 val entries=remaining.entries();val odd=entries.get(0usize).unwrap_or((9,[]));val even=entries.get(1usize).unwrap_or((9,[]));
 std::debug::assert(odd[0]==1 && odd[1][0usize]==21 && odd[1][1usize]==23 && even[0]==0 && even[1][0usize]==22 && alias.next()==None,"remaining order");
 std::debug::assert(calls.len()==5usize && calls[0usize]==20 && calls[1usize]==22 && calls[2usize]==21 && calls[3usize]==22 && calls[4usize]==23,"once only");
 items.push(42);42
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
            .load_program("grouping-state", route(&program, encoded))
            .unwrap();
        let mut vm = Vm::new(rt);
        assert_eq!(
            vm.execute(&loaded, "main").unwrap().return_value,
            Value::I32(42)
        );
        clean(&vm);
    }
}
