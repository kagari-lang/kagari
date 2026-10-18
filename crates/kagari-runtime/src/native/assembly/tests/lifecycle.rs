//! A failed preparation batch owns cleanup, but cannot roll back factory side effects.
use super::owner;
use crate::native::{
    adapter::{Explicit, NativeAdapter},
    assembly::{ModuleDefinition, ModuleSetBuilder, authoring::PendingBinding, conflict},
    binding::{Codec, NativeBinding, NativeResult},
    catalog::DeclarationCatalog,
    declarations::FunctionDecl,
    types::Type,
};
use kagari_common::cancellation::CancellationToken;
use kagari_types::declaration::FnDecl;
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

struct DropProbe(Arc<AtomicUsize>);

impl Drop for DropProbe {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

struct TrackedAdapter {
    probe: Arc<DropProbe>,
    events: Rc<RefCell<Vec<&'static str>>>,
}

impl NativeAdapter<Explicit, ()> for TrackedAdapter {
    fn prepare(self, _: &FnDecl, _: &DeclarationCatalog) -> NativeResult<NativeBinding> {
        self.events.borrow_mut().push("first adapter prepared");
        let probe = self.probe;
        Ok(NativeBinding::new(
            vec![],
            Codec::Exact(Type::unit()),
            move |_| {
                let _keep_alive = &probe;
                panic!("preparation must not execute callbacks")
            },
        ))
    }
}

#[test]
fn later_factory_failure_discards_prepared_modules_and_remaining_factories_not_snapshot() {
    let events = Rc::new(RefCell::new(vec![]));
    let first_drops = Arc::new(AtomicUsize::new(0));
    let failed_drops = Arc::new(AtomicUsize::new(0));
    let skipped_drops = Arc::new(AtomicUsize::new(0));
    let first = Arc::new(DropProbe(first_drops.clone()));
    let failed = Arc::new(DropProbe(failed_drops.clone()));
    let skipped = Arc::new(DropProbe(skipped_drops.clone()));
    let first_weak = Arc::downgrade(&first);
    let failed_weak = Arc::downgrade(&failed);
    let skipped_weak = Arc::downgrade(&skipped);
    let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
    let first_events = events.clone();
    set.add(ModuleDefinition::new(owner("prepared"), move |module| {
        module.define_function((
            FunctionDecl::new("run"),
            PendingBinding::implementation_with(move |_| {
                first_events.borrow_mut().push("first factory");
                Ok(TrackedAdapter {
                    probe: first,
                    events: first_events,
                })
            }),
        ))?;
        Ok(())
    }))
    .unwrap();
    let failed_events = events.clone();
    let prepared_binding = first_weak.clone();
    set.add(ModuleDefinition::new(owner("failed"), move |module| {
        module.define_function((
            FunctionDecl::new("run"),
            PendingBinding::implementation_with(move |_| {
                assert_eq!(
                    *failed_events.borrow(),
                    ["first factory", "first adapter prepared"]
                );
                // Reaching this module's factory means the preceding module passed
                // NativeModule::checked; its callback is still owned by the batch.
                assert!(prepared_binding.upgrade().is_some());
                assert_eq!(failed.0.load(Ordering::SeqCst), 0);
                failed_events.borrow_mut().push("second factory failed");
                Err::<TrackedAdapter, _>(conflict("deliberate second factory failure"))
            }),
        ))?;
        Ok(())
    }))
    .unwrap();
    let skipped_events = events.clone();
    set.add(ModuleDefinition::new(owner("skipped"), move |module| {
        module.define_function((
            FunctionDecl::new("run"),
            PendingBinding::implementation_with(move |_| {
                skipped_events
                    .borrow_mut()
                    .push("third factory must not run");
                Ok(TrackedAdapter {
                    probe: skipped,
                    events: skipped_events,
                })
            }),
        ))?;
        Ok(())
    }))
    .unwrap();
    let assembly = set.finish().unwrap();
    let snapshot = assembly.declarations();
    let retained = snapshot.clone();
    assert!(events.borrow().is_empty());
    assert_eq!(snapshot.modules().len(), 3);
    let bytes = snapshot
        .modules()
        .iter()
        .map(|metadata| {
            metadata
                .records()
                .compiler_input_bytes(metadata.definitions(), &CancellationToken::default())
                .unwrap()
        })
        .collect::<Vec<_>>();
    let error = assembly
        .into_modules()
        .expect_err("failure must not expose the prepared prefix as a partial batch");
    assert!(
        error
            .to_string()
            .contains("deliberate second factory failure"),
        "{error}"
    );
    assert_eq!(
        *events.borrow(),
        [
            "first factory",
            "first adapter prepared",
            "second factory failed"
        ],
        "completed factory side effects remain, but later factories do not execute"
    );
    for (probe, drops) in [
        (first_weak, first_drops),
        (failed_weak, failed_drops),
        (skipped_weak, skipped_drops),
    ] {
        assert!(
            probe.upgrade().is_none(),
            "failed batch must not retain callback/factory resources"
        );
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }
    drop(snapshot);
    // A retained declaration product remains usable without recipes, bindings or
    // a runtime. Reimporting exercises semantic validation, not only Arc retention.
    for (metadata, before) in retained.modules().iter().zip(bytes) {
        assert_eq!(
            metadata
                .records()
                .compiler_input_bytes(metadata.definitions(), &CancellationToken::default())
                .unwrap(),
            before
        );
    }
    let imported = DeclarationCatalog::from_declarations(retained.modules()).unwrap();
    assert_eq!(imported.declarations.values().count(), 3);
    assert_eq!(retained.catalog().declarations.values().count(), 3);
}
