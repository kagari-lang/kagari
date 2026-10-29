//! Cold native producers keep traced arguments until an owned execution drives them.
use crate::native::storage::scan::{NativeScanKind, ScanBudget, ScanStatus};
mod registration;
use crate::{
    Runtime,
    closure::CallableObservation,
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        completion::{Completion, CompletionRegistry, Operation, OperationPoll},
        composition::ColdComposition,
        context::arguments::ArgumentView,
        conversion::{IntoKagari, arguments::FromKagariArguments, context::ConversionContext},
        storage::{NativePayload, NativeStorage},
    },
    resource::HeapCharge,
    value::Value,
};
use kagari_types::declaration::native::NativeStorageLayout;
use std::{
    fmt,
    marker::PhantomData,
    slice,
    sync::Arc,
    task::{Poll, Waker},
};

/// Submission must be bounded and nonblocking. A completion can arrive before
/// submission returns. The optional cancellation hook must also be nonblocking.
pub enum NativeStart<T> {
    Ready(T),
    Pending(Option<Box<dyn FnOnce() + Send + 'static>>),
}

impl<T> NativeStart<T> {
    pub fn pending() -> Self {
        Self::Pending(None)
    }

    pub fn cancellable(cancel: impl FnOnce() + Send + 'static) -> Self {
        Self::Pending(Some(Box::new(cancel)))
    }
}

trait Producer: fmt::Debug + Send + Sync {
    fn start(
        &self,
        cx: &mut ConversionContext<'_>,
        cold: &NativeFuture,
        registry: &CompletionRegistry,
        wake: &Waker,
    ) -> NativeResult<Box<dyn PendingNative>>;
}

pub(crate) trait PendingNative: fmt::Debug + Send {
    fn poll(&mut self, runtime: &Runtime) -> NativeResult<Poll<Value>>;

    fn cancel(&mut self) -> NativeResult<()>;
}

struct TypedProducer<A, R, F> {
    entry: F,
    marker: PhantomData<fn(A) -> R>,
}

impl<A, R, F> fmt::Debug for TypedProducer<A, R, F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TypedProducer")
    }
}

impl<A, R, F> Producer for TypedProducer<A, R, F>
where
    A: FromKagariArguments + 'static,
    R: IntoKagari + Send + 'static,
    F: Fn(A, Completion<R>) -> NativeResult<NativeStart<R>> + Send + Sync,
{
    fn start(
        &self,
        cx: &mut ConversionContext<'_>,
        cold: &NativeFuture,
        registry: &CompletionRegistry,
        wake: &Waker,
    ) -> NativeResult<Box<dyn PendingNative>> {
        cx.check_type::<R>(&cold.output)?;
        A::check_types(cx, &cold.parameters)?;
        // Reserve before converters or submission can produce host side effects.
        let mut operation = registry.reserve::<R>()?;
        operation.set_waker(wake);
        let completion = operation.completion()?;
        let snapshot = ArgumentView::from_values(cx.runtime().gc(), cold.values.clone())?;
        let arguments = A::from_arguments(cx, &cold.parameters, &snapshot)?;
        cx.poll()?;
        match (self.entry)(arguments, completion.clone())? {
            NativeStart::Ready(value) => {
                completion.complete(Ok(value));
            }
            NativeStart::Pending(Some(cancel)) => operation.on_cancel(cancel)?,
            NativeStart::Pending(None) => {}
        }
        cx.poll()?;
        Ok(Box::new(TypedPending {
            operation,
            owner: cold.owner.clone(),
            output: cold.output.clone(),
        }))
    }
}

struct TypedPending<R> {
    operation: Operation<R>,
    owner: ExecutableModule,
    output: TypeArgument,
}

impl<R> fmt::Debug for TypedPending<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TypedPending")
            .field("operation", &self.operation)
            .finish_non_exhaustive()
    }
}

impl<R: IntoKagari + Send + 'static> PendingNative for TypedPending<R> {
    fn poll(&mut self, runtime: &Runtime) -> NativeResult<Poll<Value>> {
        runtime.resources().poll_execution()?;
        match self.operation.poll()? {
            OperationPoll::Pending => Ok(Poll::Pending),
            OperationPoll::Ready(result) => {
                let mut cx = ConversionContext::new(runtime, &self.owner)?;
                cx.poll()?;
                let value = cx.encode_transport(&self.output, result?)?;
                cx.poll()?;
                Ok(Poll::Ready(value))
            }
            OperationPoll::Retired => Err(RuntimeError::module_validation("retired native wait")),
        }
    }

    fn cancel(&mut self) -> NativeResult<()> {
        self.operation.cancel()
    }
}

