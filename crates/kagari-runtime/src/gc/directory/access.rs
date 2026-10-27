//! Typed views over sealed placement kinds; references stay within the owner borrow.
use crate::{
    closure::ClosureValueSnapshot,
    error_trace::ErrorTrace,
    execution_metadata::interfaces::InterfaceStore,
    gc::{
        directory::{ObjectStorage, PayloadKind, PayloadLocation},
        fields::{StructView, StructViewMut},
        storage::{Exclusive, HeapObject, Shared},
        values::TupleData,
    },
    host::{HostPathViewHandle, HostRootHandle},
    native::storage::NativeObject,
    range::RangeValue,
    value::{EnumValueSnapshot, EphemeralValue, Value},
};
use kagari_abi::representation::ValueType;
use std::{cell::RefCell, slice, sync::Arc};

type EnumPayload = (EnumValueSnapshot, Option<Arc<ErrorTrace>>);
type CellPayload = (ValueType, Value);

impl ObjectStorage {
    pub(in crate::gc) fn native(&self, location: PayloadLocation) -> Option<&NativeObject> {
        if location.kind != PayloadKind::Native {
            return None;
        }
        let (pointer, _) = self.pages.get(location)?;
        // SAFETY: checked occupied kind has exactly this initialized representation.
        Some(unsafe { pointer.cast::<NativeObject>().as_ref() })
    }

    pub(in crate::gc) fn native_mut(
        &mut self,
        location: PayloadLocation,
    ) -> Option<&mut NativeObject> {
        if location.kind != PayloadKind::Native {
            return None;
        }
        let (pointer, _) = self.pages.get(location)?;
        // SAFETY: exclusive directory borrow; payload loans separately exclude T access.
        Some(unsafe { pointer.cast::<NativeObject>().as_mut() })
    }

    pub(in crate::gc) fn structure(&self, location: PayloadLocation) -> Option<StructView<'_>> {
        let (pointer, descriptor) = self.pages.get(location)?;
        let description = descriptor.structure.as_deref()?;
        // SAFETY: descriptor and fully initialized bytes live for the storage borrow.
        let bytes = unsafe { slice::from_raw_parts(pointer.as_ptr(), description.size) };
        Some(StructView { description, bytes })
    }

    pub(in crate::gc) fn structure_mut(
        &mut self,
        location: PayloadLocation,
    ) -> Option<StructViewMut<'_>> {
        let (pointer, descriptor) = self.pages.get(location)?;
        let description = descriptor.structure.as_deref()?;
        // SAFETY: exclusive directory borrow excludes any other view of these bytes.
        let bytes = unsafe { slice::from_raw_parts_mut(pointer.as_ptr(), description.size) };
        Some(StructViewMut { description, bytes })
    }

    pub(in crate::gc) fn get(
        &self,
        location: PayloadLocation,
        interfaces: &RefCell<InterfaceStore>,
    ) -> Option<HeapObject<Shared<'_>>> {
        if location.kind == PayloadKind::Interface {
            return Some(HeapObject::Interface {
                snapshot: interfaces
                    .try_borrow()
                    .ok()?
                    .directory_binding(location.page)?,
            });
        }
        if location.kind == PayloadKind::Struct {
            return self
                .structure(location)
                .map(|storage| HeapObject::Struct { storage });
        }
        let (pointer, _) = self.pages.get(location)?;
        // SAFETY: each private placement arm pairs its kind with precisely the type
        // below. Occupancy is checked and every returned reference is owner-borrowed.
        Some(unsafe {
            match location.kind {
                PayloadKind::String => HeapObject::String(pointer.cast::<String>().as_ref()),
                PayloadKind::Tuple => HeapObject::Tuple(pointer.cast::<TupleData>().as_ref()),
                PayloadKind::Range => HeapObject::Range(pointer.cast::<RangeValue>().as_ref()),
                PayloadKind::HostRoot => {
                    HeapObject::HostRoot(pointer.cast::<HostRootHandle>().as_ref())
                }
                PayloadKind::HostPath => {
                    HeapObject::HostPath(pointer.cast::<Arc<HostPathViewHandle>>().as_ref())
                }
                PayloadKind::Ephemeral => {
                    HeapObject::Ephemeral(pointer.cast::<EphemeralValue>().as_ref())
                }
                PayloadKind::Native => HeapObject::Native(pointer.cast::<NativeObject>().as_ref()),
                PayloadKind::Enum => {
                    let value = pointer.cast::<EnumPayload>().as_ref();
                    HeapObject::Enum(&value.0, &value.1)
                }
                PayloadKind::Struct => unreachable!("structure handled above"),
                PayloadKind::Interface | PayloadKind::Vacant => {
                    unreachable!("checked physical descriptor")
                }
                PayloadKind::Closure => HeapObject::Closure {
                    snapshot: pointer.cast::<ClosureValueSnapshot>().as_ref(),
                },
                PayloadKind::Cell => {
                    let value = pointer.cast::<CellPayload>().as_ref();
                    HeapObject::Cell {
                        ty: &value.0,
                        value: &value.1,
                    }
                }
            }
        })
    }

    pub(in crate::gc) fn get_mut(
        &mut self,
        location: PayloadLocation,
        interfaces: &RefCell<InterfaceStore>,
    ) -> Option<HeapObject<Exclusive<'_>>> {
        if location.kind == PayloadKind::Interface {
            return Some(HeapObject::Interface {
                snapshot: interfaces
                    .try_borrow()
                    .ok()?
                    .directory_binding(location.page)?,
            });
        }
        if location.kind == PayloadKind::Struct {
            return self
                .structure_mut(location)
                .map(|storage| HeapObject::Struct { storage });
        }
        let (pointer, _) = self.pages.get(location)?;
        // SAFETY: same sealed mapping as get, with exclusive directory ownership.
        Some(unsafe {
            match location.kind {
                PayloadKind::String => HeapObject::String(pointer.cast::<String>().as_mut()),
                PayloadKind::Tuple => HeapObject::Tuple(pointer.cast::<TupleData>().as_mut()),
                PayloadKind::Range => HeapObject::Range(pointer.cast::<RangeValue>().as_mut()),
                PayloadKind::HostRoot => {
                    HeapObject::HostRoot(pointer.cast::<HostRootHandle>().as_mut())
                }
                PayloadKind::HostPath => {
                    HeapObject::HostPath(pointer.cast::<Arc<HostPathViewHandle>>().as_mut())
                }
                PayloadKind::Ephemeral => {
                    HeapObject::Ephemeral(pointer.cast::<EphemeralValue>().as_mut())
                }
                PayloadKind::Native => HeapObject::Native(pointer.cast::<NativeObject>().as_mut()),
                PayloadKind::Enum => {
                    let value = pointer.cast::<EnumPayload>().as_mut();
                    HeapObject::Enum(&mut value.0, &mut value.1)
                }
                PayloadKind::Struct => unreachable!("structure handled above"),
                PayloadKind::Interface | PayloadKind::Vacant => {
                    unreachable!("checked physical descriptor")
                }
                PayloadKind::Closure => HeapObject::Closure {
                    snapshot: pointer.cast::<ClosureValueSnapshot>().as_mut(),
                },
                PayloadKind::Cell => {
                    let value = pointer.cast::<CellPayload>().as_mut();
                    HeapObject::Cell {
                        ty: &mut value.0,
                        value: &mut value.1,
                    }
                }
            }
        })
    }
}
