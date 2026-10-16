//! Stable checked identities point into dense stores of concrete payload kinds.
use crate::{
    closure::ClosureValueSnapshot,
    error_trace::ErrorTrace,
    execution_metadata::interfaces::InterfaceSnapshotId,
    gc::{
        HeapObjectId,
        fields::{StructView, StructViewMut},
        storage::{Exclusive, HeapObject, Shared},
        values::TupleData,
    },
    host::{HostPathViewHandle, HostRootHandle},
    module::{ModuleKey, layout_storage::StructStorageLayout},
    native::storage::NativeObject,
    range::RangeValue,
    value::{EnumValueSnapshot, EphemeralValue, Value},
};
use kagari_abi::representation::ValueType;
use std::{
    collections::{HashMap, TryReserveError},
    mem,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::Arc,
};

#[derive(Debug, Clone, Copy)]
pub(super) enum PayloadKind {
    String,
    Tuple,
    Range,
    HostRoot,
    HostPath,
    Ephemeral,
    Native,
    Enum,
    Struct,
    Interface,
    Closure,
    Cell,
}

/// Exact counts produced while retiring checked directory entries in this sweep.
#[derive(Default)]
pub(super) struct DetachedCounts([usize; PayloadKind::Cell as usize + 1]);

#[derive(Debug, Clone, Copy)]
pub(super) struct PayloadLocation {
    pub(super) kind: PayloadKind,
    index: u32,
}

#[derive(Debug)]
pub(super) struct ObjectSlot {
    pub(super) revision: u64,
    pub(super) generation: u32,
    pub(super) location: Option<PayloadLocation>,
}

#[derive(Debug)]
struct Entry<T> {
    directory: u32,
    payload: T,
}

#[derive(Debug, Default)]
pub(super) struct ObjectStorage {
    pub(super) slots: Vec<ObjectSlot>,
    pub(super) initialization_owners: HashMap<HeapObjectId, ModuleKey>,
    payloads: PayloadStores,
}

/// Typed detached batches keep disposal outside the storage borrow without
/// rebuilding a maximum-sized enum for every dead object.
#[derive(Debug, Default)]
pub(super) struct PayloadStores {
    strings: Vec<Entry<String>>,
    tuples: Vec<Entry<TupleData>>,
    ranges: Vec<Entry<RangeValue>>,
    host_roots: Vec<Entry<HostRootHandle>>,
    host_paths: Vec<Entry<Arc<HostPathViewHandle>>>,
    ephemerals: Vec<Entry<EphemeralValue>>,
    natives: Vec<Entry<NativeObject>>,
    enums: Vec<Entry<(EnumValueSnapshot, Option<Arc<ErrorTrace>>)>>,
    structs: Vec<Entry<StructRecord>>,
    struct_bytes: Vec<u8>,
    interfaces: Vec<Entry<(InterfaceSnapshotId, usize)>>,
    closures: Vec<Entry<ClosureValueSnapshot>>,
    cells: Vec<Entry<(ValueType, Value)>>,
}

/// Field bytes are packed independently of the stable object and record indices.
#[derive(Debug)]
struct StructRecord {
    description: Arc<StructStorageLayout>,
    offset: usize,
}

