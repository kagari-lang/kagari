//! Typed views over sealed placement kinds; references stay within the owner borrow.
use crate::{
    closure::ClosureValueSnapshot,
    error_trace::ErrorTrace,
    execution_metadata::interfaces::InterfaceStore,
    gc::{
        directory::{
            ObjectStorage, PayloadKind, PayloadLocation, ResolvedDirect, pages::CheckedPayload,
        },
        fields::{StructView, StructViewMut},
        reference::GcRef,
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
    pub(in crate::gc) fn resolve_direct(&self, reference: GcRef) -> Option<ResolvedDirect<'_>> {
        let (location, pointer, descriptor) = self.pages.resolve_reference(reference)?;
        let fields = descriptor.structure.as_deref().map(|description| {
            // SAFETY: resolution retained initialized packed fields and their layout.
            let bytes = unsafe { slice::from_raw_parts(pointer.as_ptr(), description.size) };
            StructView { description, bytes }
        });
        Some(ResolvedDirect {
            fields,
            mark: self.pages.direct_mark(location)?,
            edges: descriptor.edges,
        })
    }

    pub(in crate::gc) fn native(&self, location: PayloadLocation) -> Option<&NativeObject> {
        if location.kind != PayloadKind::Native {
            return None;
        }
        let pointer = self.pages.get(location)?.pointer;
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
        let pointer = self.pages.get(location)?.pointer;
        // SAFETY: exclusive directory borrow; payload loans separately exclude T access.
        Some(unsafe { pointer.cast::<NativeObject>().as_mut() })
    }

    pub(in crate::gc) fn structure(&self, location: PayloadLocation) -> Option<StructView<'_>> {
        self.pages.get(location)?.structure()
    }

    pub(in crate::gc) fn structure_mut(
        &mut self,
        location: PayloadLocation,
    ) -> Option<StructViewMut<'_>> {
        let CheckedPayload {
            pointer,
            descriptor,
        } = self.pages.get(location)?;
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
        self.pages.get(location)?.view(interfaces)
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
                    .directory_binding(self.pages.get(location)?.interface_snapshot_index()?)?,
            });
        }
        if location.kind == PayloadKind::Struct {
            return self
                .structure_mut(location)
                .map(|storage| HeapObject::Struct { storage });
        }
        let pointer = self.pages.get(location)?.pointer;
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

impl<'a> CheckedPayload<'a> {
    fn interface_snapshot_index(&self) -> Option<u32> {
        if self.descriptor.kind != PayloadKind::Interface {
            return None;
        }
        // SAFETY: the occupied sealed interface representation contains one u32.
        Some(unsafe { self.pointer.cast::<u32>().as_ptr().read() })
    }

    pub(super) fn structure(&self) -> Option<StructView<'a>> {
        let description = self.descriptor.structure.as_deref()?;
        // SAFETY: the checked descriptor and initialized bytes share the owner borrow.
        let bytes = unsafe { slice::from_raw_parts(self.pointer.as_ptr(), description.size) };
        Some(StructView { description, bytes })
    }

    pub(super) fn view(
        &self,
        interfaces: &RefCell<InterfaceStore>,
    ) -> Option<HeapObject<Shared<'a>>> {
        let pointer = self.pointer;
        // SAFETY: each private placement arm pairs its kind with precisely the type
        // below. Occupancy is checked and every returned reference is owner-borrowed.
        Some(unsafe {
            match self.descriptor.kind {
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
                PayloadKind::Struct => HeapObject::Struct {
                    storage: self.structure()?,
                },
                PayloadKind::Interface => HeapObject::Interface {
                    snapshot: interfaces
                        .try_borrow()
                        .ok()?
                        .directory_binding(self.interface_snapshot_index()?)?,
                },
                PayloadKind::Vacant => unreachable!("checked physical descriptor"),
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
}
