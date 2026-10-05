use crate::{compile, compile_program};
use kagari_runtime::{
    Runtime,
    error::RuntimeError,
    frame::ExecutionFrame,
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        context::CallContext,
        declarations::FunctionDecl,
        function_handle::PinnedFunction,
        module::NativeModule,
        storage::{NativePayload, NativeStorage},
        typed::NativeContext,
        types::Type,
        views::ValueHandle,
    },
    session::{ExecutionEvent, ExecutionObserver},
    value::Value,
};
use kagari_stdlib::declarations::StandardDeclarations;
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
    fn trace<'payload>(&'payload self, _: &mut dyn FnMut(&'payload Value)) {}

    fn units(&self) -> usize {
        0
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

type Callback = PinnedFunction<(i32,), i32>;

fn module(held: &Arc<Mutex<Option<Callback>>>, drops: Sender<ThreadId>) -> NativeModule {
    let mut builder = ModuleBuilder::new(
        "fixture::transfer",
        &StandardDeclarations::default().catalog().unwrap(),
    );
    let mut declaration = builder.define_type("Counter");
    declaration
        .native_storage(NativeStorage::new(move |_| {
            Ok(Counter {
                count: Cell::new(0),
                drops: drops.clone(),
            })
        }))
        .unwrap();
    let counter = declaration.finish().unwrap();
    let new = builder
        .define_function(FunctionDecl::new("new").returns(counter.apply([]).unwrap()))
        .unwrap();
    builder
        .bind_with(
            new,
            NativeBinding::new(Vec::<Codec>::new(), counter.codec(), |cx| {
                cx.allocate_result()
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
        .bind_with(
            bump,
            NativeBinding::new(
                vec![counter.codec()],
                Codec::Scalar(Type::i32().abi().clone()),
                |cx| {
                    cx.with_payload::<Counter, _>(0, |counter| {
                        let next = counter.count.get() + 1;
                        counter.count.set(next);
                        Ok(Value::I32(next))
                    })
                },
            ),
        )
        .unwrap();
    let remember = builder
        .define_function(
            FunctionDecl::new("remember")
                .parameter("callback", Type::function([Type::i32()], Type::i32())),
        )
        .unwrap();
    let stored = held.clone();
    builder
        .bind_typed(
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
            |_cx: &mut CallContext<'_>, value: ValueHandle<'_>| -> NativeResult<i32> {
                let first = value.try_enter_operation()?.expect("first entry");
                assert!(value.try_enter_operation()?.is_none());
                drop(first);
                let panic = catch_unwind(AssertUnwindSafe(|| {
                    let _guard = value
                        .try_enter_operation()
                        .unwrap()
                        .expect("entry before panic");
                    panic!("operation unwind fixture");
                }));
                assert!(panic.is_err());
                let _after_unwind = value.try_enter_operation()?.expect("released on unwind");
                Ok(42)
            },
        )
        .unwrap();
    let current = held.clone();
    builder
        .bind_typed(
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
    let (vm, loaded) = compile(
        "use fixture::transfer::{new, guarded}; fn main() -> i32 { val counter = new(); guarded(counter) + guarded(counter) }",
        Some(&native),
    );
    assert_eq!(
        vm.execute(&loaded, "main")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(84)
    );
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(dropped.recv().unwrap(), thread::current().id());
    assert!(!vm.runtime().is_quarantined());
}

#[test]
fn live_runtime_moves_with_send_only_payloads_observer_and_pinned_closure() {
    let origin = thread::current().id();
    let held = Arc::new(Mutex::new(None));
    let (payload_drops, dropped_payloads) = mpsc::channel();
    let (observer_drops, dropped_observers) = mpsc::channel();
    let native = module(&held, payload_drops);
    let source = r#"
        use fixture::transfer::{new, bump, remember, invoke};
        fn main() -> i32 {
            val counter = new(); val offset = [40];
            remember(|value| value + offset[0] + bump(counter)); invoke(1)
        }
        fn run() -> i32 { invoke(1) }
    "#;
    let (vm, loaded) = compile(source, Some(&native));
    vm.runtime()
        .set_execution_observer(Observer {
            count: Cell::new(0),
            drops: observer_drops,
        })
        .unwrap();
    assert_eq!(
        vm.execute(&loaded, "main")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    let old_key = loaded.key();
    let replacement = vm
        .reload_program(
            &loaded,
            "boundary",
            compile_program(&source.replace("[40]", "[99]"), Some(&native)),
        )
        .unwrap();
    drop(loaded);
    drop(native);
    let first = thread::spawn(move || {
        assert_ne!(thread::current().id(), origin);
        let session = vm
            .runtime()
            .begin_execution(&replacement, Default::default())
            .unwrap();
        assert!(vm.runtime().attach_execution_observer().unwrap());
        assert_eq!(
            vm.execute(&replacement, "run")
                .unwrap()
                .return_value
                .value(vm.runtime().gc())
                .expect("retained execution result"),
            Value::I32(43)
        );
        assert!(vm.runtime().modules().loaded(old_key).is_some());
        assert!(
            vm.runtime()
                .execution_observer::<Observer>()
                .unwrap()
                .count
                .get()
                > 0
        );
        drop(session);
        assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
        (vm, replacement, thread::current().id())
    })
    .join()
    .unwrap();
    let final_thread = thread::spawn(move || {
        let (vm, replacement, previous_thread) = first;
        let receiving = thread::current().id();
        assert_ne!(receiving, previous_thread);
        assert_eq!(
            vm.execute(&replacement, "run")
                .unwrap()
                .return_value
                .value(vm.runtime().gc())
                .expect("retained execution result"),
            Value::I32(44)
        );
        assert_eq!(
            vm.execute(&replacement, "main")
                .unwrap()
                .return_value
                .value(vm.runtime().gc())
                .expect("retained execution result"),
            Value::I32(101)
        );
        vm.runtime().collect_garbage().unwrap();
        assert!(vm.runtime().modules().loaded(old_key).is_none());
        assert!(!vm.runtime().is_quarantined());
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