impl ObjectStorage {
    /// Reserve the chosen store before publication; no fallible operation follows
    /// its push. A store cannot exceed the already admitted u32 directory length.
    pub(super) fn insert(
        &mut self,
        directory: u32,
        object: HeapObject,
    ) -> Result<PayloadLocation, TryReserveError> {
        match object {
            HeapObject::String(value) => {
                self.payloads.strings.try_reserve(1)?;
                let index = u32::try_from(self.payloads.strings.len())
                    .expect("admitted heap payload index");
                self.payloads.strings.push(Entry {
                    directory,
                    payload: value,
                });
                Ok(PayloadLocation {
                    kind: PayloadKind::String,
                    index,
                })
            }
            HeapObject::Tuple(value) => {
                self.payloads.tuples.try_reserve(1)?;
                let index =
                    u32::try_from(self.payloads.tuples.len()).expect("admitted heap payload index");
                self.payloads.tuples.push(Entry {
                    directory,
                    payload: value,
                });
                Ok(PayloadLocation {
                    kind: PayloadKind::Tuple,
                    index,
                })
            }
            HeapObject::Range(value) => {
                self.payloads.ranges.try_reserve(1)?;
                let index =
                    u32::try_from(self.payloads.ranges.len()).expect("admitted heap payload index");
                self.payloads.ranges.push(Entry {
                    directory,
                    payload: value,
                });
                Ok(PayloadLocation {
                    kind: PayloadKind::Range,
                    index,
                })
            }
            HeapObject::HostRoot(value) => {
                self.payloads.host_roots.try_reserve(1)?;
                let index = u32::try_from(self.payloads.host_roots.len())
                    .expect("admitted heap payload index");
                self.payloads.host_roots.push(Entry {
                    directory,
                    payload: value,
                });
                Ok(PayloadLocation {
                    kind: PayloadKind::HostRoot,
                    index,
                })
            }
            HeapObject::HostPath(value) => {
                self.payloads.host_paths.try_reserve(1)?;
                let index = u32::try_from(self.payloads.host_paths.len())
                    .expect("admitted heap payload index");
                self.payloads.host_paths.push(Entry {
                    directory,
                    payload: value,
                });
                Ok(PayloadLocation {
                    kind: PayloadKind::HostPath,
                    index,
                })
            }
            HeapObject::Ephemeral(value) => {
                self.payloads.ephemerals.try_reserve(1)?;
                let index = u32::try_from(self.payloads.ephemerals.len())
                    .expect("admitted heap payload index");
                self.payloads.ephemerals.push(Entry {
                    directory,
                    payload: value,
                });
                Ok(PayloadLocation {
                    kind: PayloadKind::Ephemeral,
                    index,
                })
            }
            HeapObject::Native(value) => {
                self.payloads.natives.try_reserve(1)?;
                let index = u32::try_from(self.payloads.natives.len())
                    .expect("admitted heap payload index");
                self.payloads.natives.push(Entry {
                    directory,
                    payload: value,
                });
                Ok(PayloadLocation {
                    kind: PayloadKind::Native,
                    index,
                })
            }
            HeapObject::Enum(value, trace) => {
                self.payloads.enums.try_reserve(1)?;
                let index =
                    u32::try_from(self.payloads.enums.len()).expect("admitted heap payload index");
                self.payloads.enums.push(Entry {
                    directory,
                    payload: (value, trace),
                });
                Ok(PayloadLocation {
                    kind: PayloadKind::Enum,
                    index,
                })
            }
            HeapObject::Struct { storage } => {
                self.payloads.structs.try_reserve(1)?;
                self.payloads
                    .struct_bytes
                    .try_reserve(storage.description.size)?;
                let index = u32::try_from(self.payloads.structs.len())
                    .expect("admitted heap payload index");
                let offset = self.payloads.struct_bytes.len();
                self.payloads
                    .struct_bytes
                    .resize(offset + storage.description.size, 0);
                let mut view = StructViewMut {
                    description: &storage.description,
                    bytes: &mut self.payloads.struct_bytes[offset..],
                };
                for (slot, value) in storage.values.into_iter().enumerate() {
                    view.set(slot, value)
                        .expect("validated struct field encoding");
                }
                self.payloads.structs.push(Entry {
                    directory,
                    payload: StructRecord {
                        description: storage.description,
                        offset,
                    },
                });
                Ok(PayloadLocation {
                    kind: PayloadKind::Struct,
                    index,
                })
            }
            HeapObject::Interface {
                snapshot,
                method_count,
            } => {
                self.payloads.interfaces.try_reserve(1)?;
                let index = u32::try_from(self.payloads.interfaces.len())
                    .expect("admitted heap payload index");
                self.payloads.interfaces.push(Entry {
                    directory,
                    payload: (snapshot, method_count),
                });
                Ok(PayloadLocation {
                    kind: PayloadKind::Interface,
                    index,
                })
            }
            HeapObject::Closure { snapshot } => {
                self.payloads.closures.try_reserve(1)?;
                let index = u32::try_from(self.payloads.closures.len())
                    .expect("admitted heap payload index");
                self.payloads.closures.push(Entry {
                    directory,
                    payload: snapshot,
                });
                Ok(PayloadLocation {
                    kind: PayloadKind::Closure,
                    index,
                })
            }
            HeapObject::Cell { ty, value } => {
                self.payloads.cells.try_reserve(1)?;
                let index =
                    u32::try_from(self.payloads.cells.len()).expect("admitted heap payload index");
                self.payloads.cells.push(Entry {
                    directory,
                    payload: (ty, value),
                });
                Ok(PayloadLocation {
                    kind: PayloadKind::Cell,
                    index,
                })
            }
        }
    }

