//! The selected builtin scalar comparator cannot collect or invoke user code.
#[cfg(feature = "execution-diagnostics")]
use crate::collections::diagnostics;
use crate::collections::vector::{
    VecStorage,
    buffer::SequenceStorage,
    storage::{Work, recover},
};
use kagari_contract::standard::RuntimePrimitive;
use kagari_runtime::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    native::{
        binding::NativeResult, context::CallContext, storage::access::EditPlan,
        views::NativeOperationKind,
    },
};
use std::{
    cmp::Ordering,
    ops::Range,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
};

/// Match the selected intrinsic and actual pointer-free representation before
/// preparing any collecting comparator or owning receiver adapter.
pub(crate) fn try_sort_scalar(call: &CallContext<'_>) -> NativeResult<bool> {
    let selected = call.selected_at(0)?;
    let element = call.argument_type_parameter(0, 0)?;
    if selected.primitive() != Some(RuntimePrimitive::ValueCmp)
        || selected.parameters() != [element.ty().clone(), element.ty().clone()]
    {
        return Ok(false);
    }
    let object = call.payload::<VecStorage>(0)?;
    let mut cx = call.native_context()?;
    // SAFETY: Only layout and length leave this call-rooted payload loan.
    let (scalar, count) = unsafe {
        object.read(&cx, None, |v, _| {
            Ok((!matches!(v.values, SequenceStorage::Traced(_)), v.len()))
        })?
    };
    if !scalar {
        return Ok(false);
    }
    let runtime = cx.runtime();
    if count <= 512 {
        // SAFETY: A bounded scalar leaf has no payload scratch or callouts. The
        // loan itself excludes competing access, and Rust restores its permutation
        // on unwind. Publish the revision before the first possible slot write.
        unsafe {
            object.edit(
                &mut cx,
                None,
                EditPlan {
                    max_logical_units: count,
                    structural: true,
                },
                |v, access| {
                    access.commit_structure();
                    sort_storage(v, runtime, count)
                },
            )?;
        }
        return Ok(true);
    }
    // SAFETY: Recovery restores only same-payload scalar transfers, without
    // allocation or callbacks, including after cancellation and unordered floats.
    let guard = unsafe {
        object.try_enter_operation(&cx, NativeOperationKind::ExclusiveSlots, Some(recover))?
    }
    .ok_or_else(|| RuntimeError::module_validation("Vec slots are protected"))?;
    // SAFETY: Verified ValueCmp plus actual scalar buffers exclude user dispatch.
    // Polls cannot collect/reenter. Main/scratch/work stay in the same payload.
    unsafe {
        object.edit(
            &mut cx,
            Some(&guard),
            EditPlan {
                max_logical_units: count
                    .checked_add(count / 2)
                    .ok_or_else(|| RuntimeError::resource_limit("Vec merge work"))?,
                structural: false,
            },
            |v, _| {
                v.scratch
                    .try_reserve(count / 2)
                    .map_err(|_| RuntimeError::resource_limit("Vec sort scratch"))?;
                #[cfg(feature = "execution-diagnostics")]
                diagnostics::update(|counts| {
                    counts.scratch_peak_bytes =
                        counts.scratch_peak_bytes.max(v.scratch.retained_bytes())
                });
                sort_storage(v, runtime, count)
            },
        )?;
    }
    Ok(true)
}

fn sort_storage(v: &mut VecStorage, runtime: &Runtime, count: usize) -> NativeResult<()> {
    // Both buffers were selected from the same admitted element type. Matching
    // once preserves that representation proof for every transfer/comparison.
    macro_rules! scalar {
        ($($kind:ident),+) => {
            match (&mut v.values, &mut v.scratch) {
                $((SequenceStorage::$kind(values), SequenceStorage::$kind(scratch)) => {
                    sort_range(values, scratch, &mut v.work, runtime, 0, count)?;
                },)+
                _ => unreachable!("matched pointer-free storage"),
            }
        };
    }
    scalar!(
        Unit, Bool, Char, I8, I16, I32, I64, U8, U16, U32, U64, F32, F64
    );
    runtime.resources().poll_execution()
}

