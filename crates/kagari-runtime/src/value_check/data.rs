//! Data admission inspects actual mutable edges, excluding executable metadata.
#[cfg(test)]
mod tests;
use crate::{
    Runtime,
    error::{RuntimeError, trap::TrapReason},
    execution::roots::{
        ExecutionRoots, SessionId, Slots, ValueBank, ValueLocation, frames::FrameBanks,
    },
    execution_metadata::managed::{
        applications::ApplicationObject, environments::EnvironmentObject,
        groups::OperationGroupObject,
    },
    frame::types::arguments::TypeArgument,
    host::HostRegistryId,
    module::{ExecutableModule, ModuleKey, managed::revision::RevisionState},
    native::{
        conversion::arguments::ArgumentValues,
        future::storage::{ColdObject, FuturePayload},
    },
    resource::{ResourceState, candidate::CandidateOrigin},
    task::payload::{ScopePayload, TaskPayload},
    value::{
        aggregates::{structures::StructObject, tuples::TupleObject},
        arrays::ArrayObject,
        carriers::{RootedValue, ScopedValue},
        cells::CellObject,
        closures::ClosureObject,
        host_values::{EphemeralObject, HostRootObject, paths::HostPathObject},
        interfaces::InterfaceObject,
        native::NativeObject,
        properties::ValueProperties,
        view::ValueView,
    },
    value_check::scoped::{self, CheckError},
};
use kagari_gc::{
    access::{Scope, inspection::EdgeInspector},
    error::GcError,
    object::{AnyObject, Gc, GcObject},
};
use std::{
    collections::HashSet,
    ops::{ControlFlow, Range},
    panic::{AssertUnwindSafe, catch_unwind},
    slice,
};

pub(crate) fn async_protected(
    runtime: &mut Runtime,
    values: &[ValueView<'_>],
) -> Result<(), RuntimeError> {
    runtime.resources.poll_execution()?;
    let resources = &runtime.resources;
    let result = runtime.memory.scope(&runtime.resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            Ok(async_inputs(
                slots,
                values.len(),
                |slots, index| values[index].get(slots),
                resources,
            ))
        })
    });
    let valid = result
        .map_err(|error| runtime.managed_input_error(error))?
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

pub(crate) fn async_arguments(
    slots: &mut Slots<'_, '_>,
    values: ArgumentValues<'_>,
    resources: &ResourceState,
) -> Result<bool, CheckError> {
    values.check_bounds(slots)?;
    async_inputs(
        slots,
        values.len(),
        |slots, index| values.scoped(slots, index),
        resources,
    )
}