    pub(super) fn native(&self, location: PayloadLocation) -> Option<&NativeObject> {
        matches!(location.kind, PayloadKind::Native)
            .then(|| &self.payloads.natives[location.index as usize].payload)
    }

    pub(super) fn native_mut(&mut self, location: PayloadLocation) -> Option<&mut NativeObject> {
        matches!(location.kind, PayloadKind::Native)
            .then(|| &mut self.payloads.natives[location.index as usize].payload)
    }

    pub(super) fn structure(&self, location: PayloadLocation) -> Option<StructView<'_>> {
        if !matches!(location.kind, PayloadKind::Struct) {
            return None;
        }
        let record = &self.payloads.structs[location.index as usize].payload;
        Some(StructView {
            description: &record.description,
            bytes: &self.payloads.struct_bytes
                [record.offset..record.offset + record.description.size],
        })
    }

    pub(super) fn structure_mut(&mut self, location: PayloadLocation) -> Option<StructViewMut<'_>> {
        if !matches!(location.kind, PayloadKind::Struct) {
            return None;
        }
        let record = &self.payloads.structs[location.index as usize].payload;
        Some(StructViewMut {
            description: &record.description,
            bytes: &mut self.payloads.struct_bytes
                [record.offset..record.offset + record.description.size],
        })
    }

    #[cfg(test)]
    pub(super) fn corrupt_structure_for_test(&mut self, id: HeapObjectId) {
        let location = self.slots[id.index()].location.unwrap();
        let record = &mut self.payloads.structs[location.index as usize].payload;
        let description = Arc::make_mut(&mut record.description);
        description.fields = Box::new([]);
    }

    pub(super) fn get(&self, location: PayloadLocation) -> HeapObject<Shared<'_>> {
        let index = location.index as usize;
        match location.kind {
            PayloadKind::String => {
                let value = &self.payloads.strings[index].payload;
                HeapObject::String(value)
            }
            PayloadKind::Tuple => {
                let value = &self.payloads.tuples[index].payload;
                HeapObject::Tuple(value)
            }
            PayloadKind::Range => {
                let value = &self.payloads.ranges[index].payload;
                HeapObject::Range(value)
            }
            PayloadKind::HostRoot => {
                let value = &self.payloads.host_roots[index].payload;
                HeapObject::HostRoot(value)
            }
            PayloadKind::HostPath => {
                let value = &self.payloads.host_paths[index].payload;
                HeapObject::HostPath(value)
            }
            PayloadKind::Ephemeral => {
                let value = &self.payloads.ephemerals[index].payload;
                HeapObject::Ephemeral(value)
            }
            PayloadKind::Native => {
                let value = &self.payloads.natives[index].payload;
                HeapObject::Native(value)
            }
            PayloadKind::Enum => {
                let value = &self.payloads.enums[index].payload;
                HeapObject::Enum(&value.0, &value.1)
            }
            PayloadKind::Struct => HeapObject::Struct {
                storage: self.structure(location).expect("struct location"),
            },
            PayloadKind::Interface => {
                let value = &self.payloads.interfaces[index].payload;
                HeapObject::Interface {
                    snapshot: &value.0,
                    method_count: &value.1,
                }
            }
            PayloadKind::Closure => {
                let value = &self.payloads.closures[index].payload;
                HeapObject::Closure { snapshot: value }
            }
            PayloadKind::Cell => {
                let value = &self.payloads.cells[index].payload;
                HeapObject::Cell {
                    ty: &value.0,
                    value: &value.1,
                }
            }
        }
    }

    pub(super) fn get_mut(&mut self, location: PayloadLocation) -> HeapObject<Exclusive<'_>> {
        let index = location.index as usize;
        match location.kind {
            PayloadKind::String => {
                let value = &mut self.payloads.strings[index].payload;
                HeapObject::String(value)
            }
            PayloadKind::Tuple => {
                let value = &mut self.payloads.tuples[index].payload;
                HeapObject::Tuple(value)
            }
            PayloadKind::Range => {
                let value = &mut self.payloads.ranges[index].payload;
                HeapObject::Range(value)
            }
            PayloadKind::HostRoot => {
                let value = &mut self.payloads.host_roots[index].payload;
                HeapObject::HostRoot(value)
            }
            PayloadKind::HostPath => {
                let value = &mut self.payloads.host_paths[index].payload;
                HeapObject::HostPath(value)
            }
            PayloadKind::Ephemeral => {
                let value = &mut self.payloads.ephemerals[index].payload;
                HeapObject::Ephemeral(value)
            }
            PayloadKind::Native => {
                let value = &mut self.payloads.natives[index].payload;
                HeapObject::Native(value)
            }
            PayloadKind::Enum => {
                let value = &mut self.payloads.enums[index].payload;
                HeapObject::Enum(&mut value.0, &mut value.1)
            }
            PayloadKind::Struct => HeapObject::Struct {
                storage: self.structure_mut(location).expect("struct location"),
            },
            PayloadKind::Interface => {
                let value = &mut self.payloads.interfaces[index].payload;
                HeapObject::Interface {
                    snapshot: &mut value.0,
                    method_count: &mut value.1,
                }
            }
            PayloadKind::Closure => {
                let value = &mut self.payloads.closures[index].payload;
                HeapObject::Closure { snapshot: value }
            }
            PayloadKind::Cell => {
                let value = &mut self.payloads.cells[index].payload;
                HeapObject::Cell {
                    ty: &mut value.0,
                    value: &mut value.1,
                }
            }
        }
    }

    /// Detach without running payload destructors. The caller advances the stable
    /// generation; moved payloads only change their private locator.
    pub(super) fn detach_native(&mut self, id: HeapObjectId) -> Option<NativeObject> {
        let location = self.slots.get(id.index())?.location?;
        if !matches!(location.kind, PayloadKind::Native) {
            return None;
        }
        self.slots[id.index()].location = None;
        self.initialization_owners.remove(&id);
        let object = remove(
            &mut self.payloads.natives,
            &mut self.slots,
            location.index as usize,
        );
        shrink(&mut self.payloads.natives);
        self.shrink_initialization_owners();
        Some(object)
    }

    pub(super) fn mark_detached(&mut self, id: HeapObjectId, counts: &mut DetachedCounts) {
        let location = self.slots[id.index()]
            .location
            .take()
            .expect("marked live directory entry");
        counts.0[location.kind as usize] += 1;
        self.initialization_owners.remove(&id);
    }

    pub(super) fn detach_marked(&mut self, counts: DetachedCounts) -> PayloadStores {
        let structs = extract(
            &mut self.payloads.structs,
            &mut self.slots,
            counts.0[PayloadKind::Struct as usize],
        );
        let struct_bytes = if structs.is_empty() {
            Vec::new()
        } else if self.payloads.structs.is_empty() {
            mem::take(&mut self.payloads.struct_bytes)
        } else {
            let mut offset = 0;
            for entry in &mut self.payloads.structs {
                let record = &mut entry.payload;
                let size = record.description.size;
                self.payloads
                    .struct_bytes
                    .copy_within(record.offset..record.offset + size, offset);
                record.offset = offset;
                offset += size;
            }
            self.payloads.struct_bytes.truncate(offset);
            shrink(&mut self.payloads.struct_bytes);
            Vec::new()
        };
        self.shrink_initialization_owners();
        PayloadStores {
            strings: extract(
                &mut self.payloads.strings,
                &mut self.slots,
                counts.0[PayloadKind::String as usize],
            ),
            tuples: extract(
                &mut self.payloads.tuples,
                &mut self.slots,
                counts.0[PayloadKind::Tuple as usize],
            ),
            ranges: extract(
                &mut self.payloads.ranges,
                &mut self.slots,
                counts.0[PayloadKind::Range as usize],
            ),
            host_roots: extract(
                &mut self.payloads.host_roots,
                &mut self.slots,
                counts.0[PayloadKind::HostRoot as usize],
            ),
            host_paths: extract(
                &mut self.payloads.host_paths,
                &mut self.slots,
                counts.0[PayloadKind::HostPath as usize],
            ),
            ephemerals: extract(
                &mut self.payloads.ephemerals,
                &mut self.slots,
                counts.0[PayloadKind::Ephemeral as usize],
            ),
            natives: extract(
                &mut self.payloads.natives,
                &mut self.slots,
                counts.0[PayloadKind::Native as usize],
            ),
            enums: extract(
                &mut self.payloads.enums,
                &mut self.slots,
                counts.0[PayloadKind::Enum as usize],
            ),
            structs,
            struct_bytes,
            interfaces: extract(
                &mut self.payloads.interfaces,
                &mut self.slots,
                counts.0[PayloadKind::Interface as usize],
            ),
            closures: extract(
                &mut self.payloads.closures,
                &mut self.slots,
                counts.0[PayloadKind::Closure as usize],
            ),
            cells: extract(
                &mut self.payloads.cells,
                &mut self.slots,
                counts.0[PayloadKind::Cell as usize],
            ),
        }
    }

    fn shrink_initialization_owners(&mut self) {
        // Slot tombstones and their generations are permanent until Runtime drop.
        // Releasing payload capacity must never recreate an old external identity.
        if self.initialization_owners.capacity()
            > self.initialization_owners.len().saturating_mul(4).max(64)
        {
            self.initialization_owners.shrink_to_fit();
        }
    }
}

