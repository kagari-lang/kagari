use super::compile_program;
#[cfg(test)]
use crate::tests::support::bytecode::load_bytecode;
use kagari_runtime::{
    Runtime, RuntimeConfig,
    module::{ExecutableModule, entry::ProgramEntry},
    native::{
        binding::NativeResult, builder::ModuleBuilder, context::NativeCall,
        conversion::context::ConversionContext, declarations::FunctionDecl, views::ValueHandle,
    },
    resource::RuntimeLimits,
    value::carriers::RootedValue,
};

use crate::declarations::StandardDeclarations;
use kagari_gc::heap::HeapStats;
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::sync::{Arc, Mutex};

struct MutationFixture {
    vm: Runtime,
    loaded: ExecutableModule,
    retained: Arc<Mutex<Option<RootedValue>>>,
    baseline: HeapStats,
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
                    *keep.lock().unwrap() = Some(value.root(cx)?);
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
        runtime.collect_garbage().unwrap();
        let baseline = runtime.gc_stats();
        Self {
            vm: runtime,
            loaded,
            retained,
            baseline,
        }
    }

    fn value(&self) -> RootedValue {
        self.retained.lock().unwrap().as_ref().unwrap().clone()
    }

    fn is_empty(&mut self) -> bool {
        let value = self.value();
        let result = crate::tests::support::invoke(
            &mut self.vm,
            &self.loaded,
            "retained_is_empty",
            &[value],
        );
        result.as_bool().expect("checked bool result")
    }
}

#[test]
fn successful_removal_accounts_prepared_result_and_preserves_live_occupancy() {
    let mut fixture = MutationFixture::new(
        include_str!("../../../../tests/fixtures/native/prepared-removal.kgr"),
        RuntimeLimits {
            ..Default::default()
        },
        "Vec<i32>",
    );
    let result = fixture
        .vm
        .execute_module(
            &fixture.loaded,
            ProgramEntry::Main,
            fixture.vm.execution_options(),
        )
        .unwrap()
        .return_value;
    {
        let mut conversion = ConversionContext::new(&mut fixture.vm, &fixture.loaded).unwrap();
        assert_eq!(conversion.decode::<Option<i32>>(&result).unwrap(), Some(42));
    }
    drop(result);
    assert!(fixture.is_empty());
    let runtime = &mut fixture.vm;
    let counters = runtime.resources().counters();
    assert_eq!(counters.current_heap_units, 3);
    assert_eq!(counters.peak_heap_units, 4);

    fixture.retained.lock().unwrap().take();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.resources().counters().current_heap_units, 0);

    let _ = runtime
        .alloc_array(&fixture.loaded, Ty::Builtin(BuiltinType::I32), &[])
        .unwrap();
    assert_eq!(runtime.resources().counters().current_heap_units, 1);
}

#[test]
fn duplicate_native_insertions_preserve_final_container_contents() {
    let mut fixture = MutationFixture::new(
        include_str!("../../../../tests/fixtures/native/duplicate-containers.kgr"),
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
            .as_bool(),
        Some(true)
    );
}

#[test]
fn custom_map_removal_prepares_the_result_without_repeating_hash_callbacks() {
    {
        let mut fixture = MutationFixture::new(
            include_str!("../../../../tests/fixtures/native/hash-removal.kgr"),
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
                .as_i32(),
            Some(2)
        );
        assert!(fixture.is_empty());
        fixture.retained.lock().unwrap().take();
        fixture.vm.collect_garbage().unwrap();
        let heap = fixture.vm.gc_stats();
        assert_eq!(heap.objects, fixture.baseline.objects);
        assert_eq!(heap.provider_units, fixture.baseline.provider_units);
        assert_eq!(heap.root_registrations, fixture.baseline.root_registrations);
        assert_eq!(fixture.vm.resources().counters().current_heap_units, 0);
    }
}
