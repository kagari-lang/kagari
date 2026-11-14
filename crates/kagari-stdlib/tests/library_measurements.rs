//! Reproducible end-to-end interpreter samples; compilation/input creation are excluded.
//! Run with --ignored --nocapture --test-threads=1. This is not a JIT benchmark.
//! managed_objects_delta is the net number of resident managed allocations at
//! observation, including unswept garbage; it is not a cumulative allocation count.
// Shared preparation keeps verifier and linker failures observable.
#[path = "../../kagari-runtime/tests/support/bytecode.rs"]
mod bytecode_fixture;
use crate::bytecode_fixture::load_bytecode;
use kagari_stdlib::catalog as foundation_catalog;
#[path = "../../kagari-runtime/tests/native_allocations_counter.rs"]
mod native_allocations_counter;
use kagari_common::identity::table::DefinitionId;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_gc::heap::HeapStats;
use kagari_hir::{analysis::AnalysisDatabase, compile_input::program::CheckedCompilation};
use kagari_runtime::{
    Runtime, RuntimeConfig, module::ExecutableModule,
    native::conversion::context::ConversionContext, value::carriers::RootedValue,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{declaration::module::ModuleDecl, scalar::BuiltinType, ty::Ty};

use native_allocations_counter::{measured, verify_counter};
use std::{hint::black_box, slice, time::Instant};

struct Fixture {
    runtime: Runtime,
    owner: ExecutableModule,
    runner: RootedValue,
    methods: Vec<DefinitionId>,
    baseline: HeapStats,
}

fn setup() -> Fixture {
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
    let mir = lower_program_to_mir(
        &CheckedCompilation::from_program(&checked, &Default::default()).unwrap(),
        &Default::default(),
    )
    .unwrap();
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
    let loaded = load_bytecode(&mut runtime, "measure", bytecode).unwrap();
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
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let mut vm = runtime;
    let runner = vm
        .execute(&_loaded_program, "runner", &[], &Default::default())
        .unwrap()
        .return_value;
    Fixture {
        runtime: vm,
        owner: loaded,
        runner,
        methods,
        baseline,
    }
}

#[test]
#[ignore = "manual performance evidence, excludes compilation and input construction"]
fn compare_rust_native_and_script_stable_sorting() {
    verify_counter();
    let compilation = Instant::now();
    let Fixture {
        runtime: mut vm,
        owner: loaded,
        runner,
        methods,
        baseline,
    } = setup();
    println!("compile_and_load_ns={}", compilation.elapsed().as_nanos());
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
                    .alloc_array(
                        &loaded,
                        Ty::Builtin(BuiltinType::I32),
                        &input
                            .iter()
                            .copied()
                            .map(RootedValue::from)
                            .collect::<Vec<_>>(),
                    )
                    .unwrap();
                let counter = vm
                    .alloc_array(
                        &loaded,
                        Ty::Builtin(BuiltinType::U64),
                        &[RootedValue::from(0u64)],
                    )
                    .unwrap();
                // Prepare interface views outside the sort interval, and report
                // their construction separately from repeated dispatch.
                let values = if case.starts_with("interface_") {
                    let mut result = None;
                    let (allocations, duration) = measured(|| {
                        result = Some(
                            vm.invoke_interface_method(
                                &runner,
                                methods.last().unwrap(),
                                slice::from_ref(&array),
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
                    array.clone()
                };
                let arguments = [values, counter.clone()];
                let before = vm.gc_stats();
                let (allocations, duration) = measured(|| {
                    let _ = black_box(
                        vm.invoke_interface_method(&runner, method, &arguments)
                            .unwrap(),
                    );
                });
                let after = vm.gc_stats();
                let mut cx = ConversionContext::new(&mut vm, &loaded).unwrap();
                let values = cx.import(&array).unwrap();
                assert_eq!(cx.array_len(&values).unwrap(), expected.len());
                for (index, expected) in expected.iter().enumerate() {
                    assert_eq!(
                        cx.array_element(&values, index).unwrap().unwrap().as_i32(),
                        Some(*expected)
                    );
                }
                let counter_value = cx.import(&counter).unwrap();
                let callbacks = cx
                    .array_element(&counter_value, 0)
                    .unwrap()
                    .unwrap()
                    .as_u64()
                    .unwrap();
                drop(cx);
                if sample > 0 {
                    println!(
                        "{case},n={length},sample={sample},ns={},callbacks={callbacks:?},managed_objects_delta={},allocations={allocations:?}",
                        duration.as_nanos(),
                        after.objects as i128 - before.objects as i128
                    );
                }
                drop((array, counter, arguments));
                vm.collect_garbage().unwrap();
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
                        "{case},n={length},sample={sample},ns={},callbacks={comparisons},managed_objects_delta=0,allocations={allocations:?}",
                        duration.as_nanos()
                    );
                }
            }
        }
    }
    drop(runner);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

fn analysis_database() -> AnalysisDatabase {
    let mut database = AnalysisDatabase::default();
    database.set_native_modules(foundation_catalog::shared().unwrap());
    database
}
