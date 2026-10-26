use crate::tests::native_boundary::{compile, compile_program};
use kagari_runtime::native::storage::scan::{NativeScanKind, ScanBudget, ScanStatus};
use kagari_runtime::{
    error::RuntimeErrorKind,
    module::entry::ProgramEntry,
    native::{
        binding::{Codec, NativeBinding},
        builder::ModuleBuilder,
        declarations::FunctionDecl,
        module::NativeModule,
        storage::{NativePayload, NativeStorage},
    },
    resource::HeapCharge,
    value::Value,
};
use kagari_stdlib::declarations::StandardDeclarations;
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

unsafe impl NativePayload for Payload {
    type ScanState = ();
    const SCAN_KIND: NativeScanKind = NativeScanKind::Indivisible;

    fn begin_scan(&self) {}

    fn scan<'payload>(
        &'payload self,
        _: &mut Self::ScanState,
        budget: &mut ScanBudget,
        _: &mut dyn FnMut(&'payload Value),
    ) -> ScanStatus {
        budget.charge(1);
        assert!(!self.0.trace.load(Ordering::SeqCst), "fixture trace panic");

        ScanStatus::Complete
    }

    fn charge(&self) -> HeapCharge {
        HeapCharge::new(0, 0)
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
        .native_storage(NativeStorage::new(move |_| Ok(Payload(faults.clone()))))
        .unwrap();
    let payload = declaration.finish().unwrap();
    let new = builder
        .define_function(FunctionDecl::new("new").returns(payload.apply([]).unwrap()))
        .unwrap();
    builder
        .bind(
            new,
            NativeBinding::new(Vec::<Codec>::new(), payload.codec(), |cx| {
                cx.allocate_result()
            }),
        )
        .unwrap();
    builder.finish().unwrap()
}

#[test]
fn tracing_panic_quarantines_without_sweeping_a_partial_graph() {
    let faults = Arc::new(Faults::default());
    let native = module(&faults);
    let source = "use fixture::payload::{Payload, new}; fn main() -> Payload { new() }";
    let (vm, loaded) = compile(source, Some(&native));
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    let value = vm
        .execute(&loaded, ProgramEntry::Main)
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let root = vm.runtime().root_value(value).unwrap();
    vm.reload_program(
        &_loaded_program,
        Some("boundary".into()),
        compile_program(source, Some(&native)),
    )
    .unwrap();
    let before = vm.runtime().gc().stats();
    let counters = vm.runtime().resources().counters();
    faults.trace.store(true, Ordering::SeqCst);
    let error = vm.runtime().collect_garbage().unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
    assert!(
        error
            .message()
            .contains("garbage collection callback panicked")
    );
    assert!(vm.runtime().is_quarantined());
    assert_eq!(vm.runtime().gc().stats(), before);
    assert_eq!(vm.runtime().resources().counters(), counters);
    assert!(
        loaded
            .members()
            .all(|member| vm.runtime().modules().loaded(member.key()).is_some())
    );
    assert_eq!(faults.drops.load(Ordering::SeqCst), 0);
    assert_eq!(root.value(vm.runtime().gc()), Some(value));
    faults.trace.store(false, Ordering::SeqCst);
    drop(vm);
    assert_eq!(faults.drops.load(Ordering::SeqCst), 1);
    drop(root);
}

#[test]
fn destructor_panics_dispose_each_detached_payload_once_and_quarantine() {
    let faults = Arc::new(Faults::default());
    let (vm, loaded) = compile(
        "use fixture::payload::{Payload, new}; fn main() -> (Payload, Payload) { (new(), new()) }",
        Some(&module(&faults)),
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    let values = vm
        .execute(&loaded, ProgramEntry::Main)
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    faults.drop.store(true, Ordering::SeqCst);
    let error = vm.runtime().collect_garbage().unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
    assert!(
        error
            .message()
            .contains("native payload destruction panicked")
    );
    assert!(vm.runtime().is_quarantined());
    assert_eq!(faults.drops.load(Ordering::SeqCst), 2);
    assert_eq!(vm.runtime().gc().stats().allocated_objects, 0);
    assert_eq!(vm.runtime().gc().stats().current_heap_units, 0);
    assert!(!vm.runtime().gc().validate_value(&values));
    assert_eq!(
        vm.runtime().collect_garbage().unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
    drop(vm);
    assert_eq!(faults.drops.load(Ordering::SeqCst), 2);
}

#[test]
fn clearing_reloaded_static_cells_disposes_payloads_once_even_when_destructors_panic() {
    let faults = Arc::new(Faults::default());
    let native = module(&faults);
    let source = r#"
        use fixture::payload::{Payload, new};
        static var first: [Payload] = [new()];
        static var second: [Payload] = [new()];
        fn main() { val a = first; val b = second; }
        pub fn clear() { first = []; second = []; }
    "#;
    let (vm, old) = compile(source, Some(&native));
    let _old_program = old;
    let old = vm.runtime().select_program(&_old_program).unwrap();
    drop(vm.execute(&old, ProgramEntry::Main).unwrap());
    let staged = vm
        .runtime()
        .stage_reload_program(
            &_old_program,
            Some("boundary".into()),
            compile_program(source, Some(&native)),
        )
        .unwrap();
    let current = staged.module().clone();
    vm.runtime().publish_staged_reload(staged).unwrap();
    let _current_program = _old_program.clone();
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(faults.drops.load(Ordering::SeqCst), 0);
    drop(vm.execute(&current, ProgramEntry::Main).unwrap());
    vm.execute_typed::<_, ()>(&current, "clear", ()).unwrap();
    faults.drop.store(true, Ordering::SeqCst);
    let error = vm.runtime().collect_garbage().unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
    assert!(
        error
            .message()
            .contains("native payload destruction panicked")
    );
    assert!(vm.runtime().is_quarantined());
    assert_eq!(faults.drops.load(Ordering::SeqCst), 2);
    // The two empty arrays remain in the live installation; both native payloads died.
    assert_eq!(vm.runtime().gc().stats().allocated_objects, 2);
    assert!(vm.runtime().modules().loaded(old.key()).is_none());
    assert!(vm.runtime().modules().loaded(current.key()).is_some());
    drop(vm);
    assert_eq!(faults.drops.load(Ordering::SeqCst), 2);
}

#[test]
fn roots_do_not_keep_heap_payloads_alive_after_teardown() {
    use kagari_types::{scalar::BuiltinType, ty::Ty};
    let faults = Arc::new(Faults::default());
    let (vm, loaded) = compile(
        "use fixture::payload::{Payload, new}; fn main() -> Payload { new() }",
        Some(&module(&faults)),
    );
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    let session = vm
        .runtime()
        .begin_execution(&loaded, vm.runtime().execution_options())
        .unwrap();
    let value = vm
        .execute(&loaded, ProgramEntry::Main)
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let root = vm.runtime().root_value(value).unwrap();
    let array = vm
        .runtime()
        .alloc_array(&loaded, Ty::Builtin(BuiltinType::I32), vec![Value::I32(7)])
        .unwrap();
    let array_root = vm.runtime().gc().root_value(Value::Array(array)).unwrap();
    assert_eq!(faults.drops.load(Ordering::SeqCst), 0);
    drop(session);
    assert!(vm.runtime().execution_root().is_none());
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