fn remove<T>(store: &mut Vec<Entry<T>>, slots: &mut [ObjectSlot], index: usize) -> T {
    let entry = store.swap_remove(index);
    if let Some(moved) = store.get(index) {
        slots[moved.directory as usize]
            .location
            .as_mut()
            .expect("live moved payload locator")
            .index = index as u32;
    }
    entry.payload
}

fn shrink<T>(store: &mut Vec<T>) {
    if store.capacity() > store.len().saturating_mul(4).max(64) {
        store.shrink_to(store.len().saturating_mul(2));
    }
}

impl PayloadStores {
    pub(super) fn dispose(self, mut on_panic: impl FnMut()) {
        dispose(self.strings, &mut on_panic);
        dispose(self.tuples, &mut on_panic);
        dispose(self.ranges, &mut on_panic);
        dispose(self.host_roots, &mut on_panic);
        dispose(self.host_paths, &mut on_panic);
        dispose(self.ephemerals, &mut on_panic);
        dispose(self.natives, &mut on_panic);
        dispose(self.enums, &mut on_panic);
        dispose(self.structs, &mut on_panic);
        drop(self.struct_bytes);
        dispose(self.interfaces, &mut on_panic);
        dispose(self.closures, &mut on_panic);
        dispose(self.cells, &mut on_panic);
    }
}

