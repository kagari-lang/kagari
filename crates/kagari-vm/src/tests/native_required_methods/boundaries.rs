//! Mutation visibility on traps and shared-session native lifecycle boundaries.
use super::runtime;
use crate::{
    error::VmError,
    executor::Executor,
    tests::{common::compile_test_bytecode, native_destinations::boundaries::lifecycle},
    vm::Vm,
};
use kagari_bytecode::artifact::KbcArtifact;
use kagari_common::host_interface::standard_log;
use kagari_runtime::{Runtime, error::RuntimeErrorKind, host::HostFunction, value::Value};
use std::{cell::RefCell, rc::Rc};

#[test]
fn required_entries_reenter_cancel_and_exhaust_every_allocation_cut() {
    for (name, ty, setup, body) in [
        (
            "push",
            "ArrayList<i32>",
            "[20,22]",
            "fixture.push(42);std::debug::assert(fixture.get(2usize)==Some(42),\"push\");",
        ),
        (
            "insert",
            "ArrayList<i32>",
            "[20,22]",
            "fixture.insert(1usize,42);std::debug::assert(fixture.get(1usize)==Some(42),\"insert\");",
        ),
        (
            "set",
            "ArrayList<i32>",
            "[20,22]",
            "fixture.set(1usize,42);std::debug::assert(fixture.get(1usize)==Some(42),\"set\");",
        ),
        (
            "array_clear",
            "ArrayList<i32>",
            "[20,22]",
            "fixture.clear();std::debug::assert(fixture.is_empty(),\"clear\");",
        ),
        (
            "map_clear",
            "LinkedHashMap<String,i32>",
            "LinkedHashMap::from([(\"a\",42)])",
            "fixture.clear();std::debug::assert(fixture.is_empty(),\"clear\");",
        ),
        (
            "set_clear",
            "LinkedHashSet<i32>",
            "LinkedHashSet::from([20,22])",
            "fixture.clear();std::debug::assert(fixture.is_empty(),\"clear\");",
        ),
        (
            "iter_next",
            "ArrayList<i32>",
            "[20,22]",
            "val cursor=fixture.iter().map(|x|{print(\"visit\");x});std::debug::assert(cursor.next()==Some(20),\"next\");",
        ),
        (
            "bounds",
            "Range<usize>",
            "20usize..22usize",
            "std::debug::assert(fixture.start_bound()==Bound::Included(20usize) && fixture.end_bound()==Bound::Excluded(22usize),\"bounds\");",
        ),
        (
            "from_str",
            "String",
            "\"42\"",
            "std::debug::assert(<i32 as FromStr>::from_str(fixture)==Ok(42),\"parse\");",
        ),
    ] {
        let program = compile_test_bytecode(&format!(
            r#"
fn setup()->{ty}{{{setup}}}
fn main(fixture:{ty})->i32{{print("start");{body}print("done");42}}
fn inner()->i32{{42}}fn fail()->i32{{val x=2147483647;x+1}}fn ready()->i32{{7}}
"#
        ));
        lifecycle(&program, name);
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
            .flat_map(|(key, value)| [key, value])
            .collect(),
        Value::Set(id) => runtime.gc().set_snapshot(*id).unwrap(),
        _ => panic!("storage fixture"),
    }
}

