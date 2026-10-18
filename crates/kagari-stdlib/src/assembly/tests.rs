//! SR01 lifecycle proofs use the same bundle that migrated stdlib recipes will use.
use crate::assembly::Bundle;
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, PackageId,
};
use kagari_runtime::{
    Runtime,
    native::{
        assembly::{
            ModuleAssembly, ModuleDefinition, ModuleSetBuilder,
            authoring::{PendingBinding, SealedPreparation},
        },
        binding::{Codec, NativeBinding, NativeResult},
        catalog::DeclarationCatalog,
        declarations::{FunctionDecl, MethodDecl},
        module::NativeModule,
        primitive::NativePrimitive,
        types::Type,
    },
    value::Value,
};
use kagari_types::{
    declaration::{TypeDefKind, VariantDef, native::NativeTypeConstructor},
    scalar::BuiltinType,
};
use std::{
    sync::{
        Arc, Barrier,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

mod failures;
mod standard;

const WAIT: Duration = Duration::from_secs(10);

type Recipe = Box<dyn FnOnce() -> NativeResult<ModuleAssembly> + Send>;
type TestBundle = Bundle<Recipe>;

fn bundle(
    recipe: impl FnOnce() -> NativeResult<ModuleAssembly> + Send + 'static,
) -> Arc<TestBundle> {
    Arc::new(Bundle::new("SR01 representative", Box::new(recipe)))
}

fn owner(name: &str) -> ModuleIdentity {
    ModuleIdentity {
        package: PackageId("sr01-bundle".into()),
        path: vec![name.into()],
    }
}

fn enum_path() -> DefinitionPath {
    DefinitionPath {
        module: owner("data"),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Enum,
            name: "Maybe".into(),
            occurrence: 0,
        }],
    }
}

#[derive(Default)]
struct Counts {
    recipes: AtomicUsize,
    factories: AtomicUsize,
    callbacks: AtomicUsize,
}

fn representative(
    counts: Arc<Counts>,
    on_prepare: impl FnOnce(&SealedPreparation<'_>) -> NativeResult<()> + Send + 'static,
) -> NativeResult<ModuleAssembly> {
    let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default())?;
    set.add(ModuleDefinition::new(owner("data"), move |module| {
        counts.recipes.fetch_add(1, Ordering::SeqCst);
        let maybe = module.define_type("Maybe", TypeDefKind::Enum, 1, |record, _| {
            record.variants.extend([
                VariantDef {
                    name: "Some".into(),
                    payload: vec![record.generic_params[0].as_type()],
                    reports_failure: false,
                },
                VariantDef {
                    name: "None".into(),
                    payload: vec![],
                    reports_failure: false,
                },
            ]);
            Ok(())
        })?;
        module.define_function((
            FunctionDecl::new("none").returns(maybe.apply([Type::i32()])?),
            PendingBinding::implementation_with_send(move |preparation| {
                counts.factories.fetch_add(1, Ordering::SeqCst);
                // Symbolic members come from the exact sealed catalog supplied to
                // this factory, never StandardDeclarations or another global cache.
                let enumeration = preparation.catalog().type_reference(&enum_path())?;
                let none = enumeration.variant("None")?;
                let declaration = preparation.declaration();
                assert_eq!(none.definitions().id(), declaration.definitions().id());
                let result = Type::from_semantic(
                    declaration.records().function.return_type.clone(),
                    declaration.definitions(),
                )?;
                on_prepare(preparation)?;
                Ok(NativeBinding::new(
                    vec![],
                    Codec::Exact(result),
                    move |cx| {
                        counts.callbacks.fetch_add(1, Ordering::SeqCst);
                        cx.allocate_enum(&cx.result_type_argument()?, &none, vec![])
                    },
                ))
            }),
        ))?;
        module.define_constructor("String", NativeTypeConstructor::String)?;
        module.inherent(Type::scalar(BuiltinType::String), |methods| {
            methods.define(
                MethodDecl::instance("len")
                    .returns(Type::u64())
                    .implementation_send(NativeBinding::primitive(
                        NativePrimitive::StringByteLength,
                    )),
            )?;
            Ok(())
        })?;
        let interface = module.define_trait("Answer", 0, |_, _| Ok(()))?;
        module.define_trait_default(
            &interface,
            MethodDecl::static_method("answer")
                .returns(Type::u64())
                .implementation_send(NativeBinding::new(
                    vec![],
                    Codec::Exact(Type::u64()),
                    |_| Ok(Value::U64(42)),
                )),
        )?;
        Ok(())
    }))?;
    set.finish()
}

