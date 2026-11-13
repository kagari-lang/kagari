use crate::{
    declarations::StandardDeclarations,
    tests::{
        execution::native_boundary::{compile, compile_program},
        support::bytecode::reload_bytecode,
    },
};

use kagari_runtime::{
    Runtime,
    error::RuntimeError,
    frame::ExecutionFrame,
    module::entry::ProgramEntry,
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        context::NativeCall,
        declarations::FunctionDecl,
        function_handle::Function,
        module::NativeModule,
        storage::{NativePayload, NativeStorage},
        typed::NativeContext,
        types::Type,
        views::ValueHandle,
    },
    session::{ExecutionEvent, ExecutionObserver},
    value::view::ValueView,
};

use kagari_gc::{
    error::GcError,
    object::{GcObject, ObjectCharge},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};
use std::{
    cell::Cell,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex,
        mpsc::{self, Sender},
    },
    thread::{self, ThreadId},
};

#[derive(Debug)]
struct Counter {
    count: Cell<i32>,
    drops: Sender<ThreadId>,
}

impl NativePayload for Counter {
    fn registration_storage() -> NativeStorage {
        NativeStorage::payload::<Self>()
    }
}

// SAFETY: count is inline scalar state; the channel carries only Rust thread
// identities and cannot retain managed storage. Drop reads no managed peer.
unsafe impl GcObject for Counter {
    type ScanState = ();
    const SCAN_KIND: ScanKind = ScanKind::EdgeFree;

    fn begin_scan(&self) {}

    fn scan(
        &self,
        _: &mut (),
        _: &mut ScanBudget,
        _: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        Ok(ScanStatus::Complete)
    }

    fn charge(&self) -> ObjectCharge {
        ObjectCharge { units: 0, bytes: 0 }
    }
}

impl Drop for Counter {
    fn drop(&mut self) {
        self.drops.send(thread::current().id()).unwrap();
    }
}

#[derive(Debug)]
struct Observer {
    count: Cell<usize>,
    drops: Sender<ThreadId>,
}

impl ExecutionObserver for Observer {
    fn observe(
        &mut self,
        _: &Runtime,
        _: ExecutionEvent,
        _: &[ExecutionFrame],
    ) -> Result<(), RuntimeError> {
        self.count.set(self.count.get() + 1);
        Ok(())
    }
}

impl Drop for Observer {
    fn drop(&mut self) {
        self.drops.send(thread::current().id()).unwrap();
    }
}

type Callback = Function<(i32,), i32>;

fn module(held: &Arc<Mutex<Option<Callback>>>, drops: Sender<ThreadId>) -> NativeModule {
    let mut builder = ModuleBuilder::new(
        "fixture::transfer",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    let mut declaration = builder.define_type("Counter").unwrap();
    declaration
        .native_storage(NativeStorage::payload::<Counter>())
        .unwrap();
    let counter = declaration.finish().unwrap();
    let new = builder
        .define_function(FunctionDecl::new("new").returns(counter.apply([]).unwrap()))
        .unwrap();
    builder
        .bind(
            new,
            NativeBinding::new(Vec::<Codec>::new(), counter.codec(), move |cx| {
                cx.allocate_result_with(&[], |native| {
                    native.scope().alloc_leaf(Counter {
                        count: Cell::new(0),
                        drops: drops.clone(),
                    })
                })
            }),
        )
        .unwrap();
    let bump = builder
        .define_function(
            FunctionDecl::new("bump")
                .parameter("counter", counter.apply([]).unwrap())
                .returns(Type::i32()),
        )
        .unwrap();
    builder
        .bind(
            bump,
            NativeBinding::new(vec![counter.codec()], Codec::Exact(Type::i32()), |cx| {
                cx.with_payload::<Counter, _>(0, |counter| {
                    let next = counter.count.get() + 1;
                    counter.count.set(next);
                    Ok(ValueView::from(next))
                })
            }),
        )
        .unwrap();
    let remember = builder
        .define_function(FunctionDecl::new("remember").parameter(
            "callback",
            Type::function([Type::i32()], Type::i32()).unwrap(),
        ))
        .unwrap();
    let stored = held.clone();
    builder
        .bind(
            remember,
            move |_: &mut NativeContext<'_>, (callback,): (Callback,)| -> NativeResult<()> {
                *stored.lock().unwrap() = Some(callback);
                Ok(())
            },
        )
        .unwrap();
    let invoke = builder
        .define_function(
            FunctionDecl::new("invoke")
                .parameter("value", Type::i32())
                .returns(Type::i32()),
        )
        .unwrap();
    let guarded = builder
        .define_function(
            FunctionDecl::new("guarded")
                .parameter("counter", counter.apply([]).unwrap())
                .returns(Type::i32()),
        )
        .unwrap();
    builder
        .bind(
            guarded,
            |cx: &mut NativeCall<'_>, value: ValueHandle<'_>| -> NativeResult<i32> {
                value
                    .with_operation(cx, |cx| {
                        cx.collect_garbage()?;
                        assert!(value.with_operation(cx, |_| Ok(())).unwrap().is_none());
                        Ok(())
                    })?
                    .expect("first entry");
                let panic = catch_unwind(AssertUnwindSafe(|| {
                    let _ = value.with_operation(cx, |_| -> NativeResult<()> {
                        panic!("operation unwind fixture");
                    });
                }));
                assert!(panic.is_err());
                value.with_operation(cx, |_| Ok(42))?.ok_or_else(|| {
                    RuntimeError::module_validation("operation not released on unwind")
                })
            },
        )
        .unwrap();
    let current = held.clone();
    builder
        .bind(
            invoke,
            move |cx: &mut NativeContext<'_>, (value,): (i32,)| -> NativeResult<i32> {
                let callback = current.lock().unwrap().as_ref().unwrap().clone();
                cx.collect_garbage()?;
                callback.call(cx, (value,))
            },
        )
        .unwrap();
    builder.finish().unwrap()
}

