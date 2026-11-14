//! Future admission consumes actual providers and retains one computation graph.
use crate::{
    Runtime,
    closure::{CallableObservation, ClosureRecord},
    error::{RuntimeError, trap::TrapReason},
    execution::roots::ExecutionRoots,
    frame::types::arguments::TypeArgument,
    module::{ExecutableModule, managed::revision::RevisionState},
    native::{
        binding::NativeResult,
        completion::CompletionRegistry,
        composition::ColdComposition,
        future::{
            ColdFuture, PendingNative,
            storage::{ColdObject, FuturePayload},
        },
    },
    value::{
        carriers::{RootedValue, ScopedValue},
        closures::ClosureObject,
        kind::ObjectKind,
        native::NativeObject,
        view::ValueView,
    },
    value_check::{
        data::{async_object, async_scoped},
        scoped::CheckError,
    },
};
use kagari_gc::{access::Scope, error::GcError, object::Gc, root::Root};
use kagari_types::declaration::native::NativeStorageLayout;
use std::{slice, sync::Arc, task::Waker};

/// A queued/starting invocation owns one genuine root, never copied argument IDs
/// or a root per captured value. Dropping it releases the whole selected graph.
#[derive(Debug)]
pub(crate) struct ClaimedFuture {
    source: Root<ColdObject>,
    recipe: Arc<ColdFuture>,
}

impl ClaimedFuture {
    /// A parked queue can outlive mutations through another alias. This fresh
    /// closed walk precedes decoding/submission; an immediate claim/start pair
    /// uses the claim's admission and does not repeat it.
    pub(crate) fn validate_queued(&self, runtime: &mut Runtime) -> NativeResult<()> {
        runtime.resources.poll_execution()?;
        let resources = &runtime.resources;
        let result = runtime.memory.scope(&runtime.resources, |cx, _| {
            let cold = cx.get(&self.source)?;
            Ok(async_object(cx, cold.erase(), resources))
        });
        let valid = result
            .map_err(|error| runtime.managed_memory_error(error))?
            .map_err(|error| {
                runtime
                    .resources
                    .admit_execution_error(error.into_runtime(runtime))
            })?;
        if !valid {
            return Err(RuntimeError::from(
                TrapReason::AsyncStateRetainsAnInvalidOrScopedValue,
            ));
        }
        Ok(())
    }

    pub(crate) fn into_composition(self) -> NativeResult<(ColdComposition, Root<ColdObject>)> {
        let ColdFuture::Composition(contract) = self.recipe.as_ref() else {
            return Err(RuntimeError::module_validation(
                "composition computation claim",
            ));
        };
        Ok((contract.clone(), self.source))
    }

    pub(crate) fn recipe(&self) -> &ColdFuture {
        &self.recipe
    }

    pub(crate) fn source(&self) -> &Root<ColdObject> {
        &self.source
    }

    /// Consume admission authority exactly once. A retained metadata root alone
    /// cannot resubmit a zero-argument producer after parking/completion.
    pub(crate) fn start_native(
        self,
        runtime: &mut Runtime,
        registry: &CompletionRegistry,
        wake: &Waker,
    ) -> NativeResult<Box<dyn PendingNative>> {
        let ColdFuture::Native(native) = self.recipe.as_ref() else {
            return Err(RuntimeError::module_validation("native computation claim"));
        };
        native.start(runtime, &self.source, registry, wake)
    }
}