fn extract<T>(store: &mut Vec<Entry<T>>, slots: &mut [ObjectSlot], count: usize) -> Vec<Entry<T>> {
    if count == 0 {
        return Vec::new();
    }
    // An entirely dead store is already the detached batch. Moving its buffer
    // avoids allocating/copying a second batch even for a single native object.
    if count == store.len() {
        return mem::take(store);
    }
    let mut detached = Vec::with_capacity(count);
    detached.extend(store.extract_if(.., |entry| {
        slots[entry.directory as usize].location.is_none()
    }));
    for (index, entry) in store.iter().enumerate() {
        slots[entry.directory as usize]
            .location
            .as_mut()
            .expect("retained payload locator")
            .index = index as u32;
    }
    shrink(store);
    detached
}

#[inline]
fn dispose<T>(entries: Vec<Entry<T>>, on_panic: &mut impl FnMut()) {
    if !entries.is_empty() {
        dispose_nonempty(entries, on_panic);
    }
}

// Isolate destructor/panic machinery from the common empty-batch branch.
#[inline(never)]
fn dispose_nonempty<T>(entries: Vec<Entry<T>>, on_panic: &mut impl FnMut()) {
    for entry in entries {
        if catch_unwind(AssertUnwindSafe(|| drop(entry))).is_err() {
            on_panic();
        }
    }
}
