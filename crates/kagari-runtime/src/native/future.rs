//! Cold native producers keep traced arguments until an owned execution drives them.
mod registration;
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    module::LoadedModule,
    native::{
        binding::NativeResult,
        completion::{Completion, CompletionRegistry, Operation, OperationPoll},
        conversion::{IntoKagari, arguments::FromKagariArguments, context::ConversionContext},
        storage::{NativePayload, NativeStorage},
    },
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
        let arguments = A::from_arguments(cx, &cold.parameters, &cold.values)?;
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
    owner: LoadedModule,
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
                let value = cx.encode_value(&self.output, result?)?;
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
    owner: LoadedModule,
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
    // A private, fully captured resume closure keeps its executable environment
    // reachable through the existing GC metadata graph. It never escapes alone.
    Script(Value),
}

impl ColdFuture {
    pub(crate) fn values(&self) -> &[Value] {
        match self {
            Self::Native(cold) => &cold.values,
            Self::Script(closure) => slice::from_ref(closure),
        }
    }
}

#[derive(Debug)]
pub(crate) struct FuturePayload {
    pub(crate) cold: Option<ColdFuture>,
}

impl NativePayload for FuturePayload {
    fn trace<'payload>(&'payload self, visit: &mut dyn FnMut(&'payload Value)) {
        if let Some(cold) = &self.cold {
            for value in cold.values() {
                visit(value);
            }
        }
    }

    fn units(&self) -> usize {
        self.cold.as_ref().map_or(0, |cold| cold.values().len())
    }
}

impl NativeStorage {
    /// Sealed runtime storage for a nominal Future<T> declaration. Script cannot
    /// construct or modify its payload; only registered cold producers can do so.
    pub fn future() -> Self {
        Self::provided_with_layout::<FuturePayload>(NativeStorageLayout::Future)
    }
}
