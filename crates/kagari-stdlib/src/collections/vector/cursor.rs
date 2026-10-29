//! Ordinary nominal cursor with a traced source and runtime-owned revision check.
use crate::collections::vector::{ScriptVec, VecStorage};
use kagari_runtime::{
    error::RuntimeError,
    native::{
        binding::NativeResult,
        conversion::IntoKagari,
        payload::{Opaque, borrowed::BorrowedPayload},
        storage::{
            NativePayload,
            access::EditPlan,
            scan::{NativeScanKind, ScanBudget, ScanStatus},
            value::StoredValue,
        },
        storage_type::StorageType,
        typed::NativeContext,
        value_handle::ScriptValue,
    },
    resource::HeapCharge,
};

/// The standard Vec iterator is an ordinary native provider.
#[derive(Debug)]
pub struct VecCursor {
    source: StoredValue,
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
        visit: &mut dyn FnMut(&'a StoredValue),
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
    // Keep the existing owning root through cursor publication after the temporary
    // conversion ends. Cloning the handle shares that root rather than registering one.
    let source = {
        let mut conversion = cx.conversion_scope(cx.owner())?;
        values.clone().into_kagari(&mut conversion, &argument)?
    };
    // SAFETY: source retains the vector through cursor allocation and traced publication.
    let source_value = unsafe {
        cx.conversion_scope(&owner)?
            .store_transport(&argument, source.value_transport(cx.runtime())?)
    }?;
    ty.create(
        cx,
        VecCursor {
            source: source_value,
            contract,
            index: 0,
            revision,
        },
        &[],
    )
}

pub(crate) fn next(
    cx: &mut NativeContext<'_>,
    cursor: &BorrowedPayload<'_, VecCursor>,
) -> NativeResult<Option<ScriptValue>> {
    // SAFETY: The rooted cursor scans its unchanged source edge. Between the
    // parent loan and immediate source read there is no callback or collection,
    // so an alias cannot remove that protection. Retain the escaping item under
    // the source loan before advancement and fallible result conversion.
    unsafe {
        let (source, contract, index, revision) = cursor.read(cx, None, |cursor, _| {
            Ok((
                cursor.source,
                cursor.contract.clone(),
                cursor.index,
                cursor.revision,
            ))
        })?;
        let value = cx.read_payload::<VecStorage, _>(
            source.value_transport(),
            &contract,
            |values, access| {
                access.check_revision(revision).map_err(|_| {
                    RuntimeError::module_validation("invalid or structurally modified iterator")
                })?;
                values
                    .values
                    .get(index)
                    .map(|value| access.retain(&values.contract, &value))
                    .transpose()
            },
        )?;
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
