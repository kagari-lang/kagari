//! Cancellation during native output conversion must precede continuation publication.
use super::Fixture;
use crate::context::ExecutionContext;
use kagari_common::cancellation::CancellationToken;
use kagari_runtime::{
    error::{RuntimeError, RuntimeErrorKind, fault::EngineFault},
    frame::types::arguments::TypeArgument,
    native::{
        adapter::Async,
        binding::NativeResult,
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        completion::{Completion, CompletionStatus},
        conversion::{IntoKagari, KagariType, context::ConversionContext},
        declarations::FunctionDecl,
        future::NativeStart,
        types::Type,
    },
    task::{CancellationCause, TaskId, control::TaskScopeOwner, drive::TaskDriveResult},
    value::view::ValueView,
};

use std::{
    num::NonZeroUsize,
    slice,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};

const SOURCE: &str =
    include_str!("../../../../../tests/fixtures/embedding/async/output-publication.kgr");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Boundary {
    BeforeConversion,
    DuringConversion,
    ConversionFault,
    Success,
}

struct Reply {
    boundary: Boundary,
    cancellation: CancellationToken,
    encoded: Arc<AtomicUsize>,
    dropped: Arc<AtomicUsize>,
}

impl KagariType for Reply {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        <(i32,)>::kagari_type(catalog)
    }
}

impl IntoKagari for Reply {
    fn into_kagari<'call>(
        self,
        cx: &mut ConversionContext<'call>,
        expected: &TypeArgument,
    ) -> NativeResult<ValueView<'call>> {
        self.encoded.fetch_add(1, Ordering::SeqCst);
        let value = cx.encode_value(expected, (7_i32,))?;
        cx.runtime_mut().collect_garbage()?;
        match self.boundary {
            Boundary::DuringConversion => self.cancellation.cancel(),
            Boundary::ConversionFault => {
                return Err(cx
                    .runtime()
                    .quarantine_execution_invariant(EngineFault::InvalidConverterState));
            }
            Boundary::BeforeConversion | Boundary::Success => {}
        }
        // Deliberately return without polling: the boundary must reject publication.
        Ok(value)
    }
}

impl Drop for Reply {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::SeqCst);
    }
}

fn drive(f: &mut Fixture, scope: &TaskScopeOwner, task: TaskId) -> TaskDriveResult {
    for _ in 0..1000 {
        let result = f
            .runtime
            .drive_task(scope, task, NonZeroUsize::new(1).unwrap())
            .unwrap();
        if result != TaskDriveResult::Runnable {
            return result;
        }
        f.runtime.collect_garbage().unwrap();
    }
    panic!("bounded conversion driver");
}

