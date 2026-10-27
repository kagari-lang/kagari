//! Ordinary nominal cursor with a traced source and runtime-owned revision check.
use crate::collections::vector::{ScriptVec, VecStorage};
use kagari_runtime::{
    error::RuntimeError,
    native::{
        binding::NativeResult,
        conversion::{IntoKagari, context::ConversionContext},
        payload::Opaque,
        storage::{
            NativePayload,
            access::EditPlan,
            scan::{NativeScanKind, ScanBudget, ScanStatus},
        },
        storage_type::StorageType,
        typed::NativeContext,
        value_handle::ScriptValue,
    },
    resource::HeapCharge,
    value::Value,
};

/// The standard Vec iterator is an ordinary native provider.
#[derive(Debug)]
pub struct VecCursor {
    source: Value,
    contract: StorageType,
    index: usize,
    revision: u64,
}

// SAFETY: The single source edge is always initialized; the cursor stores no loan.
unsafe impl NativePayload for VecCursor {
    type ScanState = ();
    const SCAN_KIND: NativeScanKind = NativeScanKind::Bounded { max_work: 1 };

    fn begin_scan(&self) {}

    fn scan<'a>(
        &'a self,
        _: &mut (),
        budget: &mut ScanBudget,
        visit: &mut dyn FnMut(&'a Value),
    ) -> ScanStatus {
        budget.charge(1);
        visit(&self.source);
        ScanStatus::Complete
    }

    fn charge(&self) -> HeapCharge {
        HeapCharge::new(1, 0)
    }
}

pub(crate) fn iter(
    cx: &mut NativeContext<'_>,
    (values,): (ScriptVec<ScriptValue>,),
) -> NativeResult<Opaque<VecCursor>> {
    let ty = cx.result_native_type::<VecCursor>()?;
    let revision = values.revision(cx)?;
    let argument = values.object.type_argument().clone();
    let owner = values.object.owner().clone();
    let contract = StorageType::prepare_scoped(argument.clone(), &owner)?;
    let mut conversion = ConversionContext::new(cx.runtime(), cx.owner())?;
    let source = values.into_kagari(&mut conversion, &argument)?;
    ty.create(
        cx,
        VecCursor {
            source,
            contract,
            index: 0,
            revision,
        },
        &[],
    )
}

pub(crate) fn next(
    cx: &mut NativeContext<'_>,
    (cursor,): (Opaque<VecCursor>,),
) -> NativeResult<Option<ScriptValue>> {
    // SAFETY: Retain source before ending the cursor loan. Extracted items remain
    // rooted across cursor advancement and Option result allocation.
    unsafe {
        let (source, index, revision) = cursor.read(cx, None, |cursor, access| {
            Ok((
                access.retain(&cursor.contract, &cursor.source)?,
                cursor.index,
                cursor.revision,
            ))
        })?;
        let source = source.decode::<Opaque<VecStorage>>(cx)?;
        let value = source.read(cx, None, |values, access| {
            access.check_revision(revision).map_err(|_| {
                RuntimeError::module_validation("invalid or structurally modified iterator")
            })?;
            values
                .values
                .get(index)
                .map(|value| access.retain(&values.contract, &value))
                .transpose()
        })?;
        if value.is_some() {
            cursor.edit(
                cx,
                None,
                EditPlan {
                    max_logical_units: 1,
                    structural: false,
                },
                |cursor, _| {
                    cursor.index += 1;
                    Ok(())
                },
            )?;
        }
        Ok(value)
    }
}