fn async_inputs<'gc>(
    slots: &mut Slots<'_, 'gc>,
    count: usize,
    mut get: impl FnMut(&mut Slots<'_, 'gc>, usize) -> Result<ScopedValue<'gc>, GcError>,
    resources: &ResourceState,
) -> Result<bool, CheckError> {
    for index in 0..count {
        if index.is_multiple_of(1024) {
            resources.poll_closed()?;
        }
        let value = get(slots, index)?;
        if !ValueProperties::of(slots.scope(), value)?.storable {
            return Ok(false);
        }
    }
    let mut walk = DataWalk::default();
    for index in 0..count {
        if index.is_multiple_of(1024) {
            resources.poll_closed()?;
        }
        if let Some(object) = get(slots, index)?.object()
            && !walk.object(slots.scope(), object, resources)?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Ordinary async data may contain durable Future/Task values. It cannot hide
/// external descriptors or ephemeral capabilities behind mutable/erased fields.
/// The whole walk borrows a closed scope: no collection, mutation or executing
/// callback can invalidate its visited set. No admission result is cached.
pub(crate) fn async_scoped<'gc>(
    cx: &Scope<'gc>,
    values: &[ScopedValue<'gc>],
    resources: &ResourceState,
) -> Result<bool, CheckError> {
    for (index, value) in values.iter().enumerate() {
        if index.is_multiple_of(1024) {
            resources.poll_closed()?;
        }
        if !ValueProperties::of(cx, *value)?.storable {
            return Ok(false);
        }
    }
    let mut walk = DataWalk::default();
    for value in values {
        if let Some(object) = value.object()
            && !walk.object(cx, object, resources)?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Recheck a computation graph after a queued interval. Its genuine source root
/// owns the physical node; no language tag or copied capture pack is invented.
pub(crate) fn async_object<'gc>(
    cx: &Scope<'gc>,
    object: Gc<'gc, AnyObject>,
    resources: &ResourceState,
) -> Result<bool, CheckError> {
    DataWalk::default().object(cx, object, resources)
}

/// One closed parking proof shares visitation across frames and auxiliary roots.
/// The frame owner decides control-state precedence before submitting each range.
pub(crate) struct AsyncDataWalk<'gc>(DataWalk<'gc>);

impl Default for AsyncDataWalk<'_> {
    fn default() -> Self {
        Self(DataWalk {
            checkpoint: DataCheckpoint::ParkingCommit,
            ..DataWalk::default()
        })
    }
}

impl<'gc> AsyncDataWalk<'gc> {
    pub(crate) fn frame(
        &mut self,
        slots: &mut Slots<'_, 'gc>,
        execution: SessionId,
        banks: FrameBanks,
        resources: &ResourceState,
    ) -> Result<bool, CheckError> {
        if banks.execution != execution {
            return Err(GcError::Representation.into());
        }
        slots.check_frame_banks(banks)?;
        let end = banks
            .managed
            .checked_add(banks.managed_count)
            .ok_or(GcError::Index)?;
        self.bank(
            slots,
            execution,
            ValueBank::Managed,
            banks.managed..end,
            resources,
        )
    }

    /// Local/queued/pending values and typed computation dependencies are actual
    /// retained roots too. Metadata-only revisions/environments remain excluded
    /// by the same physical walk; method dependencies can own receiver data.
    pub(crate) fn remaining(
        &mut self,
        slots: &mut Slots<'_, 'gc>,
        execution: SessionId,
        resources: &ResourceState,
    ) -> Result<bool, CheckError> {
        for bank in [ValueBank::Local, ValueBank::Queued, ValueBank::Pending] {
            let count = slots.len(execution, bank)?;
            if !self.bank(slots, execution, bank, 0..count, resources)? {
                return Ok(false);
            }
        }
        let count = slots.dependency_count(execution)?;
        for index in 0..count {
            if index.is_multiple_of(1024) {
                resources.ensure_execution_allowed()?;
            }
            let object = slots.dependency_at(execution, index)?;
            if !self.0.object(slots.scope(), object, resources)? {
                return Err(RuntimeError::from(
                    TrapReason::AsyncStateRetainsAnInvalidOrScopedValue,
                )
                .into());
            }
        }
        Ok(true)
    }

    fn bank(
        &mut self,
        slots: &mut Slots<'_, 'gc>,
        execution: SessionId,
        bank: ValueBank,
        range: Range<usize>,
        resources: &ResourceState,
    ) -> Result<bool, CheckError> {
        // Check all direct operands before descending, preserving the existing
        // per-frame distinction between a declined transition and a graph trap.
        for slot in range.clone() {
            if slot.is_multiple_of(1024) {
                resources.ensure_execution_allowed()?;
            }
            let value = slots.get(ValueLocation {
                execution,
                bank,
                slot,
            })?;
            if !ValueProperties::of(slots.scope(), value)?.storable {
                return Ok(false);
            }
        }
        for slot in range {
            if slot.is_multiple_of(1024) {
                resources.ensure_execution_allowed()?;
            }
            let value = slots.get(ValueLocation {
                execution,
                bank,
                slot,
            })?;
            if let Some(object) = value.object()
                && !self.0.object(slots.scope(), object, resources)?
            {
                return Err(RuntimeError::from(
                    TrapReason::AsyncStateRetainsAnInvalidOrScopedValue,
                )
                .into());
            }
        }
        Ok(true)
    }
}

/// Pending parking is a commit of an already checked poll. A cancellation wake
/// remains ready for the next drive; it must not interrupt this source handoff.
/// Both modes still reject a faulted Runtime or an already terminated session.
#[derive(Default, Clone, Copy)]
enum DataCheckpoint {
    #[default]
    Executing,
    ParkingCommit,
}

impl DataCheckpoint {
    fn check(self, resources: &ResourceState) -> Result<(), RuntimeError> {
        match self {
            Self::Executing => resources.poll_closed(),
            Self::ParkingCommit => resources.ensure_execution_allowed(),
        }
    }
}

/// Only this closed batch can reuse visited identities. A later admission starts
/// fresh, because an alias may have changed any mutable/private descendant.
#[derive(Default)]
pub(super) struct DataWalk<'gc> {
    pending: Vec<Gc<'gc, AnyObject>>,
    seen: HashSet<Gc<'gc, AnyObject>>,
    scratch: EdgeInspector,
    candidate: Option<(HostRegistryId, ModuleKey)>,
    checkpoint: DataCheckpoint,
}