impl Runtime {
    /// Admission before branch reservation is read-only. Pair claim rechecks
    /// live state after reservations and waker callbacks, before either detach.
    pub(crate) fn future_pair_contracts(
        &mut self,
        values: &[ValueView<'_>; 2],
    ) -> NativeResult<[FutureContract; 2]> {
        let Runtime {
            memory, resources, ..
        } = self;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                Ok((|| -> Result<_, CheckError> {
                    let values = [values[0].get(slots)?, values[1].get(slots)?];
                    if !async_scoped(slots.scope(), &values, resources)? {
                        return Err(RuntimeError::from(
                            TrapReason::AsyncStateRetainsAnInvalidOrScopedValue,
                        )
                        .into());
                    }
                    let objects = [
                        payload(slots.scope(), values[0])?,
                        payload(slots.scope(), values[1])?,
                    ];
                    if objects[0].ptr_eq(objects[1]) {
                        return Err(RuntimeError::from(TrapReason::DuplicateFutureOperands).into());
                    }
                    recipe(slots.scope(), objects[0])?;
                    recipe(slots.scope(), objects[1])?;
                    Ok([
                        FutureContract::read(slots.scope(), values[0])?,
                        FutureContract::read(slots.scope(), values[1])?,
                    ])
                })())
            })
        });
        result
            .map_err(|error| self.future_storage_error(error))?
            .map_err(|error| {
                self.resources
                    .admit_execution_error(error.into_runtime(self))
            })
    }

    pub(crate) fn claim_future(&mut self, value: &ValueView<'_>) -> NativeResult<ClaimedFuture> {
        self.resources.poll_execution()?;
        let Runtime {
            memory, resources, ..
        } = self;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                Ok((|| -> Result<ClaimedFuture, CheckError> {
                    let value = value.get(slots)?;
                    let object = payload(slots.scope(), value)?;
                    if !async_scoped(slots.scope(), slice::from_ref(&value), resources)? {
                        return Err(RuntimeError::from(
                            TrapReason::AsyncStateRetainsAnInvalidOrScopedValue,
                        )
                        .into());
                    }
                    let cold = FuturePayload::cold(slots.scope(), object).ok_or_else(|| {
                        RuntimeError::from(TrapReason::FutureHasAlreadyBeenAwaited)
                    })?;
                    let recipe = slots.scope().read(cold, |cold| cold.recipe().clone());
                    let source = FuturePayload::claim(slots.scope(), object)?.ok_or_else(|| {
                        RuntimeError::from(TrapReason::FutureHasAlreadyBeenAwaited)
                    })?;
                    Ok(ClaimedFuture { source, recipe })
                })())
            })
        });
        result
            .map_err(|error| self.future_storage_error(error))?
            .map_err(|error| {
                self.resources
                    .admit_execution_error(error.into_runtime(self))
            })
    }

    pub(crate) fn claim_future_pair(
        &mut self,
        values: &[ValueView<'_>; 2],
    ) -> NativeResult<[ClaimedFuture; 2]> {
        self.resources.poll_execution()?;
        let Runtime {
            memory, resources, ..
        } = self;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                Ok((|| -> Result<[ClaimedFuture; 2], CheckError> {
                    let values = [values[0].get(slots)?, values[1].get(slots)?];
                    let objects = [
                        payload(slots.scope(), values[0])?,
                        payload(slots.scope(), values[1])?,
                    ];
                    if !async_scoped(slots.scope(), &values, resources)? {
                        return Err(RuntimeError::from(
                            TrapReason::AsyncStateRetainsAnInvalidOrScopedValue,
                        )
                        .into());
                    }
                    if objects[0].ptr_eq(objects[1]) {
                        return Err(RuntimeError::from(TrapReason::DuplicateFutureOperands).into());
                    }
                    let recipes = [
                        recipe(slots.scope(), objects[0])?,
                        recipe(slots.scope(), objects[1])?,
                    ];
                    let [left, right] = FuturePayload::claim_pair(slots.scope(), objects)?
                        .ok_or_else(|| {
                            RuntimeError::from(TrapReason::FutureHasAlreadyBeenAwaited)
                        })?;
                    let [left_recipe, right_recipe] = recipes;
                    Ok([
                        ClaimedFuture {
                            recipe: left_recipe,
                            source: left,
                        },
                        ClaimedFuture {
                            recipe: right_recipe,
                            source: right,
                        },
                    ])
                })())
            })
        });
        result
            .map_err(|error| self.future_storage_error(error))?
            .map_err(|error| {
                self.resources
                    .admit_execution_error(error.into_runtime(self))
            })
    }

    pub(in crate::native::future) fn future_storage_error(&self, error: GcError) -> RuntimeError {
        match error {
            GcError::ForeignHeap
            | GcError::InvalidRoot
            | GcError::Index
            | GcError::Representation => RuntimeError::module_validation(error.to_string()),
            error => self.managed_memory_error(error),
        }
    }
}

