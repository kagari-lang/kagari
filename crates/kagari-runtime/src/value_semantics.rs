//! Script equality reads actual managed providers under a closed scope.
pub(crate) mod hashing;
use crate::{
    Runtime,
    error::{
        RuntimeError,
        fault::EngineFault,
        reason::{CancellationReason, ErrorReason},
    },
    resource::ResourceState,
    value::carriers::{RootedValue, ScopedValue},
    value_check,
};
use kagari_gc::{access::Scope, error::GcError};
use std::cmp::Ordering;

pub(crate) mod comparison;

/// Compare mutable object identity through explicitly retained Host inputs.
/// Strings, tuples, enums, interfaces and functions are not identity operands.
pub fn identity_equal(
    runtime: &mut Runtime,
    lhs: &RootedValue,
    rhs: &RootedValue,
) -> Result<bool, RuntimeError> {
    inspect_pair(runtime, lhs, rhs, |_, _, left, right| {
        comparison::identity_equal(left, right)
    })
}

/// Compare builtin value content, including nominal enum/variant identity.
/// Member traversal short-circuits left to right; it never invokes script code.
pub fn script_equal(
    runtime: &mut Runtime,
    lhs: &RootedValue,
    rhs: &RootedValue,
) -> Result<bool, RuntimeError> {
    inspect_pair(runtime, lhs, rhs, comparison::script_equal)
}

fn inspect_pair<R>(
    runtime: &mut Runtime,
    lhs: &RootedValue,
    rhs: &RootedValue,
    inspect: impl for<'gc> FnOnce(
        &Scope<'gc>,
        &ResourceState,
        ScopedValue<'gc>,
        ScopedValue<'gc>,
    ) -> Result<R, RuntimeError>,
) -> Result<R, RuntimeError> {
    runtime.resources.ensure_synchronous_thread()?;
    runtime.resources.poll_execution()?;
    value_check::candidate_root(runtime, lhs)?;
    value_check::candidate_root(runtime, rhs)?;
    let resources = &runtime.resources;
    let result = runtime.memory.scope(&runtime.resources, |cx, _| {
        Ok(inspect(cx, resources, lhs.get(cx)?, rhs.get(cx)?))
    });
    result
        .map_err(|error| input_error(runtime, error))?
        .map_err(|error| admit_error(runtime, error))
}

pub(crate) fn storage_error(error: GcError) -> RuntimeError {
    match error {
        GcError::Allocation => RuntimeError::new(ErrorReason::AllocationFailure),
        GcError::SourceLimit => RuntimeError::new(ErrorReason::StackOverflow),
        GcError::Limit(name) => RuntimeError::resource_limit(name),
        GcError::Closed => CancellationReason::RuntimeIsClosed.into(),
        error => EngineFault::ManagedCollectionFailed(error).into(),
    }
}

fn input_error(runtime: &Runtime, error: GcError) -> RuntimeError {
    match error {
        GcError::ForeignHeap | GcError::InvalidRoot | GcError::Index | GcError::Representation => {
            RuntimeError::module_validation("foreign or expired value handle")
        }
        error => runtime.managed_memory_error(error),
    }
}

fn admit_error(runtime: &Runtime, error: RuntimeError) -> RuntimeError {
    if let ErrorReason::EngineFault(reason) = error.reason() {
        runtime.resources.quarantine(*reason)
    } else {
        error.admit_execution_error(&runtime.resources)
    }
}

pub(crate) mod formatting;

/// Produce a bounded preview without invoking user formatting or retaining roots.
/// Object identity text is opaque and valid only for the allocation's lifetime.
pub fn format_value(
    runtime: &mut Runtime,
    value: &RootedValue,
    debug: bool,
) -> Result<String, RuntimeError> {
    runtime.resources.ensure_synchronous_thread()?;
    runtime.resources.poll_execution()?;
    value_check::candidate_root(runtime, value)?;
    let resources = &runtime.resources;
    let result = runtime.memory.scope(&runtime.resources, |cx, _| {
        Ok(formatting::format_value(
            cx,
            &mut || resources.poll_closed(),
            value.get(cx)?,
            debug,
        ))
    });
    result
        .map_err(|error| input_error(runtime, error))?
        .map_err(|error| admit_error(runtime, error))
}

/// Builtin ordering consumes retained values without user protocol dispatch.
pub fn builtin_order(
    runtime: &mut Runtime,
    lhs: &RootedValue,
    rhs: &RootedValue,
) -> Result<Option<Ordering>, RuntimeError> {
    inspect_pair(runtime, lhs, rhs, comparison::builtin_order)
}

#[cfg(test)]
mod tests;
