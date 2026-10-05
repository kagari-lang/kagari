use super::compile_program;
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::RuntimeError,
    gc::roots::RootedValue,
    module::LoadedModule,
    native::{
        binding::NativeResult, builder::ModuleBuilder, context::CallContext,
        declarations::FunctionDecl, views::ValueHandle,
    },
    resource::RuntimeLimits,
    value::Value,
};
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_types::{scalar::BuiltinType, ty::Ty};
use kagari_vm::vm::Vm;
use std::{cell::RefCell, rc::Rc};

struct MutationFixture {
    vm: Vm,
    loaded: LoadedModule,
    retained: Rc<RefCell<Option<RootedValue>>>,
}

impl MutationFixture {
    fn new(source: &str, policy: RuntimeLimits) -> Self {
        let retained: Rc<RefCell<Option<RootedValue>>> = Rc::default();
        let keep = retained.clone();
        let mut module = ModuleBuilder::new(
            "test::roots",
            &StandardDeclarations::default()
                .catalog()
                .expect("explicit standard providers"),
        );
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
            limits: policy,
            ..Default::default()
        });
        kagari_runtime::native::module::NativeModule::install_all(
            &kagari_stdlib::modules().unwrap(),
            &mut runtime,
        )
        .unwrap();
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
        self.retained
            .borrow()
            .as_ref()
            .unwrap()
            .value(self.vm.runtime().gc())
            .unwrap()
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
fn successful_removal_accounts_prepared_result_and_preserves_live_occupancy() {
    let fixture = MutationFixture::new(
        "use test::roots::retain; fn main() -> Option<i32> { val array = [42]; retain(array); array.pop() }",
        RuntimeLimits {
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

    fixture.retained.borrow_mut().take();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.resources().counters().current_heap_units, 0);

    runtime
        .alloc_array(&fixture.loaded, Ty::Builtin(BuiltinType::I32), vec![])
        .unwrap();
    assert_eq!(runtime.resources().counters().current_heap_units, 1);
}

#[test]
fn duplicate_native_insertions_preserve_final_container_contents() {
    let fixture = MutationFixture::new(
        r#"use std::collections::{HashMap, HashSet};

        fn main() -> (HashMap<i32,i32>, HashSet<i32>) {
            val map: HashMap<i32,i32> = HashMap::new(); map.insert(1, 2); map.insert(1, 3);
            val set: HashSet<i32> = HashSet::new(); set.insert(1); set.insert(1); (map, set)
        }
    "#,
        RuntimeLimits {
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
    {
        let fixture = MutationFixture::new(
            r#"use std::collections::{HashMap};
use std::hash::{Hash};

            use test::roots::retain;
            struct Key { val calls: Vec<i32>, val number: i32 }
            impl PartialEq for Key { fn eq(self, other: Key) -> bool { self.number == other.number } }
            impl Eq for Key {}
            impl Hash for Key { fn hash(self) -> i64 { self.calls[0] = self.calls[0] + 1; 7 } }
            fn main() -> i32 {
                val key = Key { calls: [0], number: 1 };
                val map: HashMap<Key, i32> = HashMap::new();
                map.insert(key, 42); retain(map); map.remove(key); key.calls[0]
            }
        "#,
            RuntimeLimits::default(),
        );
        assert_eq!(
            fixture
                .vm
                .execute(&fixture.loaded, "main")
                .unwrap()
                .return_value,
            Value::I32(2)
        );
        let heap = fixture.vm.runtime().gc();
        assert!(fixture.contents().is_empty());
        fixture.retained.borrow_mut().take();
        fixture.vm.runtime().collect_garbage().unwrap();
        assert_eq!(heap.allocated_objects(), 0);
        assert_eq!(heap.stats().current_heap_units, 0);
        assert_eq!(heap.active_roots(), 0);
    }
}
