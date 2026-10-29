//! An application-owned integer-key map. Runtime imports here are all public.
use kagari_runtime::{
    error::RuntimeError,
    native::{
        binding::NativeResult,
        conversion::context::ConversionContext,
        function_handle::Function,
        payload::{NativeType, Opaque},
        storage::{
            NativePayload,
            access::{EditPlan, StorageAccess},
            scan::{NativeScanKind, ScanBudget, ScanStatus},
            value::StoredValue,
        },
        storage_type::StorageType,
        typed::NativeContext,
        value_handle::ScriptValue,
        views::{NativeOperationGuard, NativeOperationKind},
    },
    resource::HeapCharge,
};
use std::{
    collections::BTreeMap,
    ops::Bound,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Debug, Default)]
pub struct Observations {
    pub drops: AtomicUsize,
    pub pending: AtomicUsize,
    pub scan_position: AtomicUsize,
    pub scan_limit: AtomicUsize,
    pub completed_scans: AtomicUsize,
    pub recoveries: AtomicUsize,
}

#[derive(Debug)]
pub struct OrderedMap {
    index: BTreeMap<i32, usize>,
    slots: Vec<Option<StoredValue>>,
    contract: StorageType,
    // (first scratch slot, temporarily vacant indexed source slot)
    scratch: Option<(usize, usize)>,
    observations: Arc<Observations>,
}

impl OrderedMap {
    pub fn empty(
        cx: &NativeContext<'_>,
        ty: &NativeType<Self>,
        observations: Arc<Observations>,
    ) -> NativeResult<Self> {
        let conversion = ConversionContext::new(cx.runtime(), ty.owner())?;
        let item = conversion.parameter(ty.type_argument(), 0)?;
        Ok(Self {
            index: BTreeMap::new(),
            slots: Vec::new(),
            contract: StorageType::prepare_scoped(item, ty.owner())?,
            scratch: None,
            observations,
        })
    }
}

// SAFETY: The index contains no script edges. Every edge, including temporary work
// state, lives in slots. The owned continuation is two indices, never a borrowed
// BTree iterator. Every inspected slot (including a hole) consumes one work unit.
unsafe impl NativePayload for OrderedMap {
    type ScanState = (usize, usize);
    const SCAN_KIND: NativeScanKind = NativeScanKind::Resumable;

    fn begin_scan(&self) -> Self::ScanState {
        self.observations.scan_position.store(0, Ordering::SeqCst);
        self.observations
            .scan_limit
            .store(self.slots.len(), Ordering::SeqCst);
        (0, self.slots.len())
    }

    fn scan<'a>(
        &'a self,
        state: &mut Self::ScanState,
        budget: &mut ScanBudget,
        visit: &mut dyn FnMut(&'a StoredValue),
    ) -> ScanStatus {
        while state.0 < state.1 {
            if !budget.take(1) {
                self.observations.pending.fetch_add(1, Ordering::Relaxed);
                return ScanStatus::Pending;
            }
            if let Some(Some(value)) = self.slots.get(state.0) {
                visit(value);
            }
            state.0 += 1;
            self.observations
                .scan_position
                .store(state.0, Ordering::SeqCst);
        }
        self.observations
            .completed_scans
            .fetch_add(1, Ordering::SeqCst);
        ScanStatus::Complete
    }

    fn charge(&self) -> HeapCharge {
        // Vec capacity is exact. std does not expose BTree node allocation sizes:
        // the per-entry term is a pressure estimate, NOT physical allocator bytes.
        let index_pressure =
            self.index.len() * (size_of::<(i32, usize)>() + 4 * size_of::<usize>());
        HeapCharge::new(
            self.slots.len() + self.index.len(),
            self.slots.capacity() * size_of::<Option<StoredValue>>() + index_pressure,
        )
    }
}

impl Drop for OrderedMap {
    fn drop(&mut self) {
        self.observations.drops.fetch_add(1, Ordering::SeqCst);
    }
}

pub fn len(cx: &mut NativeContext<'_>, (map,): (Opaque<OrderedMap>,)) -> NativeResult<u64> {
    // SAFETY: Only a scalar leaves the loan.
    unsafe { map.read(cx, None, |map, _| Ok(map.index.len() as u64)) }
}

pub fn is_empty(cx: &mut NativeContext<'_>, args: (Opaque<OrderedMap>,)) -> NativeResult<bool> {
    Ok(len(cx, args)? == 0)
}

pub fn contains(
    cx: &mut NativeContext<'_>,
    (map, key): (Opaque<OrderedMap>, i32),
) -> NativeResult<bool> {
    // SAFETY: No interior mutation or escaping edge.
    unsafe { map.read(cx, None, |map, _| Ok(map.index.contains_key(&key))) }
}

