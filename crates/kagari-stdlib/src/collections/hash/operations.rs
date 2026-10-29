//! Short loans surround the callback-free lookup, publication and commit steps.
use crate::collections::hash::{HashPayload, HashTable, HashValue, input::Input, invalid};
use kagari_contract::standard::RuntimePrimitive;
use kagari_runtime::native::{
    binding::NativeResult,
    conversion::IntoKagari,
    payload::{Opaque, borrowed::BorrowedPayload},
    storage::{
        access::{EditPlan, StorageAccess},
        value::StoredValue,
    },
    storage_type::StorageType,
    typed::NativeContext,
    value_handle::ScriptValue,
    views::{NativeOperationGuard, NativeOperationKind},
};
use kagari_runtime::value::Value;
use kagari_runtime::value_semantics::{hash::script_hash_transport, script_equal_transport};
use kagari_runtime::{Runtime, error::RuntimeError};

pub(crate) struct Lookup<'runtime> {
    pub(crate) hash: i64,
    pub(crate) slot: Option<usize>,
    units: usize,
    guard: NativeOperationGuard<'runtime>,
}

pub(crate) fn len<P: HashPayload>(
    cx: &NativeContext<'_>,
    object: &BorrowedPayload<'_, P>,
) -> NativeResult<usize> {
    // SAFETY: Only a scalar leaves the loan.
    unsafe { object.read(cx, None, |payload, _| Ok(payload.table().slots.len())) }
}

pub(crate) fn revision<P: HashPayload>(
    cx: &NativeContext<'_>,
    object: &BorrowedPayload<'_, P>,
) -> NativeResult<u64> {
    // SAFETY: Only a scalar leaves the loan.
    unsafe { object.read(cx, None, |_, access| Ok(access.revision())) }
}

pub(crate) fn ensure_mutable<P: HashPayload>(
    cx: &mut NativeContext<'_>,
    object: &BorrowedPayload<'_, P>,
) -> NativeResult<()> {
    object.check_writable(cx)
}

pub(crate) fn encode<P: HashPayload, T: IntoKagari>(
    cx: &NativeContext<'_>,
    object: &Opaque<P>,
    index: usize,
    value: T,
) -> NativeResult<ScriptValue> {
    let mut conversion = cx.conversion_scope(cx.owner())?;
    let argument = conversion.parameter(object.type_argument(), index)?;
    conversion.encode_value(&argument, value)
}

fn scalar_position<V: HashValue>(
    runtime: &Runtime,
    table: &HashTable<V>,
    key: StoredValue,
) -> NativeResult<(i64, Option<usize>)> {
    let hash = unsafe { script_hash_transport(runtime.gc(), &key.value_transport()) }
        .ok_or_else(invalid)?;
    let mut position = table.buckets.get(&hash).copied();
    while let Some(slot) = position {
        runtime.resources().poll_execution()?;
        if unsafe {
            script_equal_transport(
                runtime.gc(),
                &table.slots[slot].key.value_transport(),
                &key.value_transport(),
            )
        }? {
            return Ok((hash, Some(slot)));
        }
        position = table.next(hash, slot);
    }
    Ok((hash, None))
}

