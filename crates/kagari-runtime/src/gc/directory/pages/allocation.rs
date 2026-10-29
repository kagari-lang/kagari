//! Typed trailer ownership; references cover controls only, never payload bytes.
mod geometry;

use self::geometry::{ClassGeometry, GEOMETRIES, HEADER_OFFSET, PAYLOAD_LAYOUT, STRUCT_GEOMETRIES};
use super::{STRUCT_GROUP_START, backing::Backing};
use crate::{
    gc::{directory::descriptors::DescriptorId, object_header::ObjectHeader},
    native::binding::NativeResult,
};
use std::{alloc::Layout, mem::needs_drop, ptr::NonNull, slice, sync::Arc};

#[derive(Debug, Default, Clone, Copy)]
pub(super) struct Slot {
    pub(super) directory: u32,
    pub(super) descriptor: Option<DescriptorId>,
}

/// MAX is the absent link; admission rejects that physical directory index.
#[derive(Debug, Clone, Copy)]
pub(super) struct Links {
    pub(super) previous: u32,
    pub(super) next: u32,
}

impl Default for Links {
    fn default() -> Self {
        Self {
            previous: u32::MAX,
            next: u32::MAX,
        }
    }
}

impl Links {
    pub(super) fn previous(self) -> Option<u32> {
        (self.previous != u32::MAX).then_some(self.previous)
    }

    pub(super) fn next(self) -> Option<u32> {
        (self.next != u32::MAX).then_some(self.next)
    }
}

#[derive(Debug)]
pub(super) struct SmallState {
    pub(super) free: usize,
    pub(super) available: Links,
    pub(super) active: Links,
}

/// Geometry is initialized once and never exposed mutably. Mutable state cannot
/// change the lengths/offsets used to construct the trailing control slices.
#[derive(Debug)]
struct SmallHeader {
    geometry: &'static ClassGeometry,
    group: usize,
    state: SmallState,
}

/// The only control owner. Cloned backing Arcs retain bytes, not trailer access.
#[derive(Debug)]
pub(super) struct Small {
    backing: Arc<Backing>,
}

pub(super) struct SmallPartsMut<'a> {
    pub(super) state: &'a mut SmallState,
    pub(super) count: usize,
    pub(super) slots: &'a mut [Slot],
    pub(super) bits: &'a mut [u64],
}

impl Small {
    pub(super) fn backing(&self) -> &Arc<Backing> {
        &self.backing
    }

    pub(super) fn new(group: usize) -> NativeResult<Self> {
        let geometry = if group < STRUCT_GROUP_START {
            &GEOMETRIES[group / 2]
        } else {
            &STRUCT_GEOMETRIES[(group - STRUCT_GROUP_START) / 2]
        };
        const {
            assert!(!needs_drop::<SmallHeader>());
            assert!(!needs_drop::<Slot>());
        }
        let (backing, offset) = Backing::new(PAYLOAD_LAYOUT, geometry.trailer)?;
        assert_eq!(offset, HEADER_OFFSET);
        // SAFETY: checked composite layout, unique allocation, and disjoint trailing
        // regions. All control values are initialized before this owner is returned.
        // No control owns Drop state; allocation failure/unwind just frees the bytes.
        unsafe {
            backing
                .pointer()
                .add(offset)
                .cast::<SmallHeader>()
                .as_ptr()
                .write(SmallHeader {
                    geometry,
                    group,
                    state: SmallState {
                        free: geometry.slot_count,
                        available: Links::default(),
                        active: Links::default(),
                    },
                });
            let slots = backing
                .pointer()
                .add(geometry.slots_offset)
                .cast::<Slot>()
                .as_ptr();
            for index in 0..geometry.metadata_count {
                slots.add(index).write(Slot::default());
            }
            backing
                .pointer()
                .add(geometry.bits_offset)
                .cast::<u64>()
                .as_ptr()
                .write_bytes(0, geometry.bitmap_words);
        }
        Ok(Self { backing })
    }

    fn header_pointer(&self) -> NonNull<SmallHeader> {
        // SAFETY: only new constructs this non-Clone control owner; the initialized
        // header is aligned at the fixed offset and its backing remains retained.
        unsafe { self.backing.pointer().add(HEADER_OFFSET).cast() }
    }

    fn header(&self) -> &SmallHeader {
        // SAFETY: shared access through the sole trailer owner, not a reference to
        // the allocation/payload region. Mutable access requires &mut this owner.
        unsafe { self.header_pointer().as_ref() }
    }

