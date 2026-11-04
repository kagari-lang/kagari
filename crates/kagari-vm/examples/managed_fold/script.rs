//! Existing interpreter and Native adapters; no alternative callback executor.

use crate::data::active;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{
    Runtime, RuntimeConfig,
    module::Program,
    native::{
        binding::NativeResult, builder::ModuleBuilder, callable::CallableHandle,
        context::CallContext, declarations::FunctionDecl, function_handle::Function,
        module::NativeModule, typed::NativeContext, types::Type,
    },
    value::Value,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_stdlib::{declarations::StandardDeclarations, modules};
use kagari_vm::vm::Vm;
use std::{hint::black_box, sync::Arc, time::Instant};

const SOURCE: &str = r#"
use example::fold_bench;
pub fn bridge() -> u64 { fold_bench::bridge(|sum, item| sum + item) }
pub fn bridge_prepared() -> u64 { fold_bench::bridge_prepared(|sum, item| sum + item) }
pub fn bridge_raw() -> u64 { fold_bench::bridge_raw(|sum, item| sum + item) }
pub fn bridge_raw_values() -> u64 { fold_bench::bridge_raw_values(|sum, item| sum + item) }
pub fn script_loop() -> u64 {
    val callback = |sum: u64, item: u64| sum + item;
    var sum: u64 = 0;
    var item: u64 = 1;
    while item <= 2048 { sum = callback(sum, item); item += 1; }
    sum
}
pub fn script_sum() -> u64 {
    var sum: u64 = 0;
    var item: u64 = 1;
    while item <= 2048 { sum += item; item += 1; }
    sum
}
pub fn scalar() -> u64 { fold_bench::scalar(|sum, item| sum + item) }
pub fn references() -> u64 { fold_bench::references(|sum, item| sum + item) }
pub fn bridge_gc() -> u64 {
    fold_bench::bridge(|sum, item| {
        val scratch = [item, item + 1];
        if item % 256 == 0 { fold_bench::pulse(); }
        sum + scratch[0]
    })
}
pub fn references_gc() -> u64 {
    fold_bench::references(|sum, item| {
        val scratch = [item, item + 1];
        if item % 256 == 0 { fold_bench::pulse(); }
        sum + scratch[0]
    })
}
pub fn values(count: u64) -> Vec<u64> {
    val values: Vec<u64> = Vec::from([]);
    var i: u64 = 1;
    while i <= count { values.push(i); i += 1; }
    values
}
pub fn production(values: Vec<u64>) -> u64 {
    values.fold(0, |sum, item| sum + item)
}
pub fn production_gc(values: Vec<u64>) -> u64 {
    values.fold(0, |sum, item| {
        val scratch = [item, item + 1];
        if item % 256 == 0 { fold_bench::pulse(); }
        sum + scratch[0]
    })
}
"#;

#[derive(Clone, Copy)]
enum Route {
    Bridge,
    Prepared,
    Scalar,
    References,
}

fn module() -> NativeModule {
    let catalog = StandardDeclarations::default().catalog().unwrap();
    let mut builder = ModuleBuilder::new("example::fold_bench", &catalog).unwrap();
    for (name, route) in [
        ("bridge", Route::Bridge),
        ("bridge_prepared", Route::Prepared),
        ("scalar", Route::Scalar),
        ("references", Route::References),
    ] {
        let entry = builder
            .define_function(
                FunctionDecl::new(name)
                    .parameter(
                        "callback",
                        Type::function([Type::u64(), Type::u64()], Type::u64()).unwrap(),
                    )
                    .returns(Type::u64()),
            )
            .unwrap();
        builder
            .bind(
                entry,
                move |cx: &mut NativeContext<'_>,
                      (callback,): (Function<(u64, u64), u64>,)|
                      -> NativeResult<u64> {
                    let data = active();
                    let mut reduce = |sum, item| callback.call(cx, (sum, item));
                    match route {
                        Route::Bridge => {
                            let mut sum = 0;
                            for item in black_box(&data.plain) {
                                sum = reduce(sum, *item)?;
                            }
                            Ok(sum)
                        }
                        Route::Prepared => {
                            let prepared = callback.prepare_call(cx.runtime())?;
                            let options = cx.runtime().execution_options();
                            let mut sum = 0;
                            for item in black_box(&data.plain) {
                                sum = prepared.call_with_options(
                                    cx,
                                    (sum, *item),
                                    options.clone(),
                                )?;
                            }
                            Ok(sum)
                        }
                        Route::Scalar => data.scalar(&mut reduce),
                        Route::References => data.references(&mut reduce),
                    }
                },
            )
            .unwrap();
    }
    for (name, values) in [("bridge_raw", false), ("bridge_raw_values", true)] {
        let entry = builder
            .define_function(
                FunctionDecl::new(name)
                    .parameter(
                        "callback",
                        Type::function([Type::u64(), Type::u64()], Type::u64()).unwrap(),
                    )
                    .returns(Type::u64()),
            )
            .unwrap();
        builder
            .bind(
                entry,
                move |cx: &mut CallContext<'_>,
                      callback: CallableHandle<'_>|
                      -> NativeResult<u64> {
                    let data = active();
                    let mut sum = 0;
                    for item in black_box(&data.plain) {
                        sum = if values {
                            let result =
                                callback.call_values(cx, &[Value::U64(sum), Value::U64(*item)])?;
                            let Value::U64(result) = result else {
                                panic!("checked scalar callback result");
                            };
                            result
                        } else {
                            callback.call(cx, (sum, *item))?
                        };
                    }
                    Ok(sum)
                },
            )
            .unwrap();
    }
    let pulse = builder.define_function(FunctionDecl::new("pulse")).unwrap();
    builder
        .bind(pulse, |cx: &mut CallContext<'_>| -> NativeResult<()> {
            // All pressure routes collect both heaps on the same schedule. Only
            // managed fold has an active candidate call/locals at this boundary.
            active().memory.collect()?;
            cx.collect_garbage()?;
            Ok(())
        })
        .unwrap();
    builder.finish().unwrap()
}

pub fn prepare() -> (Vm, Program) {
    let start = Instant::now();
    let mut modules = modules().unwrap();
    modules.push(module());
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("fold_benchmark.kgr", SOURCE.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(
        modules
            .iter()
            .map(|module| Arc::new(module.to_declaration()))
            .collect(),
    );
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let program = lower_program_to_bytecode(&mir, &Default::default())
        .unwrap()
        .into_unverified();
    eprintln!("setup,compile_ns={}", start.elapsed().as_nanos());
    let start = Instant::now();
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = None;
    let mut runtime = Runtime::new(config);
    NativeModule::install_all(&modules, &mut runtime).unwrap();
    let loaded = runtime.load_program("fold_benchmark", program).unwrap();
    let vm = Vm::new(runtime);
    eprintln!("setup,install_link_ns={}", start.elapsed().as_nanos());
    (vm, loaded)
}
