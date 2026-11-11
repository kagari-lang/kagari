use super::Call;
use crate::{
    error::RuntimeErrorKind,
    interpreter_tests::common::{compile_native_test_bytecode, test_runtime},
    test_support::bytecode::load_bytecode,
};
use kagari_runtime::{
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, KagariType, context::ConversionContext},
        types::Type,
    },
    value::Value,
};

use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
};

static DISCARDED: AtomicUsize = AtomicUsize::new(0);

struct HostileOutput;

impl KagariType for HostileOutput {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        i32::kagari_type(catalog)
    }
}

impl FromKagari for HostileOutput {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        ty: &TypeArgument,
        value: &Value,
    ) -> NativeResult<Self> {
        assert_eq!(i32::from_kagari(cx, ty, value)?, 42);
        Ok(Self)
    }
}

impl Drop for HostileOutput {
    fn drop(&mut self) {
        DISCARDED.fetch_add(1, Ordering::SeqCst);
        panic!("discarded host output");
    }
}

struct HostileDriverDrop(Arc<AtomicUsize>);

// The retained waker's destructor is under test; noop has no owned destructor.
#[expect(clippy::manual_noop_waker, reason = "exercise host waker destruction")]
impl Wake for HostileDriverDrop {
    fn wake(self: Arc<Self>) {}
}

impl Drop for HostileDriverDrop {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
        panic!("driver release failed");
    }
}

#[test]
fn driver_release_fault_overrides_completion_and_preserves_trace() {
    let provider = crate::test_support::future::provider();
    let program = compile_native_test_bytecode(
        include_str!("../../../tests/fixtures/execution/call-driver-release.kgr"),
        &[&provider],
    );
    for fail in [false, true] {
        let mut runtime = test_runtime(Default::default());
        provider.install(&mut runtime).unwrap();
        let module = load_bytecode(&mut runtime, "handoff", program.clone()).unwrap();
        let function = runtime
            .bind_async_function::<(bool,), HostileOutput>(&module.root(), "run")
            .unwrap();
        let discarded = DISCARDED.load(Ordering::SeqCst);
        let pending = runtime.submit_async(&function, (fail,)).unwrap();
        for _ in 0..100 {
            if pending.is_finished() {
                break;
            }
            assert!(
                runtime
                    .poll(&mut Context::from_waker(Waker::noop()))
                    .is_pending()
            );
        }
        assert!(pending.is_finished());
        let mut sibling = runtime.submit_async(&function, (false,)).unwrap();
        let drops = Arc::new(AtomicUsize::new(0));
        let wake = Waker::from(Arc::new(HostileDriverDrop(drops.clone())));
        let driver = runtime
            .borrow_scheduler_driver(&Context::from_waker(&wake))
            .unwrap();
        drop(wake);
        // A ready result bypasses another scheduler turn. Releasing the only
        // driver-waker owner in finish is the first operation that faults.
        let mut call = Call::new(&mut runtime, pending);
        call.driver = Some(driver);
        let Poll::Ready(result) = Pin::new(&mut call).poll(&mut Context::from_waker(Waker::noop()))
        else {
            panic!("completed invocation");
        };
        drop(call);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        let Err(error) = result else {
            panic!("driver cleanup failure must override success");
        };
        assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
        assert_eq!(
            DISCARDED.load(Ordering::SeqCst),
            discarded + usize::from(!fail)
        );
        if fail {
            assert!(
                error.trace().is_some(),
                "preserve the original execution failure trace"
            );
        }
        assert!(runtime.scheduler_is_closed());
        assert!(matches!(
            Pin::new(&mut sibling).poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Err(_))
        ));
    }
}
