use crate::{
    declarations::StandardDeclarations,
    tests::{
        execution::native_boundary::{compile, compile_program},
        support::bytecode::reload_bytecode,
    },
};

use kagari_runtime::{
    error::{RuntimeErrorKind, fault::EngineFault, reason::ErrorReason},
    module::entry::ProgramEntry,
    native::{
        binding::{Codec, NativeBinding},
        builder::ModuleBuilder,
        declarations::FunctionDecl,
        module::NativeModule,
        storage::{NativePayload, NativeStorage},
    },
    value::carriers::RootedValue,
};

use kagari_gc::{
    error::GcError,
    heap::Phase,
    object::{GcObject, ObjectCharge},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[derive(Debug, Default)]
struct Faults {
    trace: AtomicBool,
    drop: AtomicBool,
    drops: AtomicUsize,
}

#[derive(Debug)]
struct Payload(Arc<Faults>);

impl NativePayload for Payload {
    fn registration_storage() -> NativeStorage {
        NativeStorage::payload::<Self>()
    }
}

// SAFETY: only inert Rust fault/counter state, with no managed peers or backing.
// The trace flag deliberately breaks the provider contract to test quarantine.
unsafe impl GcObject for Payload {
    type ScanState = ();
    const SCAN_KIND: ScanKind = ScanKind::Bounded { max_work: 1 };

    fn begin_scan(&self) {}

    fn scan(
        &self,
        _: &mut (),
        _: &mut ScanBudget,
        _: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        assert!(!self.0.trace.load(Ordering::SeqCst), "fixture trace panic");
        Ok(ScanStatus::Complete)
    }

    fn charge(&self) -> ObjectCharge {
        ObjectCharge { units: 0, bytes: 0 }
    }
}

impl Drop for Payload {
    fn drop(&mut self) {
        self.0.drops.fetch_add(1, Ordering::SeqCst);
        assert!(
            !self.0.drop.load(Ordering::SeqCst),
            "fixture destructor panic"
        );
    }
}

fn module(faults: &Arc<Faults>) -> NativeModule {
    let mut builder = ModuleBuilder::new(
        "fixture::payload",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    let faults = faults.clone();
    let mut declaration = builder.define_type("Payload").unwrap();
    declaration
        .native_storage(NativeStorage::payload::<Payload>())
        .unwrap();
    let payload = declaration.finish().unwrap();
    let new = builder
        .define_function(FunctionDecl::new("new").returns(payload.apply([]).unwrap()))
        .unwrap();
    builder
        .bind(
            new,
            NativeBinding::new(Vec::<Codec>::new(), payload.codec(), move |cx| {
                cx.allocate_result_with(&[], |native| {
                    // SAFETY: initialized Rust-only provider; its complete scanner
                    // runs during admission before the test enables injected faults.
                    unsafe { native.scope().alloc_with(|_| Ok(Payload(faults.clone()))) }
                })
            }),
        )
        .unwrap();
    builder.finish().unwrap()
}

#[test]
fn tracing_panic_quarantines_without_sweeping_a_partial_graph() {
    let faults = Arc::new(Faults::default());
    let native = module(&faults);
    let source = include_str!("../../../../../tests/fixtures/native/fault-payload.kgr");
    let (mut vm, loaded) = compile(source, Some(&native));
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    let value = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .unwrap()
        .return_value;
    let root = value;
    reload_bytecode(
        &mut vm,
        &_loaded_program,
        Some("boundary".into()),
        compile_program(source, Some(&native)),
    )
    .unwrap();
    let before = vm.gc_stats();
    let counters = vm.resources().counters();
    faults.trace.store(true, Ordering::SeqCst);
    let error = vm.collect_garbage().unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
    assert!(matches!(
        error.reason(),
        ErrorReason::EngineFault(EngineFault::ManagedCollectionFailed(GcError::Provider(
            "scanner or charge panicked"
        )))
    ));
    assert!(vm.is_quarantined());
    let after = vm.gc_stats();
    assert_eq!(after.phase, Phase::Faulted);
    assert_eq!(after.objects, before.objects);
    assert_eq!(after.provider_units, before.provider_units);
    assert_eq!(after.backing_bytes, before.backing_bytes);
    assert_eq!(after.indirect_bytes, before.indirect_bytes);
    assert_eq!(vm.resources().counters(), counters);
    assert!(
        loaded
            .members()
            .all(|member| vm.modules().loaded(member.key()).is_some())
    );
    assert_eq!(faults.drops.load(Ordering::SeqCst), 0);
    // Release notices may retire already-dropped preparation leases even though
    // the retained payload's own Host root survives the failed marking cycle.
    assert!(after.root_registrations >= 1);
    assert!(after.root_registrations <= before.root_registrations);
    faults.trace.store(false, Ordering::SeqCst);
    drop(vm);
    assert_eq!(faults.drops.load(Ordering::SeqCst), 1);
    drop(root);
}

#[test]
fn destructor_panics_dispose_each_detached_payload_once_and_quarantine() {
    let faults = Arc::new(Faults::default());
    let (mut vm, loaded) = compile(
        include_str!("../../../../../tests/fixtures/native/fault-destructors.kgr"),
        Some(&module(&faults)),
    );
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    let values = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .unwrap()
        .return_value;
    drop(values);
    let before = vm.gc_stats();
    faults.drop.store(true, Ordering::SeqCst);
    let error = vm.collect_garbage().unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
    assert!(matches!(
        error.reason(),
        ErrorReason::EngineFault(EngineFault::ManagedCollectionFailed(GcError::Provider(
            "object destructor panicked"
        )))
    ));
    assert!(vm.is_quarantined());
    assert_eq!(faults.drops.load(Ordering::SeqCst), 2);
    assert_eq!(vm.gc_stats().phase, Phase::Faulted);
    assert!(vm.gc_stats().objects <= before.objects - 2);
    assert_eq!(
        vm.collect_garbage().unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
    drop(vm);
    assert_eq!(faults.drops.load(Ordering::SeqCst), 2);
}

#[test]
fn clearing_reloaded_static_cells_disposes_payloads_once_even_when_destructors_panic() {
    let faults = Arc::new(Faults::default());
    let native = module(&faults);
    let source = include_str!("../../../../../tests/fixtures/native/fault-statics.kgr");
    let (mut vm, old) = compile(source, Some(&native));
    let _old_program = old;
    let old = vm.select_program(&_old_program).unwrap();
    drop(
        vm.execute_module(&old, ProgramEntry::Main, vm.execution_options())
            .unwrap(),
    );
    let staged = vm
        .stage_reload_program(
            &_old_program,
            Some("boundary".into()),
            compile_program(source, Some(&native)),
        )
        .unwrap();
    let current = staged.module().clone();
    vm.publish_staged_reload(staged).unwrap();
    let _current_program = _old_program.clone();
    vm.collect_garbage().unwrap();
    assert_eq!(faults.drops.load(Ordering::SeqCst), 0);
    drop(
        vm.execute_module(&current, ProgramEntry::Main, vm.execution_options())
            .unwrap(),
    );
    vm.execute_typed::<_, ()>(&_current_program, "clear", (), &Default::default())
        .unwrap();
    faults.drop.store(true, Ordering::SeqCst);
    let error = vm.collect_garbage().unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
    assert!(matches!(
        error.reason(),
        ErrorReason::EngineFault(EngineFault::ManagedCollectionFailed(GcError::Provider(
            "object destructor panicked"
        )))
    ));
    assert!(vm.is_quarantined());
    assert_eq!(faults.drops.load(Ordering::SeqCst), 2);
    // Faulted heaps may retain unswept storage until shutdown. Both native
    // payloads died; the current live installation remains logically published.
    assert_eq!(vm.gc_stats().phase, Phase::Faulted);
    assert!(vm.modules().loaded(old.key()).is_none());
    assert!(vm.modules().loaded(current.key()).is_some());
    drop(vm);
    assert_eq!(faults.drops.load(Ordering::SeqCst), 2);
}

#[test]
fn roots_do_not_keep_heap_payloads_alive_after_teardown() {
    let faults = Arc::new(Faults::default());
    let (mut vm, loaded) = compile(
        include_str!("../../../../../tests/fixtures/native/fault-payload.kgr"),
        Some(&module(&faults)),
    );
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    let mut session = vm.begin_execution(&loaded, vm.execution_options()).unwrap();
    let session_options = session.runtime().execution_options();
    let value = session
        .runtime_mut()
        .execute_module(&loaded, ProgramEntry::Main, session_options.clone())
        .unwrap()
        .return_value;
    let root = value;
    let array_root = session
        .runtime_mut()
        .alloc_array(
            &loaded,
            Ty::Builtin(BuiltinType::I32),
            &[RootedValue::from(7i32)],
        )
        .unwrap();
    assert_eq!(faults.drops.load(Ordering::SeqCst), 0);
    drop(session);
    assert!(vm.execution_root().is_none());
    drop(vm);
    assert_eq!(faults.drops.load(Ordering::SeqCst), 1);
    std::thread::spawn(move || {
        drop(root);
        drop(array_root);
    })
    .join()
    .unwrap();
    assert_eq!(faults.drops.load(Ordering::SeqCst), 1);
}