fn sort_range<T: Copy + PartialOrd>(
    values: &mut [T],
    scratch_values: &mut Vec<T>,
    work: &mut Option<Work>,
    runtime: &Runtime,
    start: usize,
    end: usize,
) -> NativeResult<()> {
    if end - start <= 512 {
        return sort_leaf(&mut values[start..end], runtime);
    }
    let middle = start + (end - start) / 2;
    sort_range(values, scratch_values, work, runtime, start, middle)?;
    sort_range(values, scratch_values, work, runtime, middle, end)?;
    merge_range(values, scratch_values, work, runtime, start..end, middle)
}

fn merge_range<T: Copy + PartialOrd>(
    values: &mut [T],
    scratch_values: &mut Vec<T>,
    work: &mut Option<Work>,
    runtime: &Runtime,
    range: Range<usize>,
    middle: usize,
) -> NativeResult<()> {
    let Range { start, end } = range;
    for chunk in values[start..middle].chunks(64) {
        runtime.resources().poll_execution()?;
        scratch_values.extend_from_slice(chunk);
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::update(|counts| counts.scratch_copies += chunk.len() as u64);
    }
    let mut merge = Merge {
        work,
        scratch: 0,
        other: middle,
        output: start,
        end,
        complete: false,
    };
    while merge.scratch < scratch_values.len() {
        runtime.resources().poll_execution()?;
        for _ in 0..64 {
            if merge.scratch == scratch_values.len() {
                break;
            }
            let left = scratch_values[merge.scratch];
            let take_left = if merge.other == end {
                true
            } else {
                #[cfg(feature = "execution-diagnostics")]
                diagnostics::update(|counts| counts.comparisons += 1);
                // Same PartialOrd semantics as builtin_order for admitted scalars.
                left.partial_cmp(&values[merge.other]).ok_or_else(|| {
                    RuntimeError::new(
                        RuntimeErrorKind::ScriptTrap,
                        "total comparison cannot be unordered",
                    )
                })? != Ordering::Greater
            };
            values[merge.output] = if take_left { left } else { values[merge.other] };
            #[cfg(feature = "execution-diagnostics")]
            diagnostics::update(|counts| counts.output_copies += 1);
            merge.scratch += usize::from(take_left);
            merge.other += usize::from(!take_left);
            merge.output += 1;
        }
    }
    merge.complete = true;
    scratch_values.clear();
    Ok(())
}

// Only pointer-free elements enter this bounded leaf. The standard library's
// unwind guard restores its permutation before we release the payload loan.
// A leaf has at most 512 pointer-free elements (at most 4 KiB). Poll before
// and after this finite kernel; larger sorts also poll every 64 merge transfers.
// Unordered floats unwind immediately, never supplying a synthetic comparison.
struct ScalarAbort(RuntimeError);

#[cold]
fn abort(error: RuntimeError) -> ! {
    resume_unwind(Box::new(ScalarAbort(error)))
}

fn sort_leaf<T: Copy + PartialOrd>(values: &mut [T], runtime: &Runtime) -> NativeResult<()> {
    runtime.resources().poll_execution()?;
    #[cfg(feature = "execution-diagnostics")]
    diagnostics::update(|counts| {
        counts.scalar_leaf_calls += 1;
        counts.scalar_leaf_elements += values.len() as u64;
        counts.scalar_leaf_stack_bound_bytes = 4096;
    });
    let result = catch_unwind(AssertUnwindSafe(|| {
        values.sort_by(|a, b| {
            #[cfg(feature = "execution-diagnostics")]
            diagnostics::update(|counts| counts.comparisons += 1);
            a.partial_cmp(b).unwrap_or_else(|| {
                abort(RuntimeError::new(
                    RuntimeErrorKind::ScriptTrap,
                    "total comparison cannot be unordered",
                ))
            })
        });
    }));
    if let Err(payload) = result {
        match payload.downcast::<ScalarAbort>() {
            Ok(error) => return Err(error.0),
            Err(payload) => resume_unwind(payload),
        }
    }
    runtime.resources().poll_execution()
}

// A scalar loan excludes every scanner and callback. Publish the authoritative
// gap once when releasing that loan, including error/unwind; no per-transfer
// volatile payload bookkeeping is needed while its checked exclusion is active.
struct Merge<'a> {
    work: &'a mut Option<Work>,
    scratch: usize,
    other: usize,
    output: usize,
    end: usize,
    complete: bool,
}

impl Drop for Merge<'_> {
    fn drop(&mut self) {
        *self.work = (!self.complete).then_some(Work::Merge {
            scratch: self.scratch,
            other: self.other,
            output: self.output,
            end: self.end,
        });
    }
}

#[cfg(test)]
mod tests;
