//! RR00 internal calls: one warmed Host activation runs a complete script loop.
use crate::{bytecode_fixture::load_bytecode, native_allocations_counter::measured};
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::{analysis::AnalysisDatabase, compile_input::program::CheckedCompilation};
use kagari_runtime::{
    Runtime,
    native::{
        assembly::{ModuleDefinition, ModuleSetBuilder},
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        context::CallContext,
        declarations::{FunctionDecl, MethodDecl},
        module::NativeModule,
        payload::Opaque,
        storage::NativeStorage,
        typed::{NativeContext, loans::LoanContext},
        types::Type,
    },
    value::Value,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{
    declaration::native::NativeTypeConstructor, reference::ReferenceAccess, scalar::BuiltinType,
};
use std::{
    hint::black_box,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

#[cfg(feature = "execution-diagnostics")]
use std::sync::atomic::AtomicBool;

#[cfg(feature = "execution-diagnostics")]
static SNAPSHOT: AtomicBool = AtomicBool::new(false);

kagari_runtime::native_data! { struct Point { value: i32 } }

fn provider(count: &Arc<AtomicUsize>) -> (NativeModule, NativeModule) {
    let mut modules = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
    modules
        .add(ModuleDefinition::new(
            ModuleIdentity {
                package: PackageId("example".into()),
                path: vec!["strings".into()],
            },
            |module| {
                module.define_constructor("String", NativeTypeConstructor::String)?;
                Ok(())
            },
        ))
        .unwrap();
    let strings = modules
        .finish()
        .unwrap()
        .into_modules()
        .unwrap()
        .pop()
        .unwrap();
    let catalog = DeclarationCatalog::from_modules(&[&strings]).unwrap();
    let mut builder = ModuleBuilder::new("example::internal", &catalog).unwrap();
    for scalar in [false, true] {
        let declaration = if scalar {
            FunctionDecl::new("raw_scalar")
                .parameter("value", Type::i32())
                .returns(Type::i32())
        } else {
            FunctionDecl::new("raw_empty")
        };
        let function = builder.define_function(declaration).unwrap();
        let count = count.clone();
        builder
            .bind(
                function,
                NativeBinding::new(
                    if scalar {
                        vec![Codec::Exact(Type::i32())]
                    } else {
                        vec![]
                    },
                    Codec::Exact(if scalar { Type::i32() } else { Type::unit() }),
                    move |cx: &mut CallContext<'_>| {
                        count.fetch_add(1, Ordering::Relaxed);
                        if scalar {
                            cx.argument(0)
                        } else {
                            Ok(Value::Unit)
                        }
                    },
                ),
            )
            .unwrap();
    }
    let empty = builder
        .define_function(FunctionDecl::new("typed_empty"))
        .unwrap();
    let empty_count = count.clone();
    builder
        .bind(
            empty,
            move |_cx: &mut NativeContext<'_>, (): ()| -> NativeResult<()> {
                #[cfg(feature = "execution-diagnostics")]
                if SNAPSHOT.swap(false, Ordering::Relaxed) {
                    println!(
                        "internal active_storage={:?}",
                        kagari_runtime::diagnostics::storage::execution_usage(_cx.runtime())
                    );
                }
                empty_count.fetch_add(1, Ordering::Relaxed);
                Ok(())
            },
        )
        .unwrap();
    let scalar = builder
        .define_function(
            FunctionDecl::new("typed_scalar")
                .parameter("value", Type::i32())
                .returns(Type::i32()),
        )
        .unwrap();
    let scalar_count = count.clone();
    builder
        .bind(
            scalar,
            move |_: &mut NativeContext<'_>, (value,): (i32,)| -> NativeResult<i32> {
                scalar_count.fetch_add(1, Ordering::Relaxed);
                Ok(value)
            },
        )
        .unwrap();
    let owned = builder
        .define_function(
            FunctionDecl::new("owned")
                .parameter("value", Type::scalar(BuiltinType::String))
                .returns(Type::i32()),
        )
        .unwrap();
    let owned_count = count.clone();
    builder
        .bind(
            owned,
            move |_: &mut NativeContext<'_>, (value,): (String,)| -> NativeResult<i32> {
                owned_count.fetch_add(1, Ordering::Relaxed);
                assert_eq!(value, "runtime");
                Ok(value.len() as i32)
            },
        )
        .unwrap();
    let mut point = builder.define_type("Point").unwrap();
    point
        .native_storage(NativeStorage::data::<Point>())
        .unwrap();
    let point = point.finish().unwrap();
    let new = builder
        .define_function(FunctionDecl::new("point").returns(point.apply([]).unwrap()))
        .unwrap();
    builder
        .bind(
            new,
            |cx: &mut NativeContext<'_>, (): ()| -> NativeResult<Opaque<Point>> {
                cx.create_opaque(Point { value: 7 })
            },
        )
        .unwrap();
    let borrowed_count = count.clone();
    builder
        .implement(point, |implementation| {
            implementation.inherent_impl(|methods| {
                let read = methods.define_method(
                    MethodDecl::instance("read")
                        .receiver_access(ReferenceAccess::Shared)
                        .returns(Type::i32()),
                )?;
                methods.bind(
                    read,
                    move |_: &mut LoanContext<'_>, (point,): (&Point,)| -> NativeResult<i32> {
                        borrowed_count.fetch_add(1, Ordering::Relaxed);
                        Ok(point.value)
                    },
                )
            })
        })
        .unwrap();
    (strings, builder.finish().unwrap())
}

