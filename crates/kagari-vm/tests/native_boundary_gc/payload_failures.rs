use crate::{compile, compile_program};
use kagari_runtime::{
    error::RuntimeErrorKind,
    native::{
        binding::{Codec, NativeBinding},
        builder::ModuleBuilder,
        declarations::FunctionDecl,
        module::NativeModule,
        storage::{NativePayload, NativeStorage},
    },
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

impl NativePayload for Payload {
    fn trace<'payload>(&'payload self, _: &mut dyn FnMut(&'payload Value)) {
        assert!(!self.0.trace.load(Ordering::SeqCst), "fixture trace panic");
    }

    fn units(&self) -> usize {
        0
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
    );
    let faults = faults.clone();
    let mut declaration = builder.define_type("Payload");
    declaration
        .native_storage(NativeStorage::new(move |_| Ok(Payload(faults.clone()))))
        .unwrap();
    let payload = declaration.finish().unwrap();
    let new = builder
        .define_function(FunctionDecl::new("new").returns(payload.apply([]).unwrap()))
        .unwrap();
    builder
        .bind_with(
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
    let value = vm
        .execute(&loaded, "main")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let root = vm.runtime().root_value(value).unwrap();
    vm.reload_program(&loaded, "boundary", compile_program(source, Some(&native)))
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
    let values = vm
        .execute(&loaded, "main")
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
fn root_and_collection_leases_do_not_keep_heap_payloads_alive_after_teardown() {
    use kagari_types::{scalar::BuiltinType, ty::Ty};
    let faults = Arc::new(Faults::default());
    let (vm, loaded) = compile(
        "use fixture::payload::{Payload, new}; fn main() -> Payload { new() }",
        Some(&module(&faults)),
    );
    let session = vm
        .runtime()
        .begin_execution(&loaded, vm.runtime().execution_options())
        .unwrap();
    let value = vm
        .execute(&loaded, "main")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let root = vm.runtime().root_value(value).unwrap();
    let array = vm
        .runtime()
        .alloc_array(&loaded, Ty::Builtin(BuiltinType::I32), vec![Value::I32(7)])
        .unwrap();
    let iteration = vm
        .runtime()
        .gc()
        .begin_collection_iteration(&Value::Array(array))
        .unwrap();
    assert_eq!(faults.drops.load(Ordering::SeqCst), 0);
    drop(session);
    assert!(vm.runtime().execution_root().is_none());
    drop(vm);
    assert_eq!(faults.drops.load(Ordering::SeqCst), 1);
    std::thread::spawn(move || {
        drop(root);
        drop(iteration);
    })
    .join()
    .unwrap();
    assert_eq!(faults.drops.load(Ordering::SeqCst), 1);
}
