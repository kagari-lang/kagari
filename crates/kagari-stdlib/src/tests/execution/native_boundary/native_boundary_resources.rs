use super::compile_program;
#[cfg(test)]
use crate::tests::support::bytecode::load_bytecode;
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::RuntimeError,
    gc::roots::RootedValue,
    module::{ExecutableModule, entry::ProgramEntry},
    native::{
        binding::NativeResult, builder::ModuleBuilder, context::NativeCall,
        declarations::FunctionDecl, views::ValueHandle,
    },
    resource::RuntimeLimits,
    value::Value,
};

use crate::declarations::StandardDeclarations;
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::sync::{Arc, Mutex};

struct MutationFixture {
    vm: Runtime,
    loaded: ExecutableModule,
    retained: Arc<Mutex<Option<RootedValue>>>,
}

impl MutationFixture {
    fn new(source: &str, policy: RuntimeLimits, retained_type: &str) -> Self {
        let retained: Arc<Mutex<Option<RootedValue>>> = Arc::default();
        let keep = retained.clone();
        let mut module = ModuleBuilder::new(
            "test::roots",
            &StandardDeclarations::default()
                .catalog()
                .expect("explicit standard providers"),
        )
        .unwrap();
        let function = module.define_function(FunctionDecl::new("retain")).unwrap();
        module
            .function(&function, |function| {
                let ty = function.type_parameter("T")?;
                function.parameter("value", ty.ty())?;
                Ok(())
            })
            .unwrap();
        module
            .bind(
                function,
                move |cx: &mut NativeCall<'_>, value: ValueHandle<'_>| -> NativeResult<()> {
                    *keep.lock().unwrap() = Some(
                        cx.heap()
                            .root_value(value.value(cx)?)
                            .ok_or_else(|| RuntimeError::module_validation("fixture root"))?,
                    );
                    Ok(())
                },
            )
            .unwrap();
        let module = module.finish().unwrap();
        let source = format!(
            "{source} pub fn retained_is_empty(value: {retained_type}) -> bool {{ value.is_empty() }}"
        );
        let program = compile_program(&source, Some(&module));
        let mut runtime = Runtime::new(RuntimeConfig {
            limits: policy,
            ..Default::default()
        });
        kagari_runtime::native::module::NativeModule::install_all(
            &crate::modules().unwrap(),
            &mut runtime,
        )
        .unwrap();
        module.install(&mut runtime).unwrap();
        let loaded = load_bytecode(&mut runtime, "removal", program).unwrap();
        let _loaded_program = loaded;
        let loaded = runtime.select_program(&_loaded_program).unwrap();
        let vm = runtime;
        Self {
            vm,
            loaded,
            retained,
        }
    }

    fn value(&self) -> Value {
        self.retained
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .value(self.vm.gc())
            .unwrap()
    }

    fn is_empty(&mut self) -> bool {
        let value = self.value();
        let result = crate::tests::support::invoke(
            &mut self.vm,
            &self.loaded,
            "retained_is_empty",
            &[value],
        );
        let Some(Value::Bool(empty)) = result.value(self.vm.gc()) else {
            panic!("checked bool result")
        };
        empty
    }
}

#[test]
fn successful_removal_accounts_prepared_result_and_preserves_live_occupancy() {
    let mut fixture = MutationFixture::new(
        "use test::roots::retain; fn main() -> Option<i32> { val array: Vec<i32> = Vec::new(); array.push(42); retain(array); array.pop() }",
        RuntimeLimits {
            ..Default::default()
        },
        "Vec<i32>",
    );
    let Value::Enum(result) = fixture
        .vm
        .execute_module(
            &fixture.loaded,
            ProgramEntry::Main,
            fixture.vm.execution_options(),
        )
        .unwrap()
        .return_value
        .value(fixture.vm.gc())
        .expect("retained execution result")
    else {
        panic!("Option")
    };
    assert_eq!(
        fixture.vm.gc().enum_snapshot(result).unwrap().fields,
        vec![Value::I32(42)]
    );
    assert!(fixture.is_empty());
    let runtime = &mut fixture.vm;
    let counters = runtime.resources().counters();
    assert_eq!(counters.current_heap_units, 3);
    assert_eq!(counters.peak_heap_units, 4);

    fixture.retained.lock().unwrap().take();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.resources().counters().current_heap_units, 0);

    runtime
        .alloc_array(&fixture.loaded, Ty::Builtin(BuiltinType::I32), vec![])
        .unwrap();
    assert_eq!(runtime.resources().counters().current_heap_units, 1);
}

#[test]
fn duplicate_native_insertions_preserve_final_container_contents() {
    let mut fixture = MutationFixture::new(
        r#"use std::collections::{HashMap, HashSet};

        fn main() -> bool {
            val map: HashMap<i32,i32> = HashMap::new(); map.insert(1, 2); map.insert(1, 3);
            val set: HashSet<i32> = HashSet::new(); set.insert(1); set.insert(1); map.get(1) == Some(3) && set.contains(1) && set.len() == 1u64
        }
    "#,
        RuntimeLimits {
            ..Default::default()
        },
        "Vec<i32>",
    );
    assert_eq!(
        fixture
            .vm
            .execute_module(
                &fixture.loaded,
                ProgramEntry::Main,
                fixture.vm.execution_options()
            )
            .unwrap()
            .return_value
            .value(fixture.vm.gc()),
        Some(Value::Bool(true))
    );
}

#[test]
fn custom_map_removal_prepares_the_result_without_repeating_hash_callbacks() {
    {
        let mut fixture = MutationFixture::new(
            r#"use std::collections::{HashMap};
use std::hash::{Hash};

            use test::roots::retain;
            struct Key { val calls: Vec<i32>, val number: i32 }
            impl PartialEq for Key { fn eq(self, other: Key) -> bool { self.number == other.number } }
            impl Eq for Key {}
            impl Hash for Key { fn hash(self) -> i64 { self.calls[0] = self.calls[0] + 1; 7 } }
            fn main() -> i32 {
                val key = Key { calls: Vec::from([0]), number: 1 };
                val map: HashMap<Key, i32> = HashMap::new();
                map.insert(key, 42); retain(map); map.remove(key); key.calls[0]
            }
        "#,
            RuntimeLimits::default(),
            "HashMap<Key, i32>",
        );
        assert_eq!(
            fixture
                .vm
                .execute_module(
                    &fixture.loaded,
                    ProgramEntry::Main,
                    fixture.vm.execution_options()
                )
                .unwrap()
                .return_value
                .value(fixture.vm.gc())
                .expect("retained execution result"),
            Value::I32(2)
        );
        assert!(fixture.is_empty());
        fixture.retained.lock().unwrap().take();
        fixture.vm.collect_garbage().unwrap();
        let heap = fixture.vm.gc();
        assert_eq!(heap.allocated_objects(), 0);
        assert_eq!(heap.stats().current_heap_units, 0);
        assert_eq!(heap.active_roots(), 0);
    }
}