#[test]
fn enum_short_names_reject_ambiguity_without_rejecting_canonical_owners() {
    let cache = bundle(|| {
        let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default())?;
        for name in ["data", "other"] {
            set.add(ModuleDefinition::new(owner(name), |module| {
                module.define_type("Maybe", TypeDefKind::Enum, 0, |_, _| Ok(()))?;
                Ok(())
            }))?;
        }
        set.finish()
    });
    let semantic = cache.semantic().unwrap();
    assert!(semantic.enumeration_named("Maybe").is_err());
    let mut path = enum_path();
    let first = semantic.enumeration(&path).unwrap().id();
    path.module = owner("other");
    assert_ne!(first, semantic.enumeration(&path).unwrap().id());
    cache.modules().unwrap();
}

#[test]
fn semantic_first_then_concurrent_executable_use_prepares_once_on_another_thread() {
    let counts = Arc::new(Counts::default());
    let recipe_counts = counts.clone();
    let author_thread = thread::current().id();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let cache = bundle(move || {
        representative(recipe_counts, move |_| {
            assert_ne!(thread::current().id(), author_thread);
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(WAIT).unwrap();
            Ok(())
        })
    });
    let semantic = cache.semantic().unwrap();
    let before = semantic
        .snapshot()
        .modules()
        .iter()
        .map(|module| {
            module
                .records()
                .compiler_input_bytes(module.definitions(), &Default::default())
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(counts.recipes.load(Ordering::SeqCst), 1);
    assert_eq!(counts.factories.load(Ordering::SeqCst), 0);
    assert!(Arc::ptr_eq(&semantic, &cache.semantic().unwrap()));
    let enumeration = semantic.enumeration(&enum_path()).unwrap();
    assert_eq!(
        semantic.enumeration_named("Maybe").unwrap().id(),
        enumeration.id()
    );
    assert!(semantic.enumeration_named("Absent").is_err());
    let variant = semantic.variant(&enum_path(), "None").unwrap();
    let metadata = &semantic.snapshot().modules()[0];
    assert_eq!(enumeration.definitions().id(), metadata.definitions().id());
    assert_eq!(variant.definitions().id(), metadata.definitions().id());
    assert_eq!(variant.id(), enumeration.variant("None").unwrap().id());
    assert!(semantic.variant(&enum_path(), "Absent").is_err());
    let mut absent = enum_path();
    absent.module = owner("unprovided");
    assert!(semantic.enumeration(&absent).is_err());

    let barrier = Arc::new(Barrier::new(5));
    let (result_tx, result_rx) = mpsc::channel();
    let workers = (0..4)
        .map(|_| {
            let cache = cache.clone();
            let barrier = barrier.clone();
            let result_tx = result_tx.clone();
            thread::spawn(move || {
                barrier.wait();
                result_tx.send(cache.modules()).unwrap();
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    entered_rx.recv_timeout(WAIT).unwrap();
    // Readers must not wait for executable preparation, even while its factory
    // is paused. A bounded observer makes an accidental lock cycle fail the test.
    let reader_cache = cache.clone();
    let (reader_tx, reader_rx) = mpsc::channel();
    let reader = thread::spawn(move || reader_tx.send(reader_cache.semantic()).unwrap());
    let during = reader_rx.recv_timeout(WAIT).unwrap().unwrap();
    assert!(Arc::ptr_eq(&semantic, &during));
    release_tx.send(()).unwrap();
    let results = (0..4)
        .map(|_| result_rx.recv_timeout(WAIT).unwrap().unwrap())
        .collect::<Vec<_>>();
    for worker in workers {
        worker.join().unwrap();
    }
    reader.join().unwrap();
    for modules in &results {
        assert!(Arc::ptr_eq(&results[0], modules));
    }
    assert!(Arc::ptr_eq(&results[0], &cache.modules().unwrap()));
    assert_eq!(counts.recipes.load(Ordering::SeqCst), 1);
    assert_eq!(counts.factories.load(Ordering::SeqCst), 1);
    assert_eq!(counts.callbacks.load(Ordering::SeqCst), 0);
    for (before, module) in before.iter().zip(results[0].iter()) {
        assert_eq!(
            *before,
            module
                .declaration()
                .compiler_input_bytes(module.definitions(), &Default::default())
                .unwrap()
        );
    }
    // Cached bindings and symbolic enum handles are not runtime-local roots.
    for _ in 0..2 {
        let mut runtime = Runtime::default();
        NativeModule::install_all(&results[0], &mut runtime).unwrap();
        assert_eq!(runtime.gc().active_roots(), 0);
    }
}

#[test]
fn retaining_semantics_does_not_retain_unprepared_factories() {
    let counts = Arc::new(Counts::default());
    let recipe_counts = counts.clone();
    let pending_capture = Arc::new(());
    let lifetime = Arc::downgrade(&pending_capture);
    let cache = bundle(move || {
        representative(recipe_counts, move |_| {
            drop(pending_capture);
            Ok(())
        })
    });
    let semantic = cache.semantic().unwrap();
    assert!(lifetime.upgrade().is_some());
    drop(cache);
    assert!(lifetime.upgrade().is_none());
    assert!(semantic.variant(&enum_path(), "Some").is_ok());
    assert_eq!(counts.factories.load(Ordering::SeqCst), 0);
}