pub fn get(
    cx: &mut NativeContext<'_>,
    (map, key): (Opaque<OrderedMap>, i32),
) -> NativeResult<Option<ScriptValue>> {
    // SAFETY: Retention occurs while the source edge is still installed.
    unsafe {
        map.read(cx, None, |map, access| {
            map.index
                .get(&key)
                .map(|slot| access.retain(&map.contract, map.slots[*slot].as_ref().unwrap()))
                .transpose()
        })
    }
}

pub fn insert(
    cx: &mut NativeContext<'_>,
    arguments: (Opaque<OrderedMap>, i32, ScriptValue),
) -> NativeResult<()> {
    insert_with_completion(cx, arguments, || Ok(()))
}

/// The completion hook injects failure after the same insertion implementation
/// has committed, without adding a second mutation path to the fixture.
pub fn insert_with_completion(
    cx: &mut NativeContext<'_>,
    (map, key, value): (Opaque<OrderedMap>, i32, ScriptValue),
    completion: impl FnOnce() -> NativeResult<()>,
) -> NativeResult<()> {
    // SAFETY: Scalar preflight, checked publication, reserve before commit; integer
    // BTree ordering cannot reenter script or unwind through a comparator.
    unsafe {
        let borrowed = map.borrow(cx)?;
        borrowed
            .edit_prepared(
                cx,
                |map, _| {
                    Ok(Some((
                        EditPlan {
                            max_logical_units: map.charge().logical_units
                                + if map.index.contains_key(&key) { 0 } else { 2 },
                            structural: true,
                        },
                        (),
                    )))
                },
                |map, access, ()| {
                    let value = access.publish(&map.contract, &value)?;
                    if let Some(slot) = map.index.get(&key) {
                        map.slots[*slot] = Some(value);
                    } else {
                        map.slots.reserve(1);
                        access.commit_structure();
                        map.index.insert(key, map.slots.len());
                        map.slots.push(Some(value));
                    }
                    completion()
                },
            )
            .map(|result| result.expect("provider always prepares insertion"))
    }
}

pub fn remove(
    cx: &mut NativeContext<'_>,
    (map, key): (Opaque<OrderedMap>, i32),
) -> NativeResult<Option<ScriptValue>> {
    // SAFETY: Retain before removing the source edge; holes remain scan-visible.
    unsafe {
        let units = map.read(cx, None, |map, _| Ok(map.charge().logical_units))?;
        map.edit(
            cx,
            None,
            EditPlan {
                max_logical_units: units,
                structural: true,
            },
            |map, access| {
                let Some(&slot) = map.index.get(&key) else {
                    return Ok(None);
                };
                let value = access.retain(&map.contract, map.slots[slot].as_ref().unwrap())?;
                access.commit_structure();
                map.index.remove(&key);
                map.slots[slot] = None;
                Ok(Some(value))
            },
        )
    }
}

pub fn clear(cx: &mut NativeContext<'_>, (map,): (Opaque<OrderedMap>,)) -> NativeResult<()> {
    // SAFETY: Removing edges requires no publication; capacity remains charged.
    unsafe {
        let units = map.read(cx, None, |map, _| Ok(map.charge().logical_units))?;
        map.edit(
            cx,
            None,
            EditPlan {
                max_logical_units: units,
                structural: true,
            },
            |map, access| {
                if !map.index.is_empty() {
                    access.commit_structure();
                    map.index.clear();
                    map.slots.clear();
                }
                Ok(())
            },
        )
    }
}

pub fn revision(cx: &NativeContext<'_>, map: &Opaque<OrderedMap>) -> NativeResult<u64> {
    // SAFETY: Scalar only.
    unsafe { map.read(cx, None, |_, access| Ok(access.revision())) }
}

pub fn shape(
    cx: &NativeContext<'_>,
    map: &Opaque<OrderedMap>,
) -> NativeResult<(usize, HeapCharge)> {
    // SAFETY: Scalar diagnostics only.
    unsafe { map.read(cx, None, |map, _| Ok((map.slots.len(), map.charge()))) }
}

fn recover(map: &mut OrderedMap, access: &mut StorageAccess<'_>) {
    if let Some((start, source)) = map.scratch.take() {
        let value = map.slots[start].expect("protected scratch edge");
        // SAFETY: This edge never left the same protected, traced Vec.
        unsafe { access.republish_existing(&value) };
        map.slots[source] = Some(value);
        map.slots.truncate(start);
        map.observations.recoveries.fetch_add(1, Ordering::SeqCst);
    }
}