impl<'gc> DataWalk<'gc> {
    pub(super) fn candidate(owner: (HostRegistryId, ModuleKey)) -> Self {
        Self {
            candidate: Some(owner),
            ..Self::default()
        }
    }

    pub(super) fn value(
        &mut self,
        cx: &Scope<'gc>,
        value: ScopedValue<'gc>,
        resources: &ResourceState,
    ) -> Result<bool, CheckError> {
        if !ValueProperties::of(cx, value)?.storable {
            return Ok(false);
        }
        match value.object() {
            Some(object) => self.object(cx, object, resources),
            None => Ok(true),
        }
    }

    fn object(
        &mut self,
        cx: &Scope<'gc>,
        object: Gc<'gc, AnyObject>,
        resources: &ResourceState,
    ) -> Result<bool, CheckError> {
        let checkpoint = self.checkpoint;
        // The first node is already available. Metadata-only dependencies do
        // not descend, so admitting them must not allocate traversal storage.
        let mut first = Some(object);
        while let Some(object) = first.take().or_else(|| self.pending.pop()) {
            // Candidate ownership admission precedes the call's cancellation
            // checkpoint. Descendant scanning remains cooperatively polled.
            if let Some(owner) = self.candidate
                && !candidate_object(cx, object, owner)?
            {
                return Ok(false);
            }
            checkpoint.check(resources)?;
            // A retained closure/Future's executable graph can reach statics. Those
            // are not captured data. Physical custom payload edges still use the same
            // scanner; no registration-specific list can omit private managed fields.
            if metadata(cx, object)? {
                continue;
            }
            self.seen.try_reserve(1)?;
            if !self.seen.insert(object) {
                continue;
            }
            if is::<HostRootObject>(cx, object)?
                || is::<HostPathObject>(cx, object)?
                || is::<EphemeralObject>(cx, object)?
            {
                return Ok(false);
            }
            match cx.downcast::<TupleObject>(object) {
                Ok(tuple) if !cx.read(tuple, TupleObject::properties).storable => return Ok(false),
                Ok(_) | Err(GcError::Representation) => {}
                Err(error) => return Err(error.into()),
            }
            let mut until_poll = 1024;
            // Unlike the public GC inspector, both visit/checkpoint closures
            // here are engine code. A panic can only violate the provider/engine
            // contract; return it for quarantine after the closed loans retire.
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                cx.inspect_edges(
                    object,
                    &mut self.scratch,
                    |child| {
                        until_poll -= 1;
                        if until_poll == 0 {
                            until_poll = 1024;
                            if let Err(error) = checkpoint.check(resources) {
                                return ControlFlow::Break(CheckError::Execution(error));
                            }
                        }
                        if let Err(error) = self.pending.try_reserve(1) {
                            return ControlFlow::Break(CheckError::from(error));
                        }
                        self.pending.push(child);
                        ControlFlow::Continue(())
                    },
                    || match checkpoint.check(resources) {
                        Ok(()) => ControlFlow::Continue(()),
                        Err(error) => ControlFlow::Break(CheckError::Execution(error)),
                    },
                )
            }))
            .map_err(|_| GcError::Provider("data inspector scanner panicked"))??;
            if let ControlFlow::Break(error) = outcome {
                return Err(error);
            }
        }
        Ok(true)
    }
}

/// Dereferencing one operand repeats ownership admission, since a Host may hold
/// its source independently of the current call's admitted argument prefix.
/// Full transfer/output admission additionally walks all transitive data.
pub(crate) fn candidate_access<'gc>(
    cx: &Scope<'gc>,
    value: ScopedValue<'gc>,
    resources: &ResourceState,
) -> Result<(), CheckError> {
    if value.object().is_none() && value.as_runtime_ephemeral().is_none() {
        return Ok(());
    }
    let Some(owner) = resources.candidate_owner() else {
        return Ok(());
    };
    let valid = ValueProperties::of(cx, value)?.storable;
    let valid = if let Some(object) = value.object() {
        valid
            && !is::<HostRootObject>(cx, object)?
            && !is::<HostPathObject>(cx, object)?
            && !is::<EphemeralObject>(cx, object)?
            && candidate_object(cx, object, owner)?
    } else {
        valid
    };
    if !valid {
        return Err(
            RuntimeError::execution_phase_violation("external object in candidate access").into(),
        );
    }
    Ok(())
}

