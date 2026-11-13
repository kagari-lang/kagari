//! Runtime value/type compatibility consumes checked layouts and lexical type facts.
//! Heap storage supplies descriptors; the collector does not resolve type policy.
pub(crate) mod data;
pub(crate) mod scoped;
use crate::{
    Runtime,
    error::RuntimeError,
    execution::roots::ExecutionRoots,
    frame::{arguments::FrameArguments, types::arguments::TypeArgument},
    module::ExecutableModule,
    value::{carriers::RootedValue, scalar, view::ValueView},
    value_check::data::DataWalk,
};
use kagari_gc::error::GcError;
use kagari_types::ty::Ty;

/// Check an already protected result without creating conversion scratch. The
/// matcher admits actual source/type authority; candidate output policy follows
/// nominal matching, just as it does for a value produced by a converter.
pub(crate) fn check_protected(
    runtime: &mut Runtime,
    owner: &ExecutableModule,
    expected: &TypeArgument,
    value: &ValueView<'_>,
) -> Result<(), RuntimeError> {
    if !matches_protected(runtime, owner, expected, value)? {
        return Err(RuntimeError::module_validation(
            "converted value differs from its declaration",
        ));
    }
    candidate_protected(runtime, &[*value])
}

/// Candidate entry/result policy is separate from nominal matching. Select its
/// immutable owner facts without holding control, then walk actual protected data
/// under the paired scope. Executable metadata edges are not captured data.
pub(crate) fn candidate_protected(
    runtime: &mut Runtime,
    values: &[ValueView<'_>],
) -> Result<(), RuntimeError> {
    let Some(owner) = runtime.resources.candidate_owner() else {
        return Ok(());
    };
    let Runtime {
        memory, resources, ..
    } = runtime;
    let result = memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            Ok((|| {
                let mut walk = DataWalk::candidate(owner);
                for (index, value) in values.iter().enumerate() {
                    if index.is_multiple_of(1024) {
                        resources.poll_closed()?;
                    }
                    let value = value.get(slots)?;
                    if !walk.value(slots.scope(), value, resources)? {
                        return Err(RuntimeError::execution_phase_violation(
                            "external object in candidate host value",
                        )
                        .into());
                    }
                }
                Ok::<_, scoped::CheckError>(())
            })())
        })
    });
    result
        .map_err(|error| runtime.managed_input_error(error))?
        .map_err(|error| {
            runtime
                .resources
                .admit_execution_error(error.into_runtime(runtime))
        })
}

/// Direct Host object access has no argument pack to perform this admission.
/// Walk the genuine root under its heap scope, then admit errors after it ends.
pub(crate) fn candidate_root(
    runtime: &mut Runtime,
    value: &RootedValue,
) -> Result<(), RuntimeError> {
    let Some(owner) = runtime.resources.candidate_owner() else {
        return Ok(());
    };
    let resources = &runtime.resources;
    let result = runtime.memory.scope(&runtime.resources, |cx, _| {
        let value = value.get(cx)?;
        Ok((|| -> Result<(), scoped::CheckError> {
            resources.poll_closed()?;
            if !DataWalk::candidate(owner).value(cx, value, resources)? {
                return Err(RuntimeError::execution_phase_violation(
                    "external object in candidate access",
                )
                .into());
            }
            Ok(())
        })())
    });
    result
        .map_err(|error| runtime.managed_input_error(error))?
        .map_err(|error| {
            runtime
                .resources
                .admit_execution_error(error.into_runtime(runtime))
        })
}

/// Frame arguments retain their original sources; candidate data policy inspects
/// the borrowed receiver/captures/explicit pack without materializing another pack.
pub(crate) fn candidate_arguments(
    runtime: &mut Runtime,
    arguments: FrameArguments<'_, '_>,
) -> Result<(), RuntimeError> {
    let Some(owner) = runtime.resources.candidate_owner() else {
        return Ok(());
    };
    let Runtime {
        memory, resources, ..
    } = runtime;
    let result = memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            Ok((|| -> Result<(), scoped::CheckError> {
                arguments.check_bounds(slots)?;
                let mut walk = DataWalk::candidate(owner);
                for index in 0..arguments.len() {
                    if index != 0 && index.is_multiple_of(1024) {
                        resources.poll_closed()?;
                    }
                    let value = arguments.get(slots, index)?;
                    if !walk.value(slots.scope(), value, resources)? {
                        return Err(RuntimeError::execution_phase_violation(
                            "external object in candidate call arguments",
                        )
                        .into());
                    }
                }
                Ok(())
            })())
        })
    });
    result
        .map_err(|error| runtime.managed_input_error(error))?
        .map_err(|error| {
            runtime
                .resources
                .admit_execution_error(error.into_runtime(runtime))
        })
}

/// Check a Host submission while its original root supplies protection.
/// No conversion-local pack, managed allocation or root registration is needed.
pub(crate) fn matches_rooted(
    runtime: &mut Runtime,
    owner: &ExecutableModule,
    expected: &TypeArgument,
    value: &RootedValue,
) -> Result<bool, RuntimeError> {
    expected.validate(runtime)?;
    let Runtime {
        memory,
        host,
        host_borrows,
        resources,
        ..
    } = runtime;
    let result = memory.scope(resources, |cx, _| {
        let value = value.get(cx)?;
        Ok(scoped::matches_argument(
            cx,
            value,
            expected,
            owner,
            host,
            host_borrows,
            resources,
        ))
    });
    result
        .map_err(|error| runtime.managed_input_error(error))?
        .map_err(|error| {
            runtime
                .resources
                .admit_execution_error(error.into_runtime(runtime))
        })
}

/// Match prepared lexical facts against an existing protected view. This closed
/// read cannot allocate managed output, execute callbacks or retain any value;
/// the original source owns protection, so no conversion-local mark is needed.
pub(crate) fn matches_protected(
    runtime: &mut Runtime,
    owner: &ExecutableModule,
    expected: &TypeArgument,
    value: &ValueView<'_>,
) -> Result<bool, RuntimeError> {
    expected.validate(runtime)?;
    if let (Ty::Builtin(ty), Some(payload)) = (expected.ty(), value.scalar_payload()) {
        // Neither operand can contain a GC/source capability. Keep exact type
        // provenance, terminal-heap rejection and the shared domain predicate;
        // managed or lexical matching still uses paired physical access below.
        runtime
            .memory
            .check_scalar_access()
            .map_err(|error| runtime.managed_memory_error(error))?;
        return Ok(scalar::matches_builtin_payload(*ty, payload));
    }
    let Runtime {
        memory,
        host,
        host_borrows,
        resources,
        ..
    } = runtime;
    let result = memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            let value = value.get(slots)?;
            Ok(scoped::matches_argument(
                slots.scope(),
                value,
                expected,
                owner,
                host,
                host_borrows,
                resources,
            ))
        })
    });
    result
        .map_err(|error| match error {
            GcError::ForeignHeap
            | GcError::InvalidRoot
            | GcError::Index
            | GcError::Representation => RuntimeError::module_validation(error.to_string()),
            error => runtime.managed_memory_error(error),
        })?
        .map_err(|error| {
            let error = error.into_runtime(runtime);
            runtime.resources.admit_execution_error(error)
        })
}
