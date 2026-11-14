//! Cold native producers keep traced arguments until an owned execution drives them.
pub(crate) mod claims;
mod registration;
pub(crate) mod storage;
#[cfg(test)]
mod tests;
use crate::native::future::storage::ColdObject;
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
        conversion::{
            IntoKagari,
            arguments::{ArgumentValues, FromKagariArguments, PreparedArguments},
            context::ConversionContext,
        },
        storage::NativeStorage,
    },
    value::place::{PublishedResult, ResultPlace},
};
use kagari_gc::root::Root;
use kagari_types::declaration::native::NativeStorageLayout;
use std::{
    fmt,
    marker::PhantomData,
    ptr,
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
    fn start<'call>(
        &self,
        cx: &mut ConversionContext<'call>,
        cold: &NativeFuture,
        values: ArgumentValues<'call>,
        source: &Root<ColdObject>,
        registry: &CompletionRegistry,
        wake: &Waker,
    ) -> NativeResult<Box<dyn PendingNative>>;
}

pub(crate) trait PendingNative: fmt::Debug + Send {
    fn poll<'result>(
        &mut self,
        runtime: &mut Runtime,
        output: ResultPlace<'result>,
    ) -> NativeResult<Poll<PublishedResult<'result>>>;

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
    fn start<'call>(
        &self,
        cx: &mut ConversionContext<'call>,
        cold: &NativeFuture,
        values: ArgumentValues<'call>,
        source: &Root<ColdObject>,
        registry: &CompletionRegistry,
        wake: &Waker,
    ) -> NativeResult<Box<dyn PendingNative>> {
        let prepared = PreparedArguments::<A>::from_binding(cx, &cold.parameters);
        // Reserve before converters or submission can produce host side effects.
        let mut operation = registry.reserve::<R>()?;
        operation.set_waker(wake);
        let completion = operation.completion()?;
        let arguments = prepared.decode(cx, values)?;
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
            source: source.clone(),
        }))
    }
}

struct TypedPending<R> {
    source: Root<ColdObject>,
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
    fn poll<'result>(
        &mut self,
        runtime: &mut Runtime,
        output: ResultPlace<'result>,
    ) -> NativeResult<Poll<PublishedResult<'result>>> {
        runtime.resources().poll_execution()?;
        match self.operation.poll()? {
            OperationPoll::Pending => Ok(Poll::Pending),
            OperationPoll::Ready(result) => {
                let mut cx =
                    ConversionContext::for_cold_future(runtime, &self.owner, &self.source)?;
                cx.poll()?;
                let value = cx.encode_prepared(&self.output, result?)?;
                cx.poll()?;
                output.publish(cx.runtime_mut(), &value).map(Poll::Ready)
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
    producer: Arc<dyn Producer>,
}

impl NativeFuture {
    fn start(
        &self,
        runtime: &mut Runtime,
        source: &Root<ColdObject>,
        registry: &CompletionRegistry,
        wake: &Waker,
    ) -> NativeResult<Box<dyn PendingNative>> {
        let valid = runtime.memory.scope(&runtime.resources, |cx, _| {
            let cold = cx.get(source)?;
            Ok(cx.read(cold, |cold| matches!(cold.recipe().as_ref(), ColdFuture::Native(actual) if ptr::eq(actual, self))))
        }).map_err(|error| runtime.managed_memory_error(error))?;
        if !valid {
            return Err(RuntimeError::module_validation("cold producer source"));
        }
        let pending = {
            let mut cx = ConversionContext::for_cold_future(runtime, &self.owner, source)?;
            let values = cx.cold_future_arguments(source)?;
            self.producer
                .start(&mut cx, self, values, source, registry, wake)?
        };
        // The submission owns its decoded Rust inputs now. Retain only executable
        // evidence for completion conversion, not the original capture graph.
        runtime
            .memory
            .scope(&runtime.resources, |cx, _| {
                let cold = cx.get(source)?;
                ColdObject::release_captures(cx, cold)
            })
            .map_err(|error| runtime.managed_memory_error(error))?;
        Ok(pending)
    }
}

/// Immutable selected producer facts. Captured values live only in ColdObject.
#[derive(Debug)]
pub(crate) enum ColdFuture {
    Native(NativeFuture),
    Composition(ColdComposition),
    Script { owner: ExecutableModule },
}

impl ColdFuture {
    pub(crate) fn owner(&self) -> &ExecutableModule {
        match self {
            Self::Native(cold) => &cold.owner,
            Self::Composition(cold) => &cold.owner,
            Self::Script { owner } => owner,
        }
    }

    pub(crate) fn capture_count(&self) -> usize {
        match self {
            Self::Native(native) => native.parameters.len(),
            Self::Composition(_) => 2,
            Self::Script { .. } => 1,
        }
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        size_of::<Self>()
            + match self {
                Self::Native(cold) => cold.parameters.capacity() * size_of::<TypeArgument>(),
                Self::Script { .. } | Self::Composition(_) => 0,
            }
    }
}

impl NativeStorage {
    /// Sealed runtime storage for a nominal Future<T> declaration. Script cannot
    /// construct or modify its payload; only registered cold producers can do so.
    pub fn future() -> Self {
        Self::provided_with_layout::<storage::FuturePayload>(NativeStorageLayout::Future)
    }
}