/// Move a live edge into same-payload scratch, release the loan and call out.
/// Error/unwind cleanup must not allocate or require ordinary execution admission.
pub fn with_scratch<R>(
    cx: &mut NativeContext<'_>,
    map: &Opaque<OrderedMap>,
    callback: impl FnOnce(&mut NativeContext<'_>) -> NativeResult<R>,
) -> NativeResult<R> {
    let _guard = scratch_operation(cx, map)?;
    callback(cx)
}

pub fn scratch_operation<'runtime>(
    cx: &mut NativeContext<'runtime>,
    map: &Opaque<OrderedMap>,
) -> NativeResult<NativeOperationGuard<'runtime>> {
    // SAFETY: Recovery only restores a Copy value and truncates a Vec. The guard
    // outlives all short loans; the edge stays traced in the same payload.
    unsafe {
        let guard = map
            .try_enter_operation(cx, NativeOperationKind::ExclusiveSlots, Some(recover))?
            .ok_or_else(|| RuntimeError::module_validation("map operation already active"))?;
        let units = map.read(cx, Some(&guard), |map, _| Ok(map.charge().logical_units))?;
        map.edit(
            cx,
            Some(&guard),
            EditPlan {
                max_logical_units: units + 1,
                structural: true,
            },
            |map, access| {
                let (_, &slot) = map
                    .index
                    .first_key_value()
                    .ok_or_else(|| RuntimeError::module_validation("scratch needs an entry"))?;
                let value = map.slots[slot].expect("indexed live slot");
                map.slots.reserve(1);
                access.republish_existing(&value);
                access.commit_structure();
                map.scratch = Some((map.slots.len(), slot));
                map.slots.push(Some(value));
                map.slots[slot] = None;
                Ok(())
            },
        )?;
        Ok(guard)
    }
}

pub fn callback(
    cx: &mut NativeContext<'_>,
    (map, callback): (Opaque<OrderedMap>, Function<(), ()>),
) -> NativeResult<()> {
    with_scratch(cx, &map, |cx| callback.call(cx, ()))
}

#[derive(Debug)]
pub struct Cursor {
    source: StoredValue,
    contract: StorageType,
    after: Option<i32>,
    revision: u64,
}

// SAFETY: Exactly one traced source edge; continuation contains no borrowed state.
unsafe impl NativePayload for Cursor {
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

pub fn iter(
    cx: &mut NativeContext<'_>,
    (map,): (Opaque<OrderedMap>,),
) -> NativeResult<Opaque<Cursor>> {
    let ty = cx.result_native_type::<Cursor>()?;
    let revision = revision(cx, &map)?;
    let source_type = map.type_argument().clone();
    let owner = map.owner().clone();
    let contract = StorageType::prepare_scoped(source_type.clone(), &owner)?;
    let mut conversion = ConversionContext::new(cx.runtime(), cx.owner())?;
    let source = conversion.encode_value(&source_type, map)?;
    // SAFETY: source protects the map through cursor traced-edge publication.
    let source_value =
        unsafe { conversion.store_transport(&source_type, source.value_transport(cx.runtime())?) }?;
    // The conversion's roots protect source through allocation and its safepoint.
    ty.create(
        cx,
        Cursor {
            source: source_value,
            contract,
            after: None,
            revision,
        },
        &[],
    )
}

pub fn next(
    cx: &mut NativeContext<'_>,
    (cursor,): (Opaque<Cursor>,),
) -> NativeResult<Option<(i32, ScriptValue)>> {
    // SAFETY: The owning cursor stays rooted and scans its unchanged source.
    // No callback or collection separates copying that edge from its immediate
    // read; aliases cannot remove it in this gap. Retain the returned entry under
    // the source loan before cursor advancement or fallible result conversion.
    unsafe {
        let borrowed = cursor.borrow(cx)?;
        let (source, contract, after, revision) = borrowed.read(cx, None, |cursor, _| {
            Ok((
                cursor.source,
                cursor.contract.clone(),
                cursor.after,
                cursor.revision,
            ))
        })?;
        let result = cx.read_payload::<OrderedMap, _>(
            source.value_transport(),
            &contract,
            |map, access| {
                access.check_revision(revision)?;
                let entry = match after {
                    None => map.index.first_key_value(),
                    Some(key) => map
                        .index
                        .range((Bound::Excluded(key), Bound::Unbounded))
                        .next(),
                };
                entry
                    .map(|(&key, &slot)| {
                        Ok((
                            key,
                            access.retain(&map.contract, map.slots[slot].as_ref().unwrap())?,
                        ))
                    })
                    .transpose()
            },
        )?;
        if let Some((key, _)) = &result {
            cursor.edit(
                cx,
                None,
                EditPlan {
                    max_logical_units: 1,
                    structural: false,
                },
                |cursor, _| {
                    cursor.after = Some(*key);
                    Ok(())
                },
            )?;
        }
        Ok(result)
    }
}
