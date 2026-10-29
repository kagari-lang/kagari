//! Reproducible end-to-end interpreter samples; compilation/input creation are excluded.
//! Run with --ignored --nocapture --test-threads=1. This is not a JIT benchmark.
use kagari_stdlib::catalog as foundation_catalog;
mod native_allocations_counter;
use kagari_common::identity::table::DefinitionId;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{
    Runtime, RuntimeConfig, gc::roots::RootedValue, module::ExecutableModule, value::Value,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{declaration::module::ModuleDecl, scalar::BuiltinType, ty::Ty};
use kagari_vm::vm::Vm;
use native_allocations_counter::{measured, verify_counter};
use std::{hint::black_box, time::Instant};

fn setup() -> (Vm, ExecutableModule, RootedValue, Vec<DefinitionId>) {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set(
            "measure.kgr",
            include_str!("library_measurements/compare.kgr").into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut analysis = analysis_database();
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let bytecode = lower_program_to_bytecode(&mir, &Default::default())
        .map(|program| program.into_unverified())
        .unwrap();
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = Some(4096);
    let mut runtime = Runtime::new(config);
    kagari_runtime::native::module::NativeModule::install_all(
        &kagari_stdlib::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    let loaded = runtime.load_program("measure", bytecode).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let contract = loaded
        .bytecode
        .trait_contracts
        .iter()
        .find(|contract| contract.abi.name == "Run")
        .unwrap();
    let methods = [
        "primitive",
        "callback",
        "interface_primitive",
        "interface_callback",
        "script",
        "view",
    ]
    .map(|name| ModuleDecl::method_id(contract.declaration, name, loaded.definitions()).unwrap())
    .to_vec();
    let vm = Vm::new(runtime);
    let runner = vm.execute(&loaded, "runner").unwrap().return_value;
    (vm, loaded, runner, methods)
}

#[test]
#[ignore = "manual performance evidence, excludes compilation and input construction"]
fn compare_rust_native_and_script_stable_sorting() {
    verify_counter();
    let compilation = Instant::now();
    let (vm, loaded, root, methods) = setup();
    println!("compile_and_load_ns={}", compilation.elapsed().as_nanos());
    // SAFETY: The setup root remains live for every measured dispatch.
    let runner = unsafe { root.value_transport(vm.runtime().gc()) }.unwrap();
    for length in [16usize, 4096] {
        let input: Vec<i32> = (0..length)
            .map(|index| ((index * 1543 + 71) % 997) as i32)
            .collect();
        let mut expected = input.clone();
        expected.sort();
        for (case, method) in [
            "native_primitive",
            "native_callback",
            "interface_primitive",
            "interface_callback",
            "script_merge",
        ]
        .into_iter()
        .zip(&methods)
        {
            // One warm execution per shape, then three independently reset samples.
            for sample in 0..4 {
                let array = unsafe {
                    vm.runtime().alloc_array_transport(
                        &loaded,
                        Ty::Builtin(BuiltinType::I32),
                        input.iter().copied().map(Value::I32).collect(),
                    )
                }
                .unwrap();
                let array_root =
                    unsafe { vm.runtime().gc().root_value_transport(Value::Array(array)) }.unwrap();
                let counter = unsafe {
                    vm.runtime().alloc_array_transport(
                        &loaded,
                        Ty::Builtin(BuiltinType::U64),
                        vec![Value::U64(0)],
                    )
                }
                .unwrap();
                let counter_root = unsafe {
                    vm.runtime()
                        .gc()
                        .root_value_transport(Value::Array(counter))
                }
                .unwrap();
                // Prepare interface views outside the sort interval, and report
                // their construction separately from repeated dispatch.
                let values = if case.starts_with("interface_") {
                    let mut result = None;
                    let (allocations, duration) = measured(|| {
                        result = Some(
                            unsafe {
                                vm.invoke_interface_method_transport(
                                    &runner,
                                    methods.last().unwrap(),
                                    &[Value::Array(array)],
                                )
                            }
                            .unwrap(),
                        );
                    });
                    if sample > 0 {
                        println!(
                            "interface_view,n={length},sample={sample},ns={},allocations={allocations:?}",
                            duration.as_nanos()
                        );
                    }
                    result.unwrap()
                } else {
                    Value::Array(array)
                };
                let values_root =
                    unsafe { vm.runtime().gc().root_value_transport(values) }.unwrap();
                let before = vm.runtime().gc().stats();
                let (allocations, duration) = measured(|| {
                    black_box(
                        unsafe {
                            vm.invoke_interface_method_transport(
                                &runner,
                                method,
                                &[values, Value::Array(counter)],
                            )
                        }
                        .unwrap(),
                    );
                });
                let after = vm.runtime().gc().stats();
                assert_eq!(
                    unsafe { vm.runtime().gc().array_snapshot_transport(array) }.unwrap(),
                    expected.iter().copied().map(Value::I32).collect::<Vec<_>>()
                );
                let callbacks =
                    unsafe { vm.runtime().gc().array_get_transport(counter, 0) }.unwrap();
                if sample > 0 {
                    println!(
                        "{case},n={length},sample={sample},ns={},callbacks={callbacks:?},gc_objects={},allocations={allocations:?}",
                        duration.as_nanos(),
                        after.allocated_objects + after.reclaimed_objects
                            - before.allocated_objects
                            - before.reclaimed_objects
                    );
                }
                drop((array_root, counter_root, values_root));
                vm.runtime().collect_garbage().unwrap();
            }
        }
        for case in ["rust_primitive", "rust_callback"] {
            for sample in 0..4 {
                let mut values = input.clone();
                let mut comparisons = 0;
                let (allocations, duration) = measured(|| {
                    if case == "rust_primitive" {
                        black_box(&mut values).sort();
                    } else {
                        black_box(&mut values).sort_by(|left, right| {
                            comparisons += 1;
                            left.cmp(right)
                        });
                    }
                });
                assert_eq!(values, expected);
                if sample > 0 {
                    println!(
                        "{case},n={length},sample={sample},ns={},callbacks={comparisons},gc_objects=0,allocations={allocations:?}",
                        duration.as_nanos()
                    );
                }
            }
        }
    }
    drop(root);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

fn analysis_database() -> AnalysisDatabase {
    let mut database = AnalysisDatabase::default();
    database.set_native_modules(foundation_catalog::shared().unwrap());
    database
}