/// Read the selected entry in the same loan as intrinsic scalar lookup. Other
/// keys retain callback protection and the same lookup/comparison semantics.
///
/// # Safety
/// Extracted edges must remain protected by the receiver or an independent root
/// until publication; the result cannot expose any payload reference.
pub(crate) unsafe fn lookup_read<P: HashPayload, R>(
    cx: &mut NativeContext<'_>,
    object: &BorrowedPayload<'_, P>,
    key: Input<'_, '_>,
    read: impl FnOnce(&HashTable<P::Value>, Option<usize>, &mut StorageAccess<'_>) -> NativeResult<R>,
) -> NativeResult<R> {
    let mut read = Some(read);
    if key.is_scalar(cx)? {
        // SAFETY: The payload owns checked, immutable construction evidence.
        // Publication still checks the exact type and value on every invocation.
        let result = unsafe {
            object.read(cx, None, |payload, access| {
                let table = payload.table();
                if !table.builtin_keys {
                    return Ok(None);
                }
                let key = key.publish(access, &table.key)?;
                let (_, slot) = scalar_position(cx.runtime(), table, key)?;
                read.take().expect("single projection")(table, slot, access).map(Some)
            })?
        };
        if let Some(result) = result {
            return Ok(result);
        }
    }
    let found = lookup(cx, object, key)?;
    // SAFETY: The callback-safe lookup guard still protects the selected entry.
    unsafe {
        object.read(cx, Some(&found.guard), |payload, access| {
            read.take().expect("single projection")(payload.table(), found.slot, access)
        })
    }
}

pub(crate) fn lookup<'view, P: HashPayload>(
    cx: &mut NativeContext<'_>,
    object: &'view BorrowedPayload<'_, P>,
    key: Input<'_, '_>,
) -> NativeResult<Lookup<'view>> {
    // SAFETY: Read-only protection has no recovery and outlives all short loans;
    // keys/candidates remain retained while Hash/Eq may allocate and collect.
    let guard =
        unsafe { object.try_enter_operation(cx, NativeOperationKind::ReadOnlyCallbacks, None)? }
            .ok_or_else(invalid)?;
    if object.stored_primitive(cx, 0)? == Some(RuntimePrimitive::ValueHash)
        && object.stored_primitive(cx, 1)? == Some(RuntimePrimitive::ValueEq)
    {
        return primitive_lookup(cx, object, key, guard);
    }
    let hash_call = object
        .stored_function::<(ScriptValue,), i64>(cx, 0)?
        .prepare_call(cx.runtime())?;
    let equal = object
        .stored_function::<(ScriptValue, ScriptValue), bool>(cx, 1)?
        .prepare_call(cx.runtime())?;
    // SAFETY: Custom callbacks may collect; retain the key under its exact contract.
    let (key, units) = unsafe {
        object.read(cx, Some(&guard), |payload, access| {
            Ok((
                key.retain(access, &payload.table().key)?,
                payload.table().slots.len(),
            ))
        })?
    };
    let hash = hash_call.call_with_options::<_, i64>(
        cx,
        (key.clone(),),
        cx.runtime().execution_options(),
    )?;
    let mut position = None;
    let mut first = true;
    loop {
        cx.poll()?;
        let candidate = unsafe {
            object.read(cx, Some(&guard), |payload, access| {
                let table = payload.table();
                let Some(slot) = (if first {
                    table.buckets.get(&hash).copied()
                } else {
                    position
                }) else {
                    return Ok(None);
                };
                let entry = &table.slots[slot];
                Ok(Some((
                    slot,
                    table.next(hash, slot),
                    access.retain(&table.key, &entry.key)?,
                )))
            })?
        };
        let Some((slot, next, candidate)) = candidate else {
            return Ok(Lookup {
                hash,
                slot: None,
                units,
                guard,
            });
        };
        if equal.call_with_options::<_, bool>(
            cx,
            (candidate, key.clone()),
            cx.runtime().execution_options(),
        )? {
            return Ok(Lookup {
                hash,
                slot: Some(slot),
                units,
                guard,
            });
        }
        position = next;
        first = false;
    }
}

fn primitive_lookup<'view, P: HashPayload>(
    cx: &NativeContext<'_>,
    object: &'view BorrowedPayload<'_, P>,
    key: Input<'_, '_>,
    guard: NativeOperationGuard<'view>,
) -> NativeResult<Lookup<'view>> {
    // SAFETY: Both inputs remain rooted. The selected operations have intrinsic
    // semantics. Pointer-free keys need no heap views or callback boundaries, so
    // publication, hashing and collision search share this checked read loan.
    let (key, units, scalar_result) = unsafe {
        object.read(cx, Some(&guard), |payload, access| {
            let table = payload.table();
            let key = key.publish(access, &table.key)?;
            let scalar = matches!(
                key.value_transport(),
                Value::Unit
                    | Value::Bool(_)
                    | Value::Char(_)
                    | Value::I32(_)
                    | Value::I64(_)
                    | Value::U64(_)
            );
            let found = if scalar {
                Some(scalar_position(cx.runtime(), table, key)?)
            } else {
                None
            };
            Ok((key, table.slots.len(), found))
        })?
    };
    if let Some((hash, slot)) = scalar_result {
        return Ok(Lookup {
            hash,
            slot,
            units,
            guard,
        });
    }
    let hash = unsafe { script_hash_transport(cx.runtime().gc(), &key.value_transport()) }
        .ok_or_else(invalid)?;
    let mut position = None;
    let mut first = true;
    loop {
        cx.poll()?;
        // SAFETY: The rooted receiver and guard keep this edge traced and prevent
        // mutation until comparison completes. Equality runs after the short loan;
        // no callback, conversion, or collection separates extraction and comparison.
        let candidate = unsafe {
            object.read(cx, Some(&guard), |payload, _| {
                let table = payload.table();
                let Some(slot) = (if first {
                    table.buckets.get(&hash).copied()
                } else {
                    position
                }) else {
                    return Ok(None);
                };
                let entry = &table.slots[slot];
                Ok(Some((slot, table.next(hash, slot), entry.key)))
            })?
        };
        match candidate {
            Some((slot, next, candidate)) => {
                if unsafe {
                    script_equal_transport(
                        cx.runtime().gc(),
                        &candidate.value_transport(),
                        &key.value_transport(),
                    )
                }? {
                    return Ok(Lookup {
                        hash,
                        slot: Some(slot),
                        units,
                        guard,
                    });
                }
                position = next;
                first = false;
            }
            None => {
                return Ok(Lookup {
                    hash,
                    slot: None,
                    units,
                    guard,
                });
            }
        }
    }
}