    fn geometry(&self) -> &'static ClassGeometry {
        self.header().geometry
    }

    pub(super) fn stride(&self) -> usize {
        self.geometry().stride
    }

    pub(super) fn group(&self) -> usize {
        self.header().group
    }

    pub(super) fn state(&self) -> &SmallState {
        &self.header().state
    }

    pub(super) fn state_mut(&mut self) -> &mut SmallState {
        // SAFETY: exclusive control-owner borrow; only this state subregion is borrowed.
        unsafe { &mut (*self.header_pointer().as_ptr()).state }
    }

    pub(super) fn slots(&self) -> &[Slot] {
        let geometry = self.geometry();
        // SAFETY: immutable geometry and initialized, aligned slots; shared owner borrow.
        unsafe {
            slice::from_raw_parts(
                self.backing
                    .pointer()
                    .add(geometry.slots_offset)
                    .cast::<Slot>()
                    .as_ptr(),
                geometry.metadata_count,
            )
        }
    }

    pub(super) fn slot_count(&self) -> usize {
        self.geometry().slot_count
    }

    /// Struct slabs have no duplicate per-slot directory or descriptor records.
    /// Occupancy proves header initialization before reconstructing dispatch facts.
    pub(super) fn slot(&self, index: usize) -> Option<Slot> {
        if self.group() < STRUCT_GROUP_START {
            return self.slots().get(index).copied();
        }
        if index >= self.slot_count() || self.bits()[index / 64] & (1 << (index % 64)) == 0 {
            return None;
        }
        // SAFETY: occupied direct-struct slot, initialized under this sole owner.
        let header = unsafe { ObjectHeader::read(self.pointer(index)) };
        Some(Slot {
            directory: u32::MAX,
            descriptor: Some(DescriptorId::from_header(header.descriptor_id)?),
        })
    }

    pub(super) fn bits(&self) -> &[u64] {
        let geometry = self.geometry();
        // SAFETY: immutable geometry gives the checked trailing bitmap region.
        unsafe {
            slice::from_raw_parts(
                self.backing
                    .pointer()
                    .add(geometry.bits_offset)
                    .cast::<u64>()
                    .as_ptr(),
                geometry.bitmap_words,
            )
        }
    }

    pub(super) fn parts_mut(&mut self) -> SmallPartsMut<'_> {
        let geometry = self.geometry();
        // SAFETY: exclusive trailer owner and three initialized nonoverlapping regions.
        // Payloads and immutable geometry are not included in these mutable references.
        unsafe {
            SmallPartsMut {
                count: geometry.slot_count,
                state: &mut (*self.header_pointer().as_ptr()).state,
                slots: slice::from_raw_parts_mut(
                    self.backing
                        .pointer()
                        .add(geometry.slots_offset)
                        .cast::<Slot>()
                        .as_ptr(),
                    geometry.metadata_count,
                ),
                bits: slice::from_raw_parts_mut(
                    self.backing
                        .pointer()
                        .add(geometry.bits_offset)
                        .cast::<u64>()
                        .as_ptr(),
                    geometry.bitmap_words,
                ),
            }
        }
    }

    pub(super) fn pointer(&self, slot: usize) -> NonNull<u8> {
        let geometry = self.geometry();
        assert!(slot < geometry.slot_count);
        // SAFETY: checked payload slot, unchanged aligned class geometry.
        unsafe { self.backing.pointer().add(slot * geometry.stride) }
    }

    pub(super) fn reserve(&mut self) -> usize {
        let page = self.parts_mut();
        let words = page.bits.len() / 2;
        for word in 0..words {
            let bits = !(page.bits[word] | page.bits[words + word]);
            let slot = word * 64 + bits.trailing_zeros() as usize;
            if bits != 0 && slot < page.count {
                page.bits[words + slot / 64] |= 1 << (slot % 64);
                page.state.free -= 1;
                return slot;
            }
        }
        unreachable!("nonfull slab")
    }

    #[cfg(any(test, feature = "execution-diagnostics"))]
    pub(super) fn control_bytes(&self) -> usize {
        size_of::<SmallHeader>()
    }
}

#[derive(Debug)]
pub(super) struct LargeHeader {
    pub(super) descriptor: Option<DescriptorId>,
    pub(super) directory: u32,
    pub(super) active: Links,
}

#[derive(Debug)]
pub(super) struct Large {
    backing: Arc<Backing>,
}

impl Large {
    pub(super) fn backing(&self) -> &Arc<Backing> {
        &self.backing
    }

    pub(super) fn new(payload: Layout, directory: u32) -> NativeResult<Self> {
        const { assert!(!needs_drop::<LargeHeader>()) };
        let (backing, offset) = Backing::new(payload, Layout::new::<LargeHeader>())?;
        // SAFETY: the checked trailing layout is aligned and exclusively owned.
        unsafe {
            backing
                .pointer()
                .add(offset)
                .cast::<LargeHeader>()
                .as_ptr()
                .write(LargeHeader {
                    descriptor: None,
                    directory,
                    active: Links::default(),
                });
        }
        Ok(Self { backing })
    }

    fn header_pointer(&self) -> NonNull<LargeHeader> {
        // SAFETY: Layout::extend adds no trailing padding, so this initialized header
        // ends exactly at allocation size. The payload occupies a disjoint prefix.
        unsafe {
            self.backing
                .pointer()
                .add(self.backing.layout().size() - size_of::<LargeHeader>())
                .cast()
        }
    }

    pub(super) fn header(&self) -> &LargeHeader {
        // SAFETY: only this non-Clone owner grants trailer references.
        unsafe { self.header_pointer().as_ref() }
    }

    pub(super) fn header_mut(&mut self) -> &mut LargeHeader {
        // SAFETY: exclusive control-owner borrow; no reference covers payload bytes.
        unsafe { self.header_pointer().as_mut() }
    }

    #[cfg(any(test, feature = "execution-diagnostics"))]
    pub(super) fn control_bytes(&self) -> usize {
        size_of::<LargeHeader>()
    }
}

#[derive(Debug)]
pub(super) enum Allocation {
    Small(Small),
    Large(Large),
    Vacant(Option<u32>),
}

impl Allocation {
    pub(super) fn active(&self) -> Option<&Links> {
        match self {
            Self::Small(page) => Some(&page.state().active),
            Self::Large(large) => Some(&large.header().active),
            Self::Vacant(_) => None,
        }
    }

    pub(super) fn active_mut(&mut self) -> &mut Links {
        match self {
            Self::Small(page) => &mut page.state_mut().active,
            Self::Large(large) => &mut large.header_mut().active,
            Self::Vacant(_) => unreachable!("active allocation"),
        }
    }

    pub(super) fn bytes(&self) -> usize {
        match self {
            Self::Small(page) => page.backing.payload_bytes(),
            Self::Large(large) => large.backing.payload_bytes(),
            Self::Vacant(_) => 0,
        }
    }
}