/// Pure immutable observation. This does not authorize submission or retain the
/// selected graph: the input source remains owned through claim/frame transfer.
pub(crate) struct FutureContract {
    pub(crate) owner: ExecutableModule,
    pub(crate) applied: TypeArgument,
    pub(crate) script: Option<Arc<ClosureRecord>>,
}

impl FutureContract {
    /// Borrow the still-cold computation without consuming its one-shot claim.
    pub(crate) fn body<'gc>(
        cx: &Scope<'gc>,
        value: ScopedValue<'gc>,
    ) -> Result<Option<Gc<'gc, RevisionState>>, GcError> {
        let future = payload(cx, value)?;
        Ok(FuturePayload::cold(cx, future).map(|cold| ColdObject::body(cx, cold)))
    }

    pub(crate) fn read<'gc>(cx: &Scope<'gc>, value: ScopedValue<'gc>) -> Result<Self, GcError> {
        let future = payload(cx, value)?;
        let object = cx.downcast::<NativeObject>(value.object().ok_or(GcError::Representation)?)?;
        let (owner, applied) = cx.read(object, |object| {
            (object.owner().clone(), object.applied().clone())
        });
        let script = if let Some(cold) = FuturePayload::cold(cx, future)
            && cx.read(cold, |cold| {
                matches!(cold.recipe().as_ref(), ColdFuture::Script { .. })
            }) {
            let value = ColdObject::capture(cx, cold, 0)?;
            let closure =
                cx.downcast::<ClosureObject>(value.object().ok_or(GcError::Representation)?)?;
            Some(cx.read(closure, |closure| closure.record().clone()))
        } else {
            None
        };
        Ok(Self {
            owner,
            applied,
            script,
        })
    }
}

fn recipe<'gc>(
    cx: &Scope<'gc>,
    object: Gc<'gc, FuturePayload>,
) -> Result<Arc<ColdFuture>, CheckError> {
    let cold = FuturePayload::cold(cx, object)
        .ok_or_else(|| RuntimeError::from(TrapReason::FutureHasAlreadyBeenAwaited))?;
    Ok(cx.read(cold, |cold| cold.recipe().clone()))
}

pub(in crate::native::future) fn payload<'gc>(
    cx: &Scope<'gc>,
    value: ScopedValue<'gc>,
) -> Result<Gc<'gc, FuturePayload>, GcError> {
    if value.kind() != Some(ObjectKind::Native) {
        return Err(GcError::Representation);
    }
    let object = cx.downcast::<NativeObject>(value.object().ok_or(GcError::Representation)?)?;
    if cx.read(object, |object| object.storage().layout()) != NativeStorageLayout::Future {
        return Err(GcError::Representation);
    }
    cx.downcast(NativeObject::payload(cx, object))
}

pub(crate) fn observe(
    runtime: &mut Runtime,
    root: &RootedValue,
) -> NativeResult<Option<CallableObservation>> {
    enum Observed {
        Native(Option<CallableObservation>),
        Script(Arc<ClosureRecord>),
    }
    let result = runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let object = payload(cx, root.get(cx)?)?;
            let Some(cold) = FuturePayload::cold(cx, object) else {
                return Ok(Observed::Native(None));
            };
            let recipe = cx.read(cold, |cold| cold.recipe().clone());
            match recipe.as_ref() {
                ColdFuture::Native(native) => Ok(Observed::Native(native.origin)),
                ColdFuture::Composition(_) => Ok(Observed::Native(None)),
                ColdFuture::Script { .. } => {
                    let closure = ColdObject::capture(cx, cold, 0)?;
                    let closure = cx.downcast::<ClosureObject>(
                        closure.object().ok_or(GcError::Representation)?,
                    )?;
                    Ok(Observed::Script(
                        cx.read(closure, |closure| closure.record().clone()),
                    ))
                }
            }
        })
        .map_err(|error| runtime.future_storage_error(error))?;
    match result {
        Observed::Native(value) => Ok(value),
        Observed::Script(closure) => closure.observe(runtime).map(Some),
    }
}
