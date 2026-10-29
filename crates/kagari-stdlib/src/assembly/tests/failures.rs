//! Failed attempts publish no module prefix and never retry a consumed recipe.
use crate::assembly::tests::{Counts, TestBundle, WAIT, bundle, enum_path, owner, representative};
use kagari_runtime::{
    error::RuntimeError,
    native::{
        assembly::{ModuleAssembly, ModuleDefinition, ModuleSetBuilder, authoring::PendingBinding},
        binding::{Codec, NativeBinding, NativeResult},
        catalog::DeclarationCatalog,
        declarations::FunctionDecl,
        module::NativeModule,
        types::Type,
    },
    value::Value,
};
use std::{
    array,
    sync::{
        Arc, OnceLock, Weak,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
};

fn modules_on_worker(cache: Arc<TestBundle>) -> NativeResult<Arc<[NativeModule]>> {
    let (tx, rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        let result = cache.modules();
        if let Err(error) = &result {
            assert_eq!(*error, cache.modules().unwrap_err());
        }
        tx.send(result).unwrap();
    });
    let result = rx
        .recv_timeout(WAIT)
        .expect("initialization must not deadlock");
    worker.join().unwrap();
    result
}

fn batch(
    calls: Arc<[AtomicUsize; 3]>,
    captures: [Arc<()>; 3],
    fail: impl FnOnce() -> NativeResult<()> + Send + 'static,
) -> NativeResult<ModuleAssembly> {
    let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default())?;
    let mut fail = Some(fail);
    for (index, capture) in captures.into_iter().enumerate() {
        let calls = calls.clone();
        let fail = if index == 1 { fail.take() } else { None };
        set.add(ModuleDefinition::new(
            owner(&format!("part{index}")),
            move |module| {
                module.define_function((
                    FunctionDecl::new("run"),
                    PendingBinding::implementation_with_send(move |_| {
                        calls[index].fetch_add(1, Ordering::SeqCst);
                        if let Some(fail) = fail {
                            fail()?;
                        }
                        Ok(unsafe {
                            NativeBinding::new_transport(
                                vec![],
                                Codec::Exact(Type::unit()),
                                move |_| {
                                    let _retained = &capture;
                                    Ok(Value::Unit)
                                },
                            )
                        })
                    }),
                ))?;
                Ok(())
            },
        ))?;
    }
    set.finish()
}

#[test]
fn factory_error_or_panic_is_terminal_and_drops_completed_and_remaining_work() {
    for panic in [false, true] {
        let recipes = Arc::new(AtomicUsize::new(0));
        let recipe_count = recipes.clone();
        let calls = Arc::new(array::from_fn(|_| AtomicUsize::new(0)));
        let factory_calls = calls.clone();
        let captures: [Arc<()>; 3] = array::from_fn(|_| Arc::new(()));
        let lifetimes = captures.each_ref().map(Arc::downgrade);
        let cache = bundle(move || {
            recipe_count.fetch_add(1, Ordering::SeqCst);
            batch(factory_calls, captures, move || {
                if panic {
                    panic!("factory panic");
                }
                Err(RuntimeError::metadata_conflict("factory rejected"))
            })
        });
        let semantic = cache.semantic().unwrap();
        assert!(
            lifetimes
                .iter()
                .all(|lifetime| lifetime.upgrade().is_some())
        );
        let error = modules_on_worker(cache.clone()).unwrap_err();
        assert!(
            error.message().contains(if panic {
                "preparation: initializer panicked"
            } else {
                "factory rejected"
            }),
            "{error}"
        );
        assert_eq!(error, modules_on_worker(cache.clone()).unwrap_err());
        assert!(Arc::ptr_eq(&semantic, &cache.semantic().unwrap()));
        assert_eq!(recipes.load(Ordering::SeqCst), 1);
        assert_eq!(
            calls.each_ref().map(|calls| calls.load(Ordering::SeqCst)),
            [1, 1, 0]
        );
        assert!(
            lifetimes
                .iter()
                .all(|lifetime| lifetime.upgrade().is_none())
        );
    }
}

