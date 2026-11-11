use crate::{
    declarations::StandardDeclarations,
    tests::{
        execution::native_boundary::{compile, compile_program},
        support::bytecode::reload_bytecode,
    },
};

use kagari_runtime::{
    error::RuntimeErrorKind,
    module::entry::ProgramEntry,
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        declarations::FunctionDecl,
        module::NativeModule,
        storage::{NativePayload, NativeStorage},
    },
    resource::HeapCharge,
    value::Value,
};

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

    fn charge(&self) -> NativeResult<HeapCharge> {
        Ok(HeapCharge::new(0, 0))
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
    let (mut vm, loaded) = compile(source, Some(&native));
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    let value = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .unwrap()
        .return_value
        .value(vm.gc())
        .expect("retained execution result");
    let root = vm.root_value(value).unwrap();
    reload_bytecode(
        &mut vm,
        &_loaded_program,
        Some("boundary".into()),
        compile_program(source, Some(&native)),
    )
    .unwrap();
    let before = vm.gc().stats();
    let counters = vm.resources().counters();
    faults.trace.store(true, Ordering::SeqCst);
    let error = vm.collect_garbage().unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
    assert!(
        error
            .to_string()
            .contains("garbage collection callback panicked")
    );
    assert!(vm.is_quarantined());
    assert_eq!(vm.gc().stats(), before);
    assert_eq!(vm.resources().counters(), counters);
    assert!(
        loaded
            .members()
            .all(|member| vm.modules().loaded(member.key()).is_some())
    );
    assert_eq!(faults.drops.load(Ordering::SeqCst), 0);
    assert_eq!(root.value(vm.gc()), Some(value));
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
    let loaded = vm.select_program(&_loaded_program).unwrap();
    let values = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .unwrap()
        .return_value
        .value(vm.gc())
        .expect("retained execution result");
    faults.drop.store(true, Ordering::SeqCst);
    let error = vm.collect_garbage().unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
    assert!(
        error
            .to_string()
            .contains("native payload destruction panicked")
    );
    assert!(vm.is_quarantined());
    assert_eq!(faults.drops.load(Ordering::SeqCst), 2);
    assert_eq!(vm.gc().stats().allocated_objects, 0);
    assert_eq!(vm.gc().stats().current_heap_units, 0);
    assert!(!vm.gc().validate_value(&values));
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
    let source = r#"
        use fixture::payload::{Payload, new};
        static var first: [Payload] = [new()];
        static var second: [Payload] = [new()];
        fn main() { val a = first; val b = second; }
        pub fn clear() { first = []; second = []; }
    "#;
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
    assert!(
        error
            .to_string()
            .contains("native payload destruction panicked")
    );
    assert!(vm.is_quarantined());
    assert_eq!(faults.drops.load(Ordering::SeqCst), 2);
    // The two empty arrays remain in the live installation; both native payloads died.
    assert_eq!(vm.gc().stats().allocated_objects, 2);
    assert!(vm.modules().loaded(old.key()).is_none());
    assert!(vm.modules().loaded(current.key()).is_some());
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
    let loaded = vm.select_program(&_loaded_program).unwrap();
    let session = vm.begin_execution(&loaded, vm.execution_options()).unwrap();
    let value = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .unwrap()
        .return_value
        .value(vm.gc())
        .expect("retained execution result");
    let root = vm.root_value(value).unwrap();
    let array = vm
        .alloc_array(&loaded, Ty::Builtin(BuiltinType::I32), vec![Value::I32(7)])
        .unwrap();
    let array_root = vm.gc().root_value(Value::Array(array)).unwrap();
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
