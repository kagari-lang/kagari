//! Ordinary opaque cursors trace their source and retain one current item.
use crate::collections::{
    hash::{HashPayload, invalid, operations},
    map::MapStorage,
    set::SetStorage,
};
use kagari_runtime::{
    native::{
        binding::NativeResult,
        conversion::context::ConversionContext,
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
use std::marker::PhantomData;

#[derive(Debug)]
pub(crate) struct HashCursor<P> {
    source: Value,
    contract: StorageType,
    position: usize,
    revision: u64,
    marker: PhantomData<fn() -> P>,
}

// SAFETY: The one source edge is always reported; cursor continuation is scalar.
unsafe impl<P: NativePayload> NativePayload for HashCursor<P> {
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

pub(crate) fn iter<P: HashPayload>(
    cx: &mut NativeContext<'_>,
    (source,): (Opaque<P>,),
) -> NativeResult<Opaque<HashCursor<P>>> {
    let ty = cx.result_native_type::<HashCursor<P>>()?;
    let revision = operations::revision(cx, &source.borrow(cx)?)?;
    let source_type = source.type_argument().clone();
    let owner = source.owner().clone();
    let contract = StorageType::prepare_scoped(source_type.clone(), &owner)?;
    let mut conversion = ConversionContext::new(cx.runtime(), cx.owner())?;
    let source = conversion.encode_value(&source_type, source)?;
    ty.create(
        cx,
        HashCursor {
            source,
            contract,
            position: 0,
            revision,
            marker: PhantomData,
        },
        &[],
    )
}

fn next<P: HashPayload>(
    cx: &mut NativeContext<'_>,
    cursor: Opaque<HashCursor<P>>,
) -> NativeResult<Option<(ScriptValue, Option<ScriptValue>)>> {
    // SAFETY: Retain source/item under their source loans. No reference or
    // borrowed Rust iterator crosses a callback/conversion/allocation boundary.
    unsafe {
        let (source, mut position, revision) = cursor.read(cx, None, |cursor, access| {
            Ok((
                access.retain(&cursor.contract, &cursor.source)?,
                cursor.position,
                cursor.revision,
            ))
        })?;
        let source = source.decode::<Opaque<P>>(cx)?;
        cx.poll()?;
        let result = operations::retained_entry(cx, &source.borrow(cx)?, revision, position)?;
        if result.is_some() {
            position += 1;
        }
        cursor.edit(
            cx,
            None,
            EditPlan {
                max_logical_units: 1,
                structural: false,
            },
            |cursor, _| {
                cursor.position = position;
                Ok(())
            },
        )?;
        Ok(result)
    }
}

pub(crate) fn map_next(
    cx: &mut NativeContext<'_>,
    (cursor,): (Opaque<HashCursor<MapStorage>>,),
) -> NativeResult<Option<(ScriptValue, ScriptValue)>> {
    next(cx, cursor)?
        .map(|(key, value)| Ok((key, value.ok_or_else(invalid)?)))
        .transpose()
}

pub(crate) fn set_next(
    cx: &mut NativeContext<'_>,
    (cursor,): (Opaque<HashCursor<SetStorage>>,),
) -> NativeResult<Option<ScriptValue>> {
    Ok(next(cx, cursor)?.map(|(key, _)| key))
}
