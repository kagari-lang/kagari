//! Transfer only retained preparation state, never local authoring or live adapters.
use crate::{
    Runtime,
    native::{
        adapter::NativeAdapter,
        assembly::{
            ModuleDefinition, ModuleSetBuilder, SendModuleAssembly,
            authoring::{PendingBinding, SealedPreparation},
            conflict,
        },
        binding::{Codec, NativeBinding, NativeResult},
        catalog::DeclarationCatalog,
        declarations::{FunctionDecl, MethodDecl},
        storage::{NativePayload, NativeStorage},
        types::Type,
    },
    value::Value,
};
use kagari_common::identity::{ModuleIdentity, PackageId, metadata::DefinitionMetadata};
use kagari_types::declaration::{FnDecl, TypeDefKind, native::NativeStorageLayout};
use std::{
    cell::Cell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};

fn owner() -> ModuleIdentity {
    ModuleIdentity {
        package: PackageId("transfer".into()),
        path: vec!["entries".into()],
    }
}

fn unit_binding(parameters: usize) -> NativeBinding {
    NativeBinding::new(
        vec![Codec::Value; parameters],
        Codec::Exact(Type::unit()),
        |_| Ok(Value::Unit),
    )
}

// Cell proves no Sync bound; Rc marker/result types prove only the stored adapter
// is constrained by implementation_send, not NativeAdapter's inference parameters.
struct TransferAdapter {
    state: Cell<usize>,
    prepares: Arc<AtomicUsize>,
}

impl NativeAdapter<Rc<()>, Rc<()>> for TransferAdapter {
    fn prepare(self, declaration: &FnDecl, _: &DeclarationCatalog) -> NativeResult<NativeBinding> {
        self.state.set(self.state.get() + 1);
        self.prepares.fetch_add(1, Ordering::SeqCst);
        Ok(unit_binding(declaration.params.len()))
    }
}

struct LocalAdapter {
    state: Rc<Cell<usize>>,
    prepares: Arc<AtomicUsize>,
}

impl NativeAdapter<Rc<()>, Rc<()>> for LocalAdapter {
    fn prepare(self, declaration: &FnDecl, _: &DeclarationCatalog) -> NativeResult<NativeBinding> {
        self.state.set(self.state.get() + 1);
        self.prepares.fetch_add(1, Ordering::SeqCst);
        assert!(!declaration.name.starts_with("__default_"));
        assert!(declaration.generic_params.is_empty());
        Ok(unit_binding(declaration.params.len()))
    }
}

#[derive(Debug)]
struct Payload;