#[test]
fn async_output_publication_contract() {
    for boundary in [
        Boundary::BeforeConversion,
        Boundary::DuringConversion,
        Boundary::ConversionFault,
        Boundary::Success,
    ] {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let cancelled = Arc::new(AtomicUsize::new(0));
        let encoded = Arc::new(AtomicUsize::new(0));
        let dropped = Arc::new(AtomicUsize::new(0));
        let mut f = Fixture::with_source(SOURCE, Default::default(), |engine| {
            let send = sent.clone();
            let cancelled = cancelled.clone();
            let mut module =
                ModuleBuilder::new("test::converted_reply", engine.declarations()).unwrap();
            {
                let registered_function = module
                    .define_function(
                        FunctionDecl::new("read").returns(
                            module
                                .declarations()
                                .future_type()
                                .unwrap()
                                .apply([<Reply>::kagari_type(module.declarations()).unwrap()])
                                .unwrap(),
                        ),
                    )
                    .unwrap();
                module.bind::<Async<()>, Reply>(
                    registered_function,
                    move |(): (), completion: Completion<Reply>| {
                        send.lock().unwrap().push(completion);
                        let cancelled = cancelled.clone();
                        Ok(NativeStart::cancellable(move || {
                            cancelled.fetch_add(1, Ordering::SeqCst);
                        }))
                    },
                )
            }
            .unwrap();
            engine.install(module.finish().unwrap()).unwrap();
        });

        f.runtime.collect_garbage().unwrap();
        let baseline = f.runtime.gc_stats();
        let members = f
            .runtime
            .select_program(&f.module)
            .unwrap()
            .members()
            .collect::<Vec<_>>();
        let context = ExecutionContext::default();
        let scope = f
            .runtime
            .create_dispatched_task_scope(
                &f.runtime.select_program(&f.module).unwrap(),
                context.interpreter_options().unwrap(),
                Arc::new(|_| Ok(())),
            )
            .unwrap();
        let items = f.invoke("items", &[]);
        let factory = f.invoke("factory", slice::from_ref(&items));
        let handle = f
            .runtime
            .spawn_task(&(scope.capability()).clone(), &factory)
            .unwrap()
            .unwrap();
        drop(factory);
        let task = f.runtime.task_id(&handle).unwrap();
        assert_eq!(drive(&mut f, &scope, task), TaskDriveResult::Waiting);
        let reply = Reply {
            boundary,
            cancellation: context.cancellation.clone(),
            encoded: encoded.clone(),
            dropped: dropped.clone(),
        };
        let completion = sent.lock().unwrap().pop().unwrap();
        let late = completion.clone();
        thread::spawn(move || {
            assert_eq!(completion.complete(Ok(reply)), CompletionStatus::Accepted);
        })
        .join()
        .unwrap();
        assert_eq!(
            encoded.load(Ordering::SeqCst),
            0,
            "completion cannot convert on a worker"
        );
        assert_eq!(dropped.load(Ordering::SeqCst), 0);
        if boundary == Boundary::BeforeConversion {
            context.cancellation.cancel();
        }
        assert_eq!(drive(&mut f, &scope, task), TaskDriveResult::Complete);
        let report = f.runtime.take_task_report(&scope, task).unwrap().unwrap();
        assert!(f.runtime.take_task_report(&scope, task).unwrap().is_none());
        if boundary == Boundary::Success {
            assert_eq!(report.outcome.unwrap().as_i32().unwrap(), 12);
        } else {
            let failure = report.outcome.unwrap_err();
            assert_eq!(failure.source_task, task);
            if boundary == Boundary::ConversionFault {
                assert_eq!(failure.error.kind(), RuntimeErrorKind::EngineFault);
                assert!(f.runtime.is_quarantined());
            } else {
                assert_eq!(failure.error.kind(), RuntimeErrorKind::Cancelled);
                assert_eq!(failure.cancellation, Some(CancellationCause::ScopeClose));
                assert_eq!(
                    f.invoke("first", slice::from_ref(&items)).as_i32().unwrap(),
                    5
                );
            }
        }
        assert_eq!(
            encoded.load(Ordering::SeqCst),
            usize::from(boundary != Boundary::BeforeConversion)
        );
        assert_eq!(
            cancelled.load(Ordering::SeqCst),
            usize::from(boundary == Boundary::BeforeConversion)
        );
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        assert_eq!(
            late.complete(Err(RuntimeError::module_validation("late"))),
            CompletionStatus::Stale
        );
        if boundary != Boundary::ConversionFault {
            assert!(!f.runtime.is_quarantined());
            let _ = f.invoke("write", slice::from_ref(&items));
        }
        scope.close();
        f.runtime.drain_cancelled_tasks().unwrap();
        drop((scope, handle, items));
        if boundary == Boundary::ConversionFault {
            assert_eq!(
                f.runtime.collect_garbage().unwrap_err().kind(),
                RuntimeErrorKind::EngineFault
            );
        } else {
            f.runtime.collect_garbage().unwrap();
            assert_eq!(f.runtime.gc_stats().objects, baseline.objects);
            assert_eq!(
                f.runtime.gc_stats().root_registrations,
                baseline.root_registrations
            );
        }
        // Quarantine forbids collector service, including deferred root releases.
        // Execution registrations and provider counters remain observable cleanup oracles.
        for member in members {
            assert_eq!(
                f.runtime
                    .modules()
                    .retention_counts(member.key())
                    .active_calls,
                0
            );
        }
        assert!(f.runtime.execution_id().is_none());
        assert_eq!(f.runtime.resources().counters().current_call_depth, 0);
    }
}