#[test]
fn committed_unit_mutations_survive_budget_exhaustion_before_publication() {
    for (name, ty, setup, body, expected) in [
        (
            "push",
            "ArrayList<i32>",
            "[20,22]",
            "fixture.push(42);",
            vec![Value::I32(20), Value::I32(22), Value::I32(42)],
        ),
        (
            "insert",
            "ArrayList<i32>",
            "[20,22]",
            "fixture.insert(1usize,42);",
            vec![Value::I32(20), Value::I32(42), Value::I32(22)],
        ),
        (
            "set",
            "ArrayList<i32>",
            "[20,22]",
            "fixture.set(1usize,42);",
            vec![Value::I32(20), Value::I32(42)],
        ),
        (
            "array_clear",
            "ArrayList<i32>",
            "[20,22]",
            "fixture.clear();",
            vec![],
        ),
        (
            "map_clear",
            "LinkedHashMap<String,i32>",
            "LinkedHashMap::from([(\"a\",42)])",
            "fixture.clear();",
            vec![],
        ),
        (
            "set_clear",
            "LinkedHashSet<i32>",
            "LinkedHashSet::from([20,22])",
            "fixture.clear();",
            vec![],
        ),
    ] {
        let program = compile_test_bytecode(&format!(
            r#"fn setup()->{ty}{{{setup}}}fn main(fixture:{ty})->i32{{print("before");{body}print("after");42}}"#
        ));
        let root = &program.modules[program.root.index()];
        let entry = root
            .functions
            .iter()
            .find(|function| function.name == "main")
            .unwrap()
            .id;
        for encoded in [false, true] {
            let program = if encoded {
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
            };
            let effects = Rc::new(RefCell::new(Vec::new()));
            let sink = effects.clone();
            let mut rt = runtime();
            rt.register_host_function(HostFunction::new(standard_log(), move |context, args| {
                let Value::Str(label) = &args[0] else {
                    panic!()
                };
                sink.borrow_mut().push(label.clone());
                context.runtime().collect_garbage().unwrap();
                Ok(Value::Unit)
            }))
            .unwrap();
            let loaded = rt.load_program(name, program).unwrap();
            let mut vm = Vm::new(rt);
            let mut first_commit = None;
            let mut success = None;
            for limit in 0..100 {
                effects.borrow_mut().clear();
                let fixture = vm.execute(&loaded, "setup").unwrap().return_value;
                let roots = vm.runtime().gc().root_value(fixture.clone()).unwrap();
                let original = snapshot(vm.runtime(), &fixture);
                let mut options = vm.runtime().execution_options();
                options.resources.max_instruction_steps = Some(limit);
                let session = vm.runtime().begin_execution(&loaded, options).unwrap();
                let result =
                    Executor::new(vm.runtime(), &loaded, entry, std::slice::from_ref(&fixture))
                        .unwrap()
                        .run();
                let actual = snapshot(vm.runtime(), &fixture);
                if actual == expected {
                    if first_commit.is_none() {
                        assert!(
                            result.is_err(),
                            "{name}: publication must have a separate budget boundary"
                        );
                        assert_eq!(
                            *effects.borrow(),
                            ["before"],
                            "{name}: commit effect prefix"
                        );
                        first_commit = Some(limit);
                    }
                } else {
                    assert_eq!(actual, original, "{name}: partial mutation");
                }
                let complete = result.is_ok();
                if complete {
                    assert_eq!(result.unwrap(), Value::I32(42));
                    assert_eq!(*effects.borrow(), ["before", "after"]);
                    success = Some(limit);
                } else {
                    assert!(
                        matches!(result,Err(ref error) if matches!(error.cause(),VmError::RuntimeError(cause) if cause.kind()==RuntimeErrorKind::ResourceLimitExceeded)),
                        "{name} {limit}: {result:?}"
                    );
                }
                drop(session);
                drop(roots);
                assert_eq!(vm.runtime().gc().active_roots(), 0);
                assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
                assert!(!vm.runtime().is_quarantined());
                vm.runtime().collect_garbage().unwrap();
                assert_eq!(vm.runtime().gc().allocated_objects(), 0);
                if complete {
                    break;
                }
            }
            assert!(
                first_commit.unwrap() + 1 < success.unwrap(),
                "{name}: commit before publication and follow-up effects"
            );
        }
    }
}

#[test]
fn required_mutation_failures_keep_categories_and_argument_effects() {
    let program = compile_test_bytecode(
        r#"
fn index()->usize{print("index");9usize}fn item()->i32{print("item");42}
fn set()->i32{print("before");[20,22].set(index(),item());print("after");42}
fn insert()->i32{print("before");[20,22].insert(index(),item());print("after");42}
fn ready()->i32{42}
"#,
    );
    for encoded in [false, true] {
        let program = if encoded {
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
        };
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
        let loaded = rt.load_program("required-failures", program).unwrap();
        let mut vm = Vm::new(rt);
        for entry in ["set", "insert"] {
            effects.borrow_mut().clear();
            let error = vm.execute(&loaded, entry).unwrap_err();
            if entry == "set" {
                assert!(
                    matches!(error.cause(), VmError::InvalidIndex(9)),
                    "{error:?}"
                );
            } else {
                assert!(
                    matches!(error.cause(), VmError::BuiltinError(_)),
                    "{error:?}"
                );
            }
            assert_eq!(*effects.borrow(), ["before", "index", "item"]);
            assert_eq!(
                vm.execute(&loaded, "ready").unwrap().return_value,
                Value::I32(42)
            );
            assert_eq!(vm.runtime().gc().active_roots(), 0);
            assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
            assert!(!vm.runtime().is_quarantined());
            vm.runtime().collect_garbage().unwrap();
            assert_eq!(vm.runtime().gc().allocated_objects(), 0);
        }
    }
}
