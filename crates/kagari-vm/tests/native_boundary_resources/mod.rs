use super::compile_program;
use kagari_abi::{scalar::BuiltinType, types::AbiType};
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::{RuntimeError, RuntimeErrorKind},
    gc::RootedValue,
    module::LoadedModule,
    native::{
        binding::NativeResult, builder::ModuleBuilder, context::CallContext,
        declarations::FunctionDecl, language::LanguageContracts, views::ValueHandle,
    },
    resource::ResourcePolicy,
    value::Value,
};
use kagari_vm::{error::VmError, vm::Vm};
use std::{cell::RefCell, rc::Rc};

struct MutationFixture {
    vm: Vm,
    loaded: LoadedModule,
    retained: Rc<RefCell<Option<RootedValue>>>,
}
impl MutationFixture {
    fn new(source: &str, policy: ResourcePolicy) -> Self {
        let retained: Rc<RefCell<Option<RootedValue>>> = Rc::default();
        let keep = retained.clone();
        let mut module = ModuleBuilder::new("test::roots", &LanguageContracts::default());
        let function = module.define_function(FunctionDecl::new("retain")).unwrap();
        module
            .function(&function, |function| {
                let ty = function.type_parameter("T")?;
                function.parameter("value", ty.ty());
                Ok(())
            })
            .unwrap();
        module
            .bind(
                function,
                move |cx: &mut CallContext<'_>, value: ValueHandle<'_>| -> NativeResult<()> {
                    *keep.borrow_mut() = Some(
                        cx.heap()
                            .root_value(value.value())
                            .ok_or_else(|| RuntimeError::module_validation("fixture root"))?,
                    );
                    Ok(())
                },
            )
            .unwrap();
        let module = module.finish().unwrap();
        let program = compile_program(source, Some(&module));
        let mut runtime = Runtime::new(RuntimeConfig {
            resources: policy,
            ..Default::default()
        });
        module.install(&mut runtime).unwrap();
        let loaded = runtime.load_program("removal", program).unwrap();
        let vm = Vm::new(runtime);
        Self {
            vm,
            loaded,
            retained,
        }
    }
    fn value(&self) -> Value {
        self.retained.borrow().as_ref().unwrap().value()
    }
    fn contents(&self) -> Vec<Value> {
        let heap = self.vm.runtime().gc();
        match self.value() {
            Value::Array(id) => heap.array_snapshot(id).unwrap(),
            Value::Map(id) => heap
                .map_snapshot(id)
                .unwrap()
                .into_iter()
                .map(|(key, value)| Value::Tuple(vec![key, value]))
                .collect(),
            Value::Set(id) => heap.set_snapshot(id).unwrap(),
            _ => panic!("retained collection"),
        }
    }
}

#[test]
fn option_allocation_failure_does_not_remove_an_array_or_map_entry() {
    for policy in [
        ResourcePolicy {
            max_heap_units: Some(2),
            ..Default::default()
        },
        ResourcePolicy {
            max_allocation_units: Some(2),
            ..Default::default()
        },
    ] {
        for (setup, operation, expected) in [
            (
                "val array = [42]; retain(array);",
                "array.pop()",
                vec![Value::I32(42)],
            ),
            (
                "val array = [42]; retain(array);",
                "array.remove(0)",
                vec![Value::I32(42)],
            ),
            (
                "val map: HashMap<i32,i32> = HashMap::new(); map.insert(1, 42); retain(map);",
                "map.remove(1)",
                vec![Value::Tuple(vec![Value::I32(1), Value::I32(42)])],
            ),
        ] {
            let source = format!("use test::roots::retain; fn main() {{ {setup} {operation}; }}");
            let mut fixture = MutationFixture::new(&source, policy);
            let error = fixture.vm.execute(&fixture.loaded, "main").unwrap_err();
            assert!(
                matches!(error.cause(), VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::ResourceLimitExceeded),
                "{error:?}"
            );
            let heap = fixture.vm.runtime().gc();
            let contents = fixture.contents();
            assert_eq!(contents, expected, "{operation}");
            assert_eq!(heap.stats().current_heap_units, 2);
            assert_eq!(heap.stats().allocation_units, 2);
            assert_eq!(
                fixture
                    .vm
                    .runtime()
                    .resources()
                    .counters()
                    .current_call_depth,
                0
            );
            fixture.retained.borrow_mut().take();
            assert_eq!(
                fixture
                    .vm
                    .runtime()
                    .collect_garbage()
                    .unwrap()
                    .reclaimed_objects,
                1
            );
            assert_eq!(heap.active_roots(), 0);
        }
    }
}

#[test]
fn native_growth_obeys_shared_limits_without_charging_failed_writes() {
    for policy in [
        ResourcePolicy {
            max_heap_units: Some(2),
            ..Default::default()
        },
        ResourcePolicy {
            max_allocation_units: Some(2),
            ..Default::default()
        },
    ] {
        for (setup, operation, expected) in [
            (
                "val array = [1]; retain(array);",
                "array.push(3)",
                vec![Value::I32(1)],
            ),
            (
                "val array = [1]; retain(array);",
                "array.insert(0, 3)",
                vec![Value::I32(1)],
            ),
            (
                "val map: HashMap<i32,i32> = HashMap::new(); map.insert(1, 2); retain(map);",
                "map.insert(2, 3)",
                vec![Value::Tuple(vec![Value::I32(1), Value::I32(2)])],
            ),
            (
                "val set: HashSet<i32> = HashSet::new(); set.insert(1); retain(set);",
                "set.insert(2)",
                vec![Value::I32(1)],
            ),
        ] {
            let source = format!("use test::roots::retain; fn main() {{ {setup} {operation}; }}");
            let mut fixture = MutationFixture::new(&source, policy);
            let error = fixture.vm.execute(&fixture.loaded, "main").unwrap_err();
            assert!(
                matches!(error.cause(), VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::ResourceLimitExceeded),
                "{error:?}"
            );
            assert_eq!(fixture.contents(), expected);
            let heap = fixture.vm.runtime().gc();
            let before = heap.stats();
            assert_eq!(before.current_heap_units, 2);
            assert_eq!(before.allocation_units, 2);
            match fixture.value() {
                Value::Map(map) => {
                    heap.map_insert(map, Value::I32(1), Value::I32(9)).unwrap();
                    assert_eq!(heap.map_get(map, &Value::I32(1)), Some(Value::I32(9)));
                }
                Value::Set(set) => assert!(!heap.set_insert(set, Value::I32(1)).unwrap()),
                Value::Array(_) => {}
                _ => unreachable!(),
            }
            assert_eq!(heap.stats(), before);
        }
    }
}