#[test]
fn native_operation_guards_release_on_unwind() {
    let (drops, dropped) = mpsc::channel();
    let native = module(&Arc::new(Mutex::new(None)), drops);
    let (mut vm, loaded) = compile(
        include_str!("../../../../tests/fixtures/native/operation-unwind.kgr"),
        Some(&native),
    );
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(84)
    );
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert_eq!(dropped.recv().unwrap(), thread::current().id());
    assert!(!vm.is_quarantined());
}

#[test]
fn live_runtime_moves_with_send_only_payloads_observer_and_pinned_closure() {
    let origin = thread::current().id();
    let held = Arc::new(Mutex::new(None));
    let (payload_drops, dropped_payloads) = mpsc::channel();
    let (observer_drops, dropped_observers) = mpsc::channel();
    let native = module(&held, payload_drops);
    let source = include_str!("../../../../tests/fixtures/native/thread-transfer.kgr");
    let (mut vm, loaded) = compile(source, Some(&native));
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    vm.set_execution_observer(Observer {
        count: Cell::new(0),
        drops: observer_drops,
    })
    .unwrap();
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
    let old_key = loaded.key();
    reload_bytecode(
        &mut vm,
        &_loaded_program,
        Some("boundary".into()),
        compile_program(&source.replace("[40]", "[99]"), Some(&native)),
    )
    .unwrap();
    let _replacement_program = _loaded_program.clone();
    let replacement = vm.select_program(&_replacement_program).unwrap();
    drop(loaded);
    drop(native);
    let first = thread::spawn(move || {
        assert_ne!(thread::current().id(), origin);
        let mut session = vm
            .begin_execution(&replacement, Default::default())
            .unwrap();
        let session_options = session.runtime().execution_options();
        assert!(session.runtime().attach_execution_observer().unwrap());
        assert_eq!(
            session
                .runtime_mut()
                .execute_module(&replacement, "run", session_options.clone())
                .unwrap()
                .return_value
                .as_i32(),
            Some(43)
        );
        assert!(session.runtime().modules().loaded(old_key).is_some());
        assert!(
            session
                .runtime()
                .execution_observer::<Observer>()
                .unwrap()
                .count
                .get()
                > 0
        );
        drop(session);
        assert_eq!(vm.resources().counters().current_call_depth, 0);
        (vm, replacement, thread::current().id())
    })
    .join()
    .unwrap();
    let final_thread = thread::spawn(move || {
        let (mut vm, replacement, previous_thread) = first;
        let receiving = thread::current().id();
        assert_ne!(receiving, previous_thread);
        assert_eq!(
            vm.execute_module(&replacement, "run", vm.execution_options())
                .unwrap()
                .return_value
                .as_i32(),
            Some(44)
        );
        assert_eq!(
            vm.execute_module(&replacement, ProgramEntry::Main, vm.execution_options())
                .unwrap()
                .return_value
                .as_i32(),
            Some(101)
        );
        vm.collect_garbage().unwrap();
        assert!(vm.modules().loaded(old_key).is_none());
        assert!(!vm.is_quarantined());
        // An external callback root must not keep heap storage alive after teardown.
        drop(vm);
        held.lock().unwrap().take();
        receiving
    })
    .join()
    .unwrap();
    assert_eq!(
        dropped_payloads.into_iter().collect::<Vec<_>>(),
        [final_thread, final_thread]
    );
    assert_eq!(
        dropped_observers.into_iter().collect::<Vec<_>>(),
        [final_thread]
    );
}
