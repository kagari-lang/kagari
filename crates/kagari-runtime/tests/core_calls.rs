//! Core call contracts and matched workloads use explicitly authored providers.
#[path = "support/bytecode.rs"]
mod bytecode_fixture;
mod internal_calls;
mod native_allocations_counter;
mod native_scope;
use crate::bytecode_fixture::load_bytecode;
use crate::native_allocations_counter::{measured, verify_counter};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::{analysis::AnalysisDatabase, compile_input::program::CheckedCompilation};
use kagari_runtime::{
    Runtime,
    native::{
        binding::NativeResult, builder::ModuleBuilder, catalog::DeclarationCatalog,
        declarations::FunctionDecl, typed::NativeContext, types::Type,
    },
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};

use std::{hint::black_box, sync::Arc, time::Instant};

type CallCase<'a> = (&'a str, &'a dyn Fn(&mut Runtime));

#[test]
#[ignore = "manual call performance evidence; compilation and binding are excluded"]
fn core_only_script_and_native_calls() {
    verify_counter();
    let mut builder = ModuleBuilder::new("example::core", &DeclarationCatalog::default()).unwrap();
    let echo = builder
        .define_function(
            FunctionDecl::new("echo")
                .parameter("value", Type::i32())
                .returns(Type::i32()),
        )
        .unwrap();
    builder
        .bind(
            echo,
            |_: &mut NativeContext<'_>, (value,): (i32,)| -> NativeResult<i32> { Ok(value) },
        )
        .unwrap();
    let provider = builder.finish().unwrap();
    let mut sources = SourceDatabase::default();
    let root = sources
        .set(
            "core.kgr",
            include_str!("fixtures/core_only_calls.kgr").into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(vec![Arc::new(provider.to_declaration())]);
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(
        &CheckedCompilation::from_program(&checked, &Default::default()).unwrap(),
        &Default::default(),
    )
    .unwrap();
    let bytecode = lower_program_to_bytecode(&mir, &Default::default()).unwrap();
    let mut runtime = Runtime::default();
    provider.install(&mut runtime).unwrap();
    let program = load_bytecode(&mut runtime, "core", bytecode.into_unverified()).unwrap();
    let mut vm = runtime;
    let empty = vm
        .bind_function::<(), ()>(&program.root(), "empty")
        .unwrap();
    let script = vm
        .bind_function::<(), i32>(&program.root(), "script")
        .unwrap();
    let native = vm
        .bind_function::<(), i32>(&program.root(), "main")
        .unwrap();
    let cases: [CallCase<'_>; 3] = [
        ("empty", &|vm| vm.call(&empty, ()).unwrap()),
        ("script", &|vm| {
            assert_eq!(black_box(vm.call(&script, ()).unwrap()), 42)
        }),
        ("native", &|vm| {
            assert_eq!(black_box(vm.call(&native, ()).unwrap()), 42)
        }),
    ];
    const CALLS: usize = 10_000;
    for (name, call) in cases {
        for sample in 0..14 {
            let start = Instant::now();
            for _ in 0..CALLS {
                call(&mut vm);
            }
            let elapsed = start.elapsed().as_nanos();
            if sample >= 3 {
                println!(
                    "core case={name} sample={} calls={CALLS} ns={elapsed}",
                    sample - 3
                );
            }
        }
        let (counts, _) = measured(|| call(&mut vm));
        println!("core case={name} allocations={counts:?}");
    }
}
