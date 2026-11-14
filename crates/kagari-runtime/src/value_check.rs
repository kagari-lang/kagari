//! Runtime value/type compatibility consumes checked layouts and lexical type facts.
//! Heap storage supplies descriptors; the collector does not resolve type policy.
pub(crate) mod data;
pub(crate) mod scoped;
use crate::{
    Runtime,
    error::RuntimeError,
    execution::roots::ExecutionRoots,
    frame::{
        arguments::FrameArguments,
        types::{arguments::TypeArgument, compatibility::TypeView},
    },
    module::ExecutableModule,
    value::{carriers::RootedValue, properties::ValueProperties, scalar, view::ValueView},
    value_check::data::DataWalk,
};
use kagari_bytecode::module::BytecodeFunction;
use kagari_gc::error::GcError;
use kagari_types::ty::Ty;

/// Admit an owned entry's original roots against the selected verified body's
/// lexical parameter facts. These views never escape this closed read, so no
/// reified TypeArguments, conversion scratch or extra result roots are needed.
pub(crate) fn entry_arguments(
    runtime: &mut Runtime,
    owner: &ExecutableModule,
    function: &BytecodeFunction,
    arguments: &[RootedValue],
) -> Result<(), RuntimeError> {
    let invalid = || RuntimeError::module_validation("invalid owned execution arguments");
    if arguments.len() != usize::from(function.parameter_count)
        || (0..arguments.len()).any(|index| !function.metadata.semantic.params.contains_key(&index))
    {
        return Err(invalid());
    }
    // Reject unresolved entry binders before inspecting any supplied root, as
    // reification did. Definition-prefix validity already belongs to this
    // selected verified body; no owned type copy or escaping witness is needed.
    for index in 0..arguments.len() {
        if TypeView::new(&function.metadata.semantic.params[&index], owner, None)
            .closed()
            .is_none_or(|ty| !ty.is_concrete())
        {
            return Err(RuntimeError::module_validation("type argument scope"));
        }
    }
    if arguments.is_empty() {
        return Ok(());
    }
    if arguments.iter().enumerate().all(|(index, argument)| {
        matches!(function.metadata.semantic.params[&index], Ty::Builtin(_))
            && argument.scalar_payload().is_some()
    }) {
        // Pure bits need neither a heap brand nor a source loan. Keep terminal
        // state admission and the matcher's per-argument cancellation checkpoint.
        runtime
            .memory
            .check_scalar_access()
            .map_err(|error| runtime.managed_input_error(error))?;
        for (index, argument) in arguments.iter().enumerate() {
            runtime
                .resources
                .poll_closed()
                .map_err(|error| runtime.resources.admit_execution_error(error))?;
            let Ty::Builtin(ty) = function.metadata.semantic.params[&index] else {
                unreachable!("checked scalar parameter pack")
            };
            if !scalar::matches_builtin_payload(ty, argument.scalar_payload().unwrap()) {
                return Err(invalid());
            }
        }
        return Ok(());
    }
    let Runtime {
        memory,
        resources,
        host,
        host_borrows,
        ..
    } = runtime;
    let result = memory.scope(resources, |cx, _| {
        Ok((|| -> Result<bool, scoped::CheckError> {
            for (index, argument) in arguments.iter().enumerate() {
                if index != 0 && index.is_multiple_of(1024) {
                    resources.poll_closed()?;
                }
                let value = argument.get(cx)?;
                if !ValueProperties::of(cx, value)?.storable {
                    return Ok(false);
                }
                let expected =
                    TypeView::new(&function.metadata.semantic.params[&index], owner, None);
                if !scoped::matches(cx, value, expected, host, host_borrows, resources)? {
                    return Ok(false);
                }
            }
            Ok(true)
        })())
    });
    let matched = result
        .map_err(|error| runtime.managed_input_error(error))?
        .map_err(|error| {
            runtime
                .resources
                .admit_execution_error(error.into_runtime(runtime))
        })?;
    if matched { Ok(()) } else { Err(invalid()) }
}

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

/// Match retained inputs in type/provenance fixtures. Typed conversion uses
/// protected views; owned entry borrows its selected body's facts above.
#[cfg(test)]
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
