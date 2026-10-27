//! Short call-rooted mutations share the owning handle's storage implementation.
use crate::collections::vector::{VecStorage, index_error};
use kagari_runtime::{
    error::{RuntimeError, RuntimeErrorKind},
    native::{binding::NativeResult, context::CallContext, storage::access::EditPlan},
};

fn length(cx: &CallContext<'_>) -> NativeResult<usize> {
    // SAFETY: Only scalar metadata leaves the loan.
    unsafe { cx.with_payload::<VecStorage, _>(0, |v| Ok(v.len())) }
}

pub(crate) fn clear(cx: &CallContext<'_>) -> NativeResult<()> {
    let count = length(cx)?;
    // SAFETY: Removing edges needs no imported publication; the shared storage
    // implementation commits the revision before the first changed slot.
    unsafe {
        cx.edit_payload::<VecStorage, _>(
            0,
            EditPlan {
                max_logical_units: count,
                structural: true,
            },
            |v, access| {
                v.truncate(access, 0);
                Ok(())
            },
        )
    }
}

pub(crate) fn set(cx: &CallContext<'_>, index: usize, argument: usize) -> NativeResult<()> {
    let count = length(cx)?;
    if index >= count {
        return Err(index_error());
    }
    // SAFETY: Exact argument publication precedes replacing the destination edge.
    unsafe {
        cx.edit_payload::<VecStorage, _>(
            0,
            EditPlan {
                max_logical_units: count,
                structural: false,
            },
            |v, access| {
                let value = access.publish_argument(&v.contract, cx, argument)?;
                v.set(index, value)
            },
        )
    }
}

pub(crate) fn insert(
    cx: &CallContext<'_>,
    index: Option<usize>,
    argument: usize,
) -> NativeResult<()> {
    let count = length(cx)?;
    let index = index.unwrap_or(count);
    if index > count {
        return Err(RuntimeError::new(
            RuntimeErrorKind::IndexOutOfBounds,
            format!("Vec insert index {index} exceeds length {count}"),
        ));
    }
    let units = count
        .checked_add(1)
        .ok_or_else(|| RuntimeError::resource_limit("Vec length"))?;
    // SAFETY: Import is checked against the exact call argument before the shared
    // implementation reserves, republishes moved edges, and commits its revision.
    unsafe {
        cx.edit_payload::<VecStorage, _>(
            0,
            EditPlan {
                max_logical_units: units,
                structural: true,
            },
            |v, access| {
                let value = access.publish_argument(&v.contract, cx, argument)?;
                v.insert(access, index, value)
            },
        )
    }
}

/// The caller must retain any extracted element before invoking this removal.
pub(crate) fn remove(cx: &CallContext<'_>, index: usize) -> NativeResult<()> {
    let count = length(cx)?;
    // SAFETY: This entry returns no extracted edge, and relocation uses the same
    // publication and revision implementation as the owning adapter.
    unsafe {
        cx.edit_payload::<VecStorage, _>(
            0,
            EditPlan {
                max_logical_units: count,
                structural: true,
            },
            |v, access| {
                v.remove(access, index);
                Ok(())
            },
        )
    }
}