#[test]
fn collection_error_or_panic_is_cached_without_replaying_authoring() {
    for panic in [false, true] {
        let recipes = Arc::new(AtomicUsize::new(0));
        let attempts = recipes.clone();
        let cache = bundle(move || {
            attempts.fetch_add(1, Ordering::SeqCst);
            if panic {
                panic!("recipe panic");
            }
            Err(RuntimeError::metadata_conflict("recipe rejected"))
        });
        let error = modules_on_worker(cache.clone()).unwrap_err();
        assert!(
            error.message().contains(if panic {
                "collection: initializer panicked"
            } else {
                "recipe rejected"
            }),
            "{error}"
        );
        assert_eq!(error, cache.semantic().unwrap_err());
        assert_eq!(error, modules_on_worker(cache).unwrap_err());
        assert_eq!(recipes.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn recursive_collection_or_preparation_is_terminal_even_if_its_error_is_swallowed() {
    for (preparing, request_modules) in [(false, false), (false, true), (true, true)] {
        let target = Arc::new(OnceLock::<Weak<TestBundle>>::new());
        let recipe_target = target.clone();
        let counts = Arc::new(Counts::default());
        let recipe_counts = counts.clone();
        let cache = bundle(move || {
            let reenter = move || {
                let cache = recipe_target.get().unwrap().upgrade().unwrap();
                let error = if request_modules {
                    cache.modules().unwrap_err()
                } else {
                    cache.semantic().unwrap_err()
                };
                assert!(error.message().contains("recursive bundle initialization"));
                // Deliberately swallow it: the outer attempt must still fail closed.
            };
            if preparing {
                representative(recipe_counts, move |_| {
                    reenter();
                    Ok(())
                })
            } else {
                reenter();
                representative(recipe_counts, |_| Ok(()))
            }
        });
        target.set(Arc::downgrade(&cache)).unwrap();
        let error = modules_on_worker(cache.clone()).unwrap_err();
        assert!(
            error.message().contains("recursive bundle initialization"),
            "{error}"
        );
        assert!(error.message().contains(if preparing {
            "preparation:"
        } else {
            "collection:"
        }));
        assert_eq!(error, modules_on_worker(cache.clone()).unwrap_err());
        if preparing {
            assert!(
                cache
                    .semantic()
                    .unwrap()
                    .enumeration(&enum_path())
                    .and_then(|enumeration| enumeration.variant("None"))
                    .is_ok()
            );
        } else {
            assert_eq!(error, cache.semantic().unwrap_err());
        }
        assert_eq!(counts.recipes.load(Ordering::SeqCst), 1);
        assert_eq!(
            counts.factories.load(Ordering::SeqCst),
            usize::from(preparing)
        );
    }
}

#[test]
fn preparation_can_read_published_semantics_without_reentering_collection() {
    let target = Arc::new(OnceLock::<Weak<TestBundle>>::new());
    let factory_target = target.clone();
    let cache = bundle(move || {
        representative(Arc::new(Counts::default()), move |preparation| {
            let cache = factory_target.get().unwrap().upgrade().unwrap();
            let semantic = cache.semantic()?;
            let handle = semantic
                .enumeration(&enum_path())
                .and_then(|enumeration| enumeration.variant("None"))?;
            assert_eq!(
                handle.definitions().id(),
                preparation.declaration().definitions().id()
            );
            Ok(())
        })
    });
    target.set(Arc::downgrade(&cache)).unwrap();
    modules_on_worker(cache).unwrap();
}

#[test]
fn independent_bundle_with_the_same_label_is_not_recursive_initialization() {
    let other = bundle(|| representative(Arc::new(Counts::default()), |_| Ok(())));
    let cache = bundle(move || {
        other.semantic()?;
        representative(Arc::new(Counts::default()), |_| Ok(()))
    });
    modules_on_worker(cache).unwrap();
}