#[test]
#[ignore = "manual internal-call baseline; prepare outside timing; run release serially"]
fn internal_script_and_native_calls() {
    let preparation = Instant::now();
    let count = Arc::new(AtomicUsize::new(0));
    let (strings, provider) = provider(&count);
    let mut sources = SourceDatabase::default();
    let root = sources
        .set(
            "internal.kgr",
            include_str!("fixtures/internal_calls.kgr").into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(vec![
        Arc::new(strings.to_declaration()),
        Arc::new(provider.to_declaration()),
    ]);
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(
        &CheckedCompilation::from_program(&checked, &Default::default()).unwrap(),
        &Default::default(),
    )
    .unwrap();
    let code = lower_program_to_bytecode(&mir, &Default::default()).unwrap();
    let compilation = preparation.elapsed();
    let installation = Instant::now();
    let mut runtime = Runtime::default();
    strings.install(&mut runtime).unwrap();
    provider.install(&mut runtime).unwrap();
    let program = load_bytecode(&mut runtime, "internal", code.into_unverified()).unwrap();
    println!(
        "internal preparation compilation_ns={} installation_ns={}",
        compilation.as_nanos(),
        installation.elapsed().as_nanos()
    );
    const CALLS: i32 = 10_000;
    for (name, expected, callbacks) in [
        ("script", CALLS * 7, 0),
        ("raw_empty", 0, CALLS),
        ("raw_scalar", CALLS * 7, CALLS),
        ("typed_empty", 0, CALLS),
        ("typed_scalar", CALLS * 7, CALLS),
        ("borrowed", CALLS * 7, CALLS),
        ("owned", CALLS * 7, CALLS),
    ] {
        let binding = Instant::now();
        let function = runtime
            .bind_function::<(i32,), i32>(&program.root(), name)
            .unwrap();
        println!(
            "internal case={name} binding_ns={}",
            binding.elapsed().as_nanos()
        );
        let call = |calls| {
            let before = count.load(Ordering::Relaxed);
            assert_eq!(
                black_box(runtime.call(&function, (calls,)).unwrap()),
                expected / CALLS * calls
            );
            assert_eq!(
                count.load(Ordering::Relaxed) - before,
                (callbacks / CALLS * calls) as usize
            );
        };
        for sample in 0..14 {
            let start = Instant::now();
            call(CALLS);
            let elapsed = start.elapsed().as_nanos();
            if sample >= 3 {
                println!(
                    "internal case={name} sample={} calls={CALLS} ns={elapsed}",
                    sample - 3
                );
            }
        }
        let (allocations, _) = measured(|| call(CALLS));
        println!("internal case={name} allocations={allocations:?}");
        for calls in [0, 1] {
            let (allocations, _) = measured(|| call(calls));
            println!("internal case={name} calls={calls} allocations={allocations:?}");
        }
        #[cfg(feature = "execution-diagnostics")]
        {
            SNAPSHOT.store(name == "typed_empty", Ordering::Relaxed);
            let (_, counts) = kagari_runtime::diagnostics::measure(|| call(CALLS));
            println!("internal case={name} execution={counts:?}");
            println!(
                "internal case={name} storage={:?}",
                kagari_runtime::diagnostics::storage::execution_usage(&runtime)
            );
        }
        runtime.collect_garbage().unwrap();
    }
}