#[derive(Debug)]
pub(crate) struct NativeFuture {
    pub(crate) origin: Option<CallableObservation>,
    owner: ExecutableModule,
    parameters: Vec<TypeArgument>,
    output: TypeArgument,
    pub(crate) values: Vec<Value>,
    producer: Arc<dyn Producer>,
}

impl NativeFuture {
    pub(crate) fn start(
        &self,
        runtime: &Runtime,
        registry: &CompletionRegistry,
        wake: &Waker,
    ) -> NativeResult<Box<dyn PendingNative>> {
        let mut cx = ConversionContext::new(runtime, &self.owner)?;
        self.producer.start(&mut cx, self, registry, wake)
    }
}

#[derive(Debug)]
pub(crate) enum ColdFuture {
    Native(NativeFuture),
    Composition(ColdComposition),
    // A private, fully captured resume closure keeps its executable environment
    // reachable through the existing GC metadata graph. It never escapes alone.
    Script(Value),
}

impl ColdFuture {
    pub(crate) fn retained_bytes(&self) -> usize {
        match self {
            Self::Native(cold) => cold
                .values
                .capacity()
                .checked_mul(size_of::<Value>())
                .and_then(|bytes| {
                    bytes.checked_add(
                        cold.parameters
                            .capacity()
                            .checked_mul(size_of::<TypeArgument>())?,
                    )
                })
                .expect("disjoint cold capture buffers"),
            Self::Script(_) | Self::Composition(_) => 0,
        }
    }

    pub(crate) fn values(&self) -> &[Value] {
        match self {
            Self::Native(cold) => &cold.values,
            Self::Script(closure) => slice::from_ref(closure),
            Self::Composition(cold) => &cold.values,
        }
    }
}

#[derive(Debug)]
pub(crate) struct FuturePayload {
    pub(crate) cold: Option<ColdFuture>,
}

unsafe impl NativePayload for FuturePayload {
    type ScanState = (usize, usize);
    const SCAN_KIND: NativeScanKind = NativeScanKind::Resumable;

    fn begin_scan(&self) -> Self::ScanState {
        (
            0,
            self.cold.as_ref().map_or(&[][..], ColdFuture::values).len(),
        )
    }

    fn scan<'payload>(
        &'payload self,
        state: &mut Self::ScanState,
        budget: &mut ScanBudget,
        visit: &mut dyn FnMut(&'payload Value),
    ) -> ScanStatus {
        let values = self.cold.as_ref().map_or(&[][..], ColdFuture::values);
        while state.0 < state.1 {
            if !budget.take(1) {
                return ScanStatus::Pending;
            }
            if let Some(value) = values.get(state.0) {
                visit(value);
            }
            state.0 += 1;
        }
        ScanStatus::Complete
    }

    fn charge(&self) -> HeapCharge {
        HeapCharge::new(
            self.cold.as_ref().map_or(0, |cold| cold.values().len()),
            self.cold.as_ref().map_or(0, ColdFuture::retained_bytes),
        )
    }
}