/// Each mutable language identity is stamped by its allocation scope. Immutable
/// wrappers still scan children; arbitrary Native payload providers use the same
/// scanner, so private edges cannot conceal a disallowed descendant.
fn candidate_object<'gc>(
    cx: &Scope<'gc>,
    object: Gc<'gc, AnyObject>,
    owner: (HostRegistryId, ModuleKey),
) -> Result<bool, GcError> {
    if is::<ColdObject>(cx, object)?
        || is::<FuturePayload>(cx, object)?
        || is::<TaskPayload>(cx, object)?
        || is::<ScopePayload>(cx, object)?
    {
        return Ok(false);
    }
    let checks = [
        origin_matches(cx, object, owner, ArrayObject::origin)?,
        origin_matches(cx, object, owner, StructObject::origin)?,
        origin_matches(cx, object, owner, CellObject::origin)?,
        origin_matches(cx, object, owner, ClosureObject::origin)?,
        origin_matches(cx, object, owner, InterfaceObject::origin)?,
        origin_matches(cx, object, owner, NativeObject::origin)?,
    ];
    Ok(checks.into_iter().flatten().next().unwrap_or(true))
}

fn origin_matches<'gc, T: GcObject>(
    cx: &Scope<'gc>,
    object: Gc<'gc, AnyObject>,
    owner: (HostRegistryId, ModuleKey),
    origin: fn(&T) -> &CandidateOrigin,
) -> Result<Option<bool>, GcError> {
    match cx.downcast::<T>(object) {
        Ok(object) => Ok(Some(
            cx.read(object, |object| origin(object).matches(owner)),
        )),
        Err(GcError::Representation) => Ok(None),
        Err(error) => Err(error),
    }
}

fn metadata<'gc>(cx: &Scope<'gc>, object: Gc<'gc, AnyObject>) -> Result<bool, GcError> {
    match cx.downcast::<ApplicationObject>(object) {
        Ok(application) => return Ok(!cx.read(application, ApplicationObject::owns_receiver_data)),
        Err(GcError::Representation) => {}
        Err(error) => return Err(error),
    }
    Ok(is::<RevisionState>(cx, object)?
        || is::<EnvironmentObject>(cx, object)?
        || is::<OperationGroupObject>(cx, object)?)
}

fn is<'gc, T: GcObject>(cx: &Scope<'gc>, object: Gc<'gc, AnyObject>) -> Result<bool, GcError> {
    match cx.downcast::<T>(object) {
        Ok(_) => Ok(true),
        Err(GcError::Representation) => Ok(false),
        Err(error) => Err(error),
    }
}

/// Shared Task/group result admission preserves nominal checking before data safety.
pub(crate) fn async_result(
    runtime: &mut Runtime,
    root: &RootedValue,
    output: &TypeArgument,
    owner: &ExecutableModule,
) -> Result<(), RuntimeError> {
    runtime.resources.ensure_execution_allowed()?;
    output.validate(runtime)?;
    let Runtime {
        memory,
        host,
        host_borrows,
        resources,
        ..
    } = &mut *runtime;
    let result = memory.scope(resources, |cx, _| {
        let value = root.get(cx)?;
        let matched =
            match scoped::matches(cx, value, output.view(owner), host, host_borrows, resources) {
                Ok(matched) => matched,
                Err(CheckError::Storage(error)) => return Err(error),
                Err(CheckError::Execution(error)) => return Ok(Err(error)),
            };
        if !matched {
            return Ok(Err(RuntimeError::module_validation("async result type")));
        }
        // Full async reachability remains distinct from top-level storability:
        // a trait box may hide a Host receiver. Do not weaken this to a tag test.
        let valid = match async_scoped(cx, slice::from_ref(&value), resources) {
            Ok(valid) => valid,
            Err(CheckError::Storage(error)) => return Err(error),
            Err(CheckError::Execution(error)) => return Ok(Err(error)),
        };
        if !valid {
            return Ok(Err(RuntimeError::from(
                TrapReason::AsyncStateRetainsAnInvalidOrScopedValue,
            )));
        }
        Ok(Ok(()))
    });
    result
        .map_err(|error| runtime.managed_input_error(error))?
        .map_err(|error| runtime.resources.admit_execution_error(error))
}