impl NativePayload for Payload {
    fn trace<'payload>(&'payload self, _: &mut dyn FnMut(&'payload Value)) {}

    fn units(&self) -> usize {
        0
    }
}

#[test]
fn transfer_drops_local_authoring_and_prepares_once_on_destination_thread() {
    fn assert_send<T: Send>() {}
    assert_send::<SendModuleAssembly>();
    let factories = Arc::new(AtomicUsize::new(0));
    let prepares = Arc::new(AtomicUsize::new(0));
    let local = Rc::new(Cell::new(0));
    let recipe_local = local.clone();
    let recipe_factories = factories.clone();
    let recipe_prepares = prepares.clone();
    let source_thread = thread::current().id();
    let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
    set.add(ModuleDefinition::new(owner(), move |module| {
        recipe_local.set(recipe_local.get() + 1);
        let payload = module.define_type(
            "Payload",
            TypeDefKind::NativeStorage(NativeStorageLayout::Opaque),
            0,
            |_, _| Ok(()),
        )?;
        module.bind_storage(&payload, NativeStorage::payload::<Payload>())?;
        module.inherent(payload.apply([])?, |methods| {
            methods.define(MethodDecl::static_method("direct").implementation_send(
                TransferAdapter {
                    state: Cell::new(0),
                    prepares: recipe_prepares.clone(),
                },
            ))?;
            let configured = recipe_local.clone();
            methods.define(
                MethodDecl::static_method("configured_direct")
                    .configure(move |_| {
                        configured.set(configured.get() + 1);
                        Ok(())
                    })
                    .implementation_send(TransferAdapter {
                        state: Cell::new(0),
                        prepares: recipe_prepares.clone(),
                    }),
            )?;
            let factory_count = recipe_factories.clone();
            let prepare_count = recipe_prepares.clone();
            // Capturing Cell deliberately makes this factory Send but not Sync.
            let factory_state = Cell::new(0);
            methods.define(
                MethodDecl::static_method("factory").implementation_with_send(move |view| {
                    assert_ne!(thread::current().id(), source_thread);
                    assert_eq!(view.declaration().records().function.params.len(), 0);
                    factory_state.set(factory_state.get() + 1);
                    factory_count.fetch_add(1, Ordering::SeqCst);
                    Ok(LocalAdapter {
                        state: Rc::new(Cell::new(0)),
                        prepares: prepare_count,
                    })
                }),
            )?;
            methods.define(
                MethodDecl::static_method("configured_factory")
                    .configure(move |_| {
                        recipe_local.set(recipe_local.get() + 1);
                        Ok(())
                    })
                    .implementation_with_send(move |_| {
                        assert_ne!(thread::current().id(), source_thread);
                        recipe_factories.fetch_add(1, Ordering::SeqCst);
                        Ok(LocalAdapter {
                            state: Rc::new(Cell::new(0)),
                            prepares: recipe_prepares,
                        })
                    }),
            )?;
            Ok(())
        })
    }))
    .unwrap();
    let assembly = set.finish().unwrap();
    assert_eq!(local.get(), 3);
    assert_eq!(
        Rc::strong_count(&local),
        1,
        "recipes/configuration have been consumed"
    );
    let snapshot = assembly.declarations();
    let transferred = assembly.try_into_send().unwrap();
    assert_eq!(factories.load(Ordering::SeqCst), 0);
    assert_eq!(prepares.load(Ordering::SeqCst), 0);
    let before = snapshot.modules()[0]
        .records()
        .compiler_input_bytes(snapshot.modules()[0].definitions(), &Default::default())
        .unwrap();
    let transferred_snapshot = transferred.declarations();
    assert_eq!(
        transferred_snapshot.modules()[0]
            .records()
            .compiler_input_bytes(
                transferred_snapshot.modules()[0].definitions(),
                &Default::default(),
            )
            .unwrap(),
        before
    );
    thread::spawn(move || {
        let modules = transferred.into_modules().unwrap();
        assert_eq!(modules.len(), 1);
        let module = modules.into_iter().next().unwrap();
        assert_eq!(
            module
                .declaration()
                .compiler_input_bytes(module.definitions(), &Default::default())
                .unwrap(),
            before
        );
        // Installation checks that transfer preserved the native storage association.
        module.install(&mut Runtime::default()).unwrap();
    })
    .join()
    .unwrap();
    assert_eq!(factories.load(Ordering::SeqCst), 2);
    assert_eq!(prepares.load(Ordering::SeqCst), 4);
    assert_eq!(local.get(), 3);
}

#[test]
fn local_pending_rejection_reports_canonical_target_without_preparation() {
    for direct in [false, true] {
        let factories = Rc::new(Cell::new(0));
        let prepares = Arc::new(AtomicUsize::new(0));
        let factory_count = factories.clone();
        let prepare_count = prepares.clone();
        let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
        set.add(ModuleDefinition::new(owner(), move |module| {
            let pending = if direct {
                PendingBinding::implementation(LocalAdapter {
                    state: Rc::new(Cell::new(0)),
                    prepares: prepare_count,
                })
            } else {
                PendingBinding::implementation_with(move |_| {
                    factory_count.set(factory_count.get() + 1);
                    Ok(LocalAdapter {
                        state: Rc::new(Cell::new(0)),
                        prepares: prepare_count,
                    })
                })
            };
            module.define_function((FunctionDecl::new("local"), pending))?;
            Ok(())
        }))
        .unwrap();
        let assembly = set.finish().unwrap();
        let snapshot = assembly.declarations();
        let error = match assembly.try_into_send() {
            Ok(_) => panic!("local entry cannot transfer"),
            Err(error) => error,
        };
        assert!(
            error.message().contains("transfer::entries::local"),
            "{error}"
        );
        assert_eq!(factories.get(), 0);
        assert_eq!(prepares.load(Ordering::SeqCst), 0);
        assert_eq!(snapshot.modules()[0].records().functions[0].name, "local");
    }
}

#[test]
fn default_wrapper_preserves_local_or_transferable_category_and_authored_signature() {
    for transferable in [false, true] {
        let factories = Arc::new(AtomicUsize::new(0));
        let prepares = Arc::new(AtomicUsize::new(0));
        let factory_count = factories.clone();
        let prepare_count = prepares.clone();
        let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
        set.add(ModuleDefinition::new(owner(), move |module| {
            let interface = module.define_trait("Run", 0, |_, _| Ok(()))?;
            let factory = move |view: &SealedPreparation<'_>| {
                factory_count.fetch_add(1, Ordering::SeqCst);
                assert_eq!(
                    view.declaration().records().function.generic_params.len(),
                    1
                );
                Ok(LocalAdapter {
                    state: Rc::new(Cell::new(0)),
                    prepares: prepare_count,
                })
            };
            let declaration = MethodDecl::instance("run");
            let definition = if transferable {
                declaration.implementation_with_send(factory)
            } else {
                declaration.implementation_with(factory)
            };
            module.define_trait_default(&interface, definition)?;
            Ok(())
        }))
        .unwrap();
        let assembly = set.finish().unwrap();
        let snapshot = assembly.declarations();
        assert_eq!(factories.load(Ordering::SeqCst), 0);
        if transferable {
            let transferred = assembly.try_into_send().unwrap();
            assert_eq!(factories.load(Ordering::SeqCst), 0);
            thread::spawn(move || {
                let modules = transferred.into_modules().unwrap();
                assert_eq!(
                    modules[0]
                        .declaration()
                        .compiler_input_bytes(modules[0].definitions(), &Default::default())
                        .unwrap(),
                    snapshot.modules()[0]
                        .records()
                        .compiler_input_bytes(
                            snapshot.modules()[0].definitions(),
                            &Default::default()
                        )
                        .unwrap(),
                );
            })
            .join()
            .unwrap();
            assert_eq!(factories.load(Ordering::SeqCst), 1);
            assert_eq!(prepares.load(Ordering::SeqCst), 1);
        } else {
            let error = match assembly.try_into_send() {
                Ok(_) => panic!("default wrapper upgraded a local entry"),
                Err(error) => error,
            };
            assert!(
                error
                    .message()
                    .contains("transfer::entries::__default_3_Run_run"),
                "{error}"
            );
            assert_eq!(factories.load(Ordering::SeqCst), 0);
            assert_eq!(prepares.load(Ordering::SeqCst), 0);
        }
    }
}