#[test]
fn successful_removal_accounts_prepared_result_and_never_refunds_allocation_budget() {
    let mut fixture = MutationFixture::new(
        "use test::roots::retain; fn main() -> Option<i32> { val array = [42]; retain(array); array.pop() }",
        ResourcePolicy {
            max_allocation_units: Some(4),
            ..Default::default()
        },
    );
    let Value::Enum(result) = fixture
        .vm
        .execute(&fixture.loaded, "main")
        .unwrap()
        .return_value
    else {
        panic!("Option")
    };
    let runtime = fixture.vm.runtime();
    assert_eq!(
        runtime.gc().enum_snapshot(result).unwrap().fields,
        vec![Value::I32(42)]
    );
    assert!(fixture.contents().is_empty());
    let counters = runtime.resources().counters();
    assert_eq!(counters.current_heap_units, 3);
    assert_eq!(counters.peak_heap_units, 4);
    assert_eq!(counters.allocation_units, 4);
    fixture.retained.borrow_mut().take();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.resources().counters().current_heap_units, 0);
    assert_eq!(runtime.resources().counters().allocation_units, 4);
    assert!(
        runtime
            .alloc_array(&fixture.loaded, AbiType::Builtin(BuiltinType::I32), vec![])
            .unwrap_err()
            .message()
            .contains("allocation units")
    );
}

#[test]
fn duplicate_native_insertions_only_charge_final_container_size() {
    let mut fixture = MutationFixture::new(
        r#"
        fn main() -> (HashMap<i32,i32>, HashSet<i32>) {
            val map: HashMap<i32,i32> = HashMap::new(); map.insert(1, 2); map.insert(1, 3);
            val set: HashSet<i32> = HashSet::new(); set.insert(1); set.insert(1); (map, set)
        }
    "#,
        ResourcePolicy {
            max_heap_units: Some(4),
            max_allocation_units: Some(4),
            ..Default::default()
        },
    );
    let Value::Tuple(values) = fixture
        .vm
        .execute(&fixture.loaded, "main")
        .unwrap()
        .return_value
    else {
        panic!("containers")
    };
    let [Value::Map(map), Value::Set(set)] = values.as_slice() else {
        panic!("handles")
    };
    let runtime = fixture.vm.runtime();
    assert_eq!(runtime.resources().counters().allocation_units, 4);
    assert_eq!(
        runtime.gc().map_get(*map, &Value::I32(1)),
        Some(Value::I32(3))
    );
    assert_eq!(
        runtime.gc().set_snapshot(*set).unwrap(),
        vec![Value::I32(1)]
    );
}

#[test]
fn custom_map_removal_prepares_the_result_without_repeating_hash_callbacks() {
    for policy in [
        ResourcePolicy {
            max_heap_units: Some(7),
            ..Default::default()
        },
        ResourcePolicy {
            max_allocation_units: Some(7),
            ..Default::default()
        },
    ] {
        let mut fixture = MutationFixture::new(
            r#"
            use test::roots::retain;
            struct Key { val calls: ArrayList<i32>, val number: i32 }
            impl PartialEq for Key { fn eq(self, other: Key) -> bool { self.number == other.number } }
            impl Eq for Key {}
            impl Hash for Key { fn hash(self) -> i64 { self.calls[0] = self.calls[0] + 1; 7 } }
            fn main() {
                val key = Key { calls: [0], number: 1 };
                val map: HashMap<Key, i32> = HashMap::new();
                map.insert(key, 42); retain(map); map.remove(key);
            }
        "#,
            policy,
        );
        let error = fixture.vm.execute(&fixture.loaded, "main").unwrap_err();
        assert!(
            matches!(error.cause(), VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::ResourceLimitExceeded),
            "{error:?}"
        );
        let heap = fixture.vm.runtime().gc();
        let Value::Map(map) = fixture.value() else {
            panic!("map")
        };
        let entries = heap.map_snapshot(map).unwrap();
        let [(Value::Struct(key), Value::I32(42))] = entries.as_slice() else {
            panic!("original entry")
        };
        let schema = heap.struct_layout(*key).unwrap();
        let Value::Array(calls) = heap.struct_get_slot(*key, &schema, 0).unwrap() else {
            panic!("calls")
        };
        assert_eq!(heap.array_snapshot(calls).unwrap(), vec![Value::I32(2)]);
        assert_eq!(heap.stats().current_heap_units, 7);
        assert_eq!(heap.stats().allocation_units, 7);
        fixture.retained.borrow_mut().take();
        assert_eq!(
            fixture
                .vm
                .runtime()
                .collect_garbage()
                .unwrap()
                .reclaimed_objects,
            3
        );
        assert_eq!(heap.active_roots(), 0);
    }
}