impl NativeStorage {
    /// Sealed runtime storage for a nominal Future<T> declaration. Script cannot
    /// construct or modify its payload; only registered cold producers can do so.
    pub fn future() -> Self {
        Self::provided_with_layout::<FuturePayload>(NativeStorageLayout::Future)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{gc::graph::GraphIdentity, test_support::layouts::allocation_owner};
    use kagari_types::{scalar::BuiltinType, ty::Ty};

    #[derive(Debug)]
    struct Unstarted;

    impl Producer for Unstarted {
        fn start(
            &self,
            _: &mut ConversionContext<'_>,
            _: &NativeFuture,
            _: &CompletionRegistry,
            _: &Waker,
        ) -> NativeResult<Box<dyn PendingNative>> {
            panic!("claiming captures must not submit the producer")
        }
    }

    #[test]
    fn capture_roots_cover_pair_claims_between_marker_slices_without_submitting() {
        let mut runtime = Runtime::default();
        let owner = allocation_owner(&mut runtime);
        let ty = Ty::Builtin(BuiltinType::Unit);
        let output = runtime
            .resolve_type_arguments(&owner, slice::from_ref(&ty))
            .unwrap()
            .remove(0);
        let children = [
            runtime.gc.alloc_string("left".into()).unwrap(),
            runtime.gc.alloc_string("right".into()).unwrap(),
        ];
        let ids = children.map(|child| {
            let payload = FuturePayload {
                cold: Some(ColdFuture::Native(NativeFuture {
                    origin: None,
                    owner: owner.clone(),
                    parameters: vec![],
                    output: output.clone(),
                    values: vec![child],
                    producer: Arc::new(Unstarted),
                })),
            };
            let object = NativeStorage::future()
                .prepare_payload(&runtime.gc, &ty, payload, &owner)
                .unwrap();
            runtime.gc.alloc_native(object).unwrap()
        });
        runtime.gc.graph.lock().begin_cycle().unwrap();
        runtime.gc.graph.lock().finish_roots().unwrap();
        for id in ids {
            runtime.gc.publish_value(Value::GcHandle(id)).unwrap();
        }
        // Both source payloads are still gray. Capture publication must protect
        // their values before take removes the only heap edges to those values.
        let roots = runtime.gc.prepare_future_pair(ids).unwrap();
        let cold = runtime.gc.take_cold_future_pair(ids).unwrap();
        runtime.gc.drain_graph_work(runtime.modules()).unwrap();
        runtime.gc.graph.lock().close_mark().unwrap();
        for (index, child) in children.iter().enumerate() {
            assert_eq!(cold[index].values(), slice::from_ref(child));
            assert!(
                runtime
                    .gc
                    .graph_survivor(
                        runtime.modules(),
                        GraphIdentity::Object(child.checked_id().unwrap())
                    )
                    .unwrap()
            );
        }
        runtime.gc.graph.lock().finish_sweep().unwrap();
        drop(roots);
        runtime.collect_garbage().unwrap();
        for child in children {
            assert!(!runtime.gc.validate_value(&child));
        }
    }

    #[test]
    fn cold_claim_transfers_backing_and_sweep_releases_only_remaining_charge() {
        let mut runtime = Runtime::default();
        let owner = allocation_owner(&mut runtime);
        let ty = Ty::Builtin(BuiltinType::Unit);
        let output = runtime
            .resolve_type_arguments(&owner, std::slice::from_ref(&ty))
            .unwrap()
            .remove(0);
        let mut values = Vec::with_capacity(64);
        values.extend([Value::Unit, Value::Unit]);
        let parameters = Vec::with_capacity(8);
        let backing = values.capacity() * size_of::<Value>()
            + parameters.capacity() * size_of::<TypeArgument>();
        let payload = FuturePayload {
            cold: Some(ColdFuture::Native(NativeFuture {
                origin: None,
                owner: owner.clone(),
                parameters,
                output,
                values,
                producer: Arc::new(Unstarted),
            })),
        };
        let object = NativeStorage::future()
            .prepare_payload(runtime.gc(), &ty, payload, &owner)
            .unwrap();
        let id = runtime.gc().alloc_native(object).unwrap();
        let root = runtime.root_value(Value::GcHandle(id)).unwrap();
        // This synthetic producer has no binding identity. Observation neither
        // invents one nor starts/claims the producer (Unstarted would panic).
        assert_eq!(runtime.observe_callable_value(&root).unwrap(), None);
        assert_eq!(
            runtime.gc().stats().current_retained_bytes,
            (backing + size_of::<FuturePayload>()) as u128
        );
        let cold = runtime.gc().take_cold_future(id).unwrap();
        assert_eq!(runtime.observe_callable_value(&root).unwrap(), None);
        assert_eq!(runtime.gc().stats().current_heap_units, 1);
        assert_eq!(
            runtime.gc().stats().current_retained_bytes,
            size_of::<FuturePayload>() as u128
        );
        assert!(runtime.gc().take_cold_future(id).is_err());
        drop(root);
        runtime.collect_garbage().unwrap();
        assert_eq!(runtime.gc().stats().current_heap_units, 0);
        assert_eq!(runtime.gc().stats().current_retained_bytes, 0);
        assert_eq!(cold.values(), [Value::Unit, Value::Unit]);
    }
}