#[test]
fn transferable_default_still_rejects_foreign_token_owners() {
    let factories = Arc::new(AtomicUsize::new(0));
    let factory_count = factories.clone();
    let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
    set.add(ModuleDefinition::new(owner(), move |module| {
        module.define_function((
            FunctionDecl::new("foreign"),
            PendingBinding::implementation_send(unit_binding(0)),
        ))?;
        let interface = module.define_trait("Run", 0, |_, _| Ok(()))?;
        module.define_trait_default(
            &interface,
            MethodDecl::static_method("run").implementation_with_send(move |view| {
                factory_count.fetch_add(1, Ordering::SeqCst);
                let foreign = view
                    .catalog()
                    .declarations
                    .values()
                    .find(|declaration| declaration.function.name == "foreign")
                    .unwrap()
                    .clone();
                let foreign = DefinitionMetadata::checked(
                    view.catalog().definitions(),
                    foreign,
                    &Default::default(),
                )
                .map_err(conflict)?;
                Ok(NativeBinding::declared(&foreign, |_, (): ()| Ok(())))
            }),
        )?;
        Ok(())
    }))
    .unwrap();
    let transferred = set.finish().unwrap().try_into_send().unwrap();
    assert_eq!(factories.load(Ordering::SeqCst), 0);
    thread::spawn(move || {
        let error = transferred.into_modules().expect_err("foreign token owner");
        assert!(
            error
                .message()
                .contains("foreign native default requirement owner"),
            "{error}"
        );
    })
    .join()
    .unwrap();
    assert_eq!(factories.load(Ordering::SeqCst), 1);
}
