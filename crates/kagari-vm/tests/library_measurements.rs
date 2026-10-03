//! Reproducible end-to-end interpreter samples; compilation/input creation are excluded.
//! Run with --ignored --nocapture --test-threads=1. This is not a JIT benchmark.
mod native_allocations_counter;
use kagari_common::{
    identity::DefinitionPath,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_contract::{declaration::ModuleDecl, scalar::BuiltinType, types::Ty};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{Runtime, RuntimeConfig, module::LoadedModule, value::Value};
use kagari_vm::vm::Vm;
use native_allocations_counter::{measured, verify_counter};
use std::{hint::black_box, time::Instant};

fn setup() -> (Vm, LoadedModule, Value, Vec<DefinitionPath>) {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set(
            "measure.kgr",
            include_str!("library_measurements/compare.kgr").into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let bytecode = lower_program_to_bytecode(&mir).unwrap();
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = Some(4096);
    let mut runtime = Runtime::new(config);
    let loaded = runtime.load_program("measure", bytecode).unwrap();
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
    .map(|name| {
        ModuleDecl::method_id(
            &loaded
                .definitions()
                .resolve(contract.declaration)
                .unwrap()
                .to_path(),
            name,
        )
    })
    .to_vec();
    let mut vm = Vm::new(runtime);
    let runner = vm.execute(&loaded, "runner").unwrap().return_value;
    (vm, loaded, runner, methods)
}

#[test]
#[ignore = "manual performance evidence, excludes compilation and input construction"]
fn compare_rust_native_and_script_stable_sorting() {
    verify_counter();
    let compilation = Instant::now();
    let (mut vm, loaded, runner, methods) = setup();
    println!("compile_and_load_ns={}", compilation.elapsed().as_nanos());
    let root = vm.runtime().root_value(runner.clone()).unwrap();
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
                let array = vm
                    .runtime()
                    .alloc_array(
                        &loaded,
                        Ty::Builtin(BuiltinType::I32),
                        input.iter().copied().map(Value::I32).collect(),
                    )
                    .unwrap();
                let array_root = vm.runtime().root_value(Value::Array(array)).unwrap();
                let counter = vm
                    .runtime()
                    .alloc_array(
                        &loaded,
                        Ty::Builtin(BuiltinType::USize),
                        vec![Value::U64(0)],
                    )
                    .unwrap();
                let counter_root = vm.runtime().root_value(Value::Array(counter)).unwrap();
                // Prepare interface views outside the sort interval, and report
                // their construction separately from repeated dispatch.
                let values = if case.starts_with("interface_") {
                    let mut result = None;
                    let (allocations, duration) = measured(|| {
                        result = Some(
                            vm.invoke_interface_method(
                                &runner,
                                methods.last().unwrap(),
                                &[Value::Array(array)],
                            )
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
                let values_root = vm.runtime().root_value(values.clone()).unwrap();
                let before = vm.runtime().gc().stats();
                let (allocations, duration) = measured(|| {
                    black_box(
                        vm.invoke_interface_method(
                            &runner,
                            method,
                            &[values, Value::Array(counter)],
                        )
                        .unwrap(),
                    );
                });
                let after = vm.runtime().gc().stats();
                assert_eq!(
                    vm.runtime().gc().array_snapshot(array).unwrap(),
                    expected.iter().copied().map(Value::I32).collect::<Vec<_>>()
                );
                let callbacks = vm.runtime().gc().array_get(counter, 0).unwrap();
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