pub(crate) fn get<P: HashPayload>(
    cx: &NativeContext<'_>,
    object: &BorrowedPayload<'_, P>,
    found: &Lookup<'_>,
) -> NativeResult<Option<ScriptValue>> {
    // SAFETY: Retain before leaving the protected source loan.
    unsafe {
        with_value(cx, object, found, |contract, value, access| {
            access.retain(contract, value)
        })
    }
}

/// Extract a value while its source and lookup guard remain live.
///
/// # Safety
/// The caller must publish or retain extracted edges before releasing source
/// protection. No payload references may escape the callback.
pub(crate) unsafe fn with_value<P: HashPayload, R>(
    cx: &NativeContext<'_>,
    object: &BorrowedPayload<'_, P>,
    found: &Lookup<'_>,
    extract: impl FnOnce(&StorageType, &StoredValue, &mut StorageAccess<'_>) -> NativeResult<R>,
) -> NativeResult<Option<R>> {
    // SAFETY: The caller owns the lifetime of any extracted edges; the callback
    // executes under the same lookup protection used to select the slot.
    unsafe {
        object.read(cx, Some(&found.guard), |payload, access| {
            let table = payload.table();
            found
                .slot
                .map(|slot| {
                    let entry = &table.slots[slot];
                    extract(
                        table.value.as_ref().ok_or_else(invalid)?,
                        entry.value.as_value().ok_or_else(invalid)?,
                        access,
                    )
                })
                .transpose()
        })
    }
}

pub(crate) fn insert<P: HashPayload>(
    cx: &mut NativeContext<'_>,
    object: &BorrowedPayload<'_, P>,
    key: Input<'_, '_>,
    value: Option<Input<'_, '_>>,
) -> NativeResult<bool> {
    if key.is_scalar(cx)? {
        let runtime = cx.runtime();
        // SAFETY: Actual immutable builtin evidence excludes collecting callbacks.
        // Lookup, exact occupancy preflight and commit share one checked loan;
        // generic keys still use this same table mutation after their callbacks.
        let result = unsafe {
            object.edit_prepared(
                cx,
                |payload, access| {
                    let table = payload.table();
                    if !table.builtin_keys {
                        return Ok(None);
                    }
                    let key = key.publish(access, &table.key)?;
                    let (hash, slot) = scalar_position(runtime, table, key)?;
                    let units = table
                        .slots
                        .len()
                        .checked_add(usize::from(slot.is_none()))
                        .ok_or_else(|| RuntimeError::resource_limit("hash occupancy"))?;
                    Ok(Some((
                        EditPlan {
                            max_logical_units: units,
                            structural: true,
                        },
                        (key, hash, slot),
                    )))
                },
                |payload, access, (key, hash, slot)| {
                    let table = payload.table_mut();
                    let value = value
                        .map(|value| {
                            value.publish(access, table.value.as_ref().ok_or_else(invalid)?)
                        })
                        .transpose()?;
                    commit_insert(table, access, hash, slot, key, value)
                },
            )?
        };
        if let Some(result) = result {
            return Ok(result);
        }
    }
    ensure_mutable(cx, object)?;

    let found = lookup(cx, object, key)?;
    let Lookup {
        hash,
        slot,
        units,
        guard,
    } = found;
    drop(guard);
    // SAFETY: No callback, converter or GC boundary separates lookup protection
    // from this edit. All imports publish before commit; backing reserves precede
    // writes. Replacing a value preserves the original key and structural revision.
    unsafe {
        object.edit(
            cx,
            None,
            EditPlan {
                max_logical_units: units + usize::from(slot.is_none()),
                structural: true,
            },
            |payload, access| {
                let table = payload.table_mut();
                let key = key.publish(access, &table.key)?;
                let value = value
                    .map(|value| value.publish(access, table.value.as_ref().ok_or_else(invalid)?))
                    .transpose()?;
                commit_insert(table, access, hash, slot, key, value)
            },
        )
    }
}

fn commit_insert<V: HashValue>(
    table: &mut HashTable<V>,
    access: &mut StorageAccess<'_>,
    hash: i64,
    slot: Option<usize>,
    key: StoredValue,
    value: Option<StoredValue>,
) -> NativeResult<bool> {
    if let Some(slot) = slot {
        if let Some(value) = value {
            table.slots[slot].value = HashValue::from_value(Some(value))?;
        }
        return Ok(false);
    }
    let value = HashValue::from_value(value)?;
    table.reserve(hash)?;
    access.commit_structure();
    table.insert(hash, key, value);
    Ok(true)
}

pub(crate) fn remove<P: HashPayload>(
    cx: &mut NativeContext<'_>,
    object: &BorrowedPayload<'_, P>,
    found: Lookup<'_>,
) -> NativeResult<bool> {
    let Lookup {
        slot, units, guard, ..
    } = found;
    drop(guard);
    // SAFETY: No callback/collection gap. Removal uses dense relocation and repairs
    // scalar bucket positions without allocation; moved references are republished.
    unsafe {
        object.edit(
            cx,
            None,
            EditPlan {
                max_logical_units: units,
                structural: true,
            },
            |payload, access| {
                let Some(slot) = slot else {
                    return Ok(false);
                };
                access.commit_structure();
                payload.table_mut().remove(slot, access);
                Ok(true)
            },
        )
    }
}

pub(crate) fn clear<P: HashPayload>(
    cx: &mut NativeContext<'_>,
    object: &BorrowedPayload<'_, P>,
) -> NativeResult<()> {
    let units = len(cx, object)?;
    // SAFETY: Only edge deletion; backing capacity remains reported.
    unsafe {
        object.edit(
            cx,
            None,
            EditPlan {
                max_logical_units: units,
                structural: true,
            },
            |payload, access| {
                if !payload.table().slots.is_empty() {
                    access.commit_structure();
                    payload.table_mut().clear();
                }
                Ok(())
            },
        )
    }
}

pub(crate) fn retained_entry<P: HashPayload>(
    cx: &NativeContext<'_>,
    object: &BorrowedPayload<'_, P>,
    revision: u64,
    position: usize,
) -> NativeResult<Option<(ScriptValue, Option<ScriptValue>)>> {
    // SAFETY: Check the source before reading even at exhaustion; retain both
    // fields before any conversion or callback can replace/remove this entry.
    unsafe {
        object.read(cx, None, |payload, access| {
            retain_entry(payload, access, revision, position)
        })
    }
}

pub(crate) fn retain_entry<P: HashPayload>(
    payload: &P,
    access: &mut StorageAccess<'_>,
    revision: u64,
    position: usize,
) -> NativeResult<Option<(ScriptValue, Option<ScriptValue>)>> {
    access.check_revision(revision)?;
    let table = payload.table();
    table
        .slots
        .get(position)
        .map(|entry| {
            Ok((
                access.retain(&table.key, &entry.key)?,
                table
                    .value
                    .as_ref()
                    .map(|contract| {
                        access.retain(contract, entry.value.as_value().ok_or_else(invalid)?)
                    })
                    .transpose()?,
            ))
        })
        .transpose()
}

pub(crate) fn snapshot<P: HashPayload>(
    cx: &mut NativeContext<'_>,
    object: &BorrowedPayload<'_, P>,
) -> NativeResult<Vec<(ScriptValue, Option<ScriptValue>)>> {
    let count = len(cx, object)?;
    cx.conversion().check_elements(count)?;
    // SAFETY: Every element is retained under its source loan, before any child
    // converter can reenter, mutate the collection or trigger collection.
    unsafe {
        object.read(cx, None, |payload, access| {
            let table = payload.table();
            table
                .slots
                .iter()
                .map(|entry| {
                    cx.poll()?;
                    Ok((
                        access.retain(&table.key, &entry.key)?,
                        table
                            .value
                            .as_ref()
                            .map(|contract| {
                                access.retain(contract, entry.value.as_value().ok_or_else(invalid)?)
                            })
                            .transpose()?,
                    ))
                })
                .collect()
        })
    }
}
