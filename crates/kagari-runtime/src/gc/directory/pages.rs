//! One nonmoving allocator and disposal protocol with small-slab and large controls.
mod allocation;
mod backing;
pub(in crate::gc) mod batch;
#[cfg(test)]
mod header_tests;
pub(super) mod retirement;
mod sweep;
mod take;
#[cfg(test)]
mod tests;

use self::{
    allocation::{Allocation, Large, Links, Small},
    backing::Backing,
    sweep::Cursor,
};
use crate::{
    error::RuntimeError,
    gc::{
        directory::{
            PayloadLocation,
            descriptors::{Descriptor, DescriptorId, Descriptors},
        },
        graph::marks::{Domain, MarkRef, WorkGroup},
        object_header::ObjectHeader,
        reference::GcRef,
    },
    native::binding::NativeResult,
};
use std::{
    alloc::Layout,
    cell::Cell,
    iter,
    marker::PhantomData,
    mem,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    ptr::NonNull,
    sync::Arc,
};

#[cfg(test)]
use crate::gc::directory::PayloadKind;

#[cfg(any(test, feature = "execution-diagnostics"))]
use crate::gc::graph::marks::PageMarks;

pub(super) const SLAB_BYTES: usize = 1024;
pub(super) const CLASSES: [usize; 24] = [
    8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 448, 512, 640,
    768, 896, 1024,
];

const ALIGNMENTS: [usize; 2] = [16, 64];
const GROUPS_PER_ALIGNMENT: usize = CLASSES.len() * 2;
const GROUPS: usize = GROUPS_PER_ALIGNMENT * ALIGNMENTS.len();

#[derive(Debug)]
pub(super) struct Pages {
    allocations: Vec<Allocation>,
    free: Option<u32>,
    available: [Option<u32>; GROUPS],
    first_active: Option<u32>,
    sweep: Cursor,
    pub(super) descriptors: Descriptors,
    pub(super) backing_bytes: usize,
    _not_sync: PhantomData<Cell<()>>,
}

impl Default for Pages {
    fn default() -> Self {
        Self {
            allocations: Vec::new(),
            free: None,
            available: [None; GROUPS],
            first_active: None,
            sweep: Cursor::default(),
            descriptors: Descriptors::default(),
            backing_bytes: 0,
            _not_sync: PhantomData,
        }
    }
}

/// Initialized physical representation checked under the storage owner's borrow.
/// The descriptor retains dispatch and bounds; this proof cannot outlive mutation.
pub(super) struct CheckedPayload<'a> {
    pub(super) pointer: NonNull<u8>,
    pub(super) descriptor: &'a Descriptor,
}

impl<'a> CheckedPayload<'a> {
    /// Callers establish occupancy and allocation bounds under this same borrow.
    fn new(
        location: PayloadLocation,
        pointer: NonNull<u8>,
        id: DescriptorId,
        descriptor: &'a Descriptor,
    ) -> Option<Self> {
        // SAFETY: occupied slot and retained descriptor prove initialized
        // aligned header storage. Validate dispatch before reading the payload.
        let header = unsafe { ObjectHeader::read(pointer) };
        Self::from_header(location, pointer, header, id, descriptor)
    }

    /// Reuse a header read under the same uninterrupted physical-owner borrow.
    /// The caller has established occupancy and read this exact pointer; the copy
    /// grants no access independently of that borrow or the admitted descriptor.
    fn from_header(
        location: PayloadLocation,
        pointer: NonNull<u8>,
        header: ObjectHeader,
        id: DescriptorId,
        descriptor: &'a Descriptor,
    ) -> Option<Self> {
        if descriptor.kind != location.kind
            || header.descriptor_id != id.get()
            || header.allocation_id != location.page
        {
            return None;
        }
        // SAFETY: the complete payload lies within the descriptor's allocation.
        Some(CheckedPayload {
            pointer: unsafe { pointer.add(descriptor.payload_offset()) },
            descriptor,
        })
    }

    pub(super) fn direct_reference(&self) -> GcRef {
        let offset = self.descriptor.payload_offset();
        // SAFETY: validation retained the initialized header and original allocation.
        unsafe { GcRef::from_allocation(self.pointer.sub(offset)) }
    }
}

/// Detached payloads retain dispatch and backing independently of the live owner.
#[derive(Debug)]
pub(in crate::gc) struct Retired {
    location: PayloadLocation,
    backing: Arc<Backing>,
    descriptor: Arc<Descriptor>,
    descriptor_id: DescriptorId,
    offset: usize,
    initialized: bool,
}

impl Retired {
    pub(in crate::gc::directory) fn drop_attribution(&self) -> Option<(&'static str, usize)> {
        self.descriptor.destroy.map(|_| {
            (
                self.descriptor.kind.observation_owner(),
                self.descriptor.layout.size(),
            )
        })
    }

    pub(super) fn is_disposed(&self) -> bool {
        !self.initialized
    }

    #[cfg(test)]
    pub(super) fn pointer(&self) -> NonNull<u8> {
        // SAFETY: checked offset and backing retained through disposal.
        unsafe {
            self.backing
                .pointer()
                .add(self.offset + self.descriptor.payload_offset())
        }
    }

    /// Release descriptor provenance outside table borrows, containing a possible
    /// final-owner destructor panic separately from payload destruction.
    pub(super) fn release(self) -> bool {
        catch_unwind(AssertUnwindSafe(|| drop(self))).is_err()
    }

    pub(in crate::gc) fn dispose(&mut self) -> bool {
        let Self {
            descriptor,
            initialized,
            backing,
            offset,
            ..
        } = self;
        if !*initialized {
            return false;
        }
        *initialized = false;
        let Some(destroy) = descriptor.destroy else {
            return false;
        };
        catch_unwind(AssertUnwindSafe(|| {
            // SAFETY: initialization consumed before dispatch, even on panic.
            unsafe { destroy(backing.pointer().add(*offset + descriptor.payload_offset())) };
        }))
        .is_err()
    }
}

impl Drop for Retired {
    fn drop(&mut self) {
        self.dispose();
    }
}

impl Pages {
    pub(super) fn class(layout: Layout, edges: bool) -> Option<usize> {
        if layout.size() > SLAB_BYTES {
            return None;
        }
        let alignment = ALIGNMENTS
            .iter()
            .position(|&align| layout.align() <= align)?;
        CLASSES
            .iter()
            .position(|&stride| stride >= layout.size().max(1) && stride % layout.align() == 0)
            .map(|class| alignment * GROUPS_PER_ALIGNMENT + class * 2 + usize::from(edges))
    }

    fn small(&self, index: u32) -> &Small {
        let Allocation::Small(page) = &self.allocations[index as usize] else {
            unreachable!("small allocation")
        };
        page
    }

    fn small_mut(&mut self, index: u32) -> &mut Small {
        let Allocation::Small(page) = &mut self.allocations[index as usize] else {
            unreachable!("small allocation")
        };
        page
    }

    fn link_available(&mut self, index: u32) {
        let group = self.small(index).group();
        let next = self.available[group];
        if let Some(next) = next {
            self.small_mut(next).state_mut().available.previous = index;
        }
        self.small_mut(index).state_mut().available = Links {
            previous: u32::MAX,
            next: next.unwrap_or(u32::MAX),
        };
        self.available[group] = Some(index);
    }

    fn unlink_available(&mut self, index: u32) {
        let page = self.small(index);
        let (group, links) = (page.group(), page.state().available);
        if let Some(previous) = links.previous() {
            self.small_mut(previous).state_mut().available.next = links.next;
        } else {
            self.available[group] = links.next();
        }
        if let Some(next) = links.next() {
            self.small_mut(next).state_mut().available.previous = links.previous;
        }
        self.small_mut(index).state_mut().available = Links::default();
    }

    fn publish_allocation(&mut self, mut allocation: Allocation) -> NativeResult<u32> {
        let index = self
            .free
            .map_or_else(|| u32::try_from(self.allocations.len()).ok(), Some)
            .filter(|index| *index != u32::MAX)
            .ok_or_else(|| RuntimeError::resource_limit("physical allocation index"))?;
        if index as usize == self.allocations.len() {
            self.allocations
                .try_reserve(1)
                .map_err(|_| RuntimeError::resource_limit("physical allocation directory"))?;
        }
        let bytes = self
            .backing_bytes
            .checked_add(allocation.bytes())
            .ok_or_else(|| RuntimeError::resource_limit("backing accounting"))?;
        allocation.active_mut().next = self.first_active.unwrap_or(u32::MAX);
        if let Some(next) = self.first_active {
            self.allocations[next as usize].active_mut().previous = index;
        }
        if index as usize == self.allocations.len() {
            self.allocations.push(allocation);
        } else {
            let Allocation::Vacant(next) = self.allocations[index as usize] else {
                unreachable!("free physical allocation")
            };
            self.free = next;
            self.allocations[index as usize] = allocation;
        }
        self.first_active = Some(index);
        self.backing_bytes = bytes;
        Ok(index)
    }

    fn release_allocation(&mut self, index: u32) {
        drop(self.take_allocation(index));
    }

    fn take_allocation(&mut self, index: u32) -> Allocation {
        let allocation = &self.allocations[index as usize];
        let links = *allocation.active().unwrap();
        let bytes = allocation.bytes();
        self.sweep.unlink(index, links.next());
        if let Some(previous) = links.previous() {
            self.allocations[previous as usize].active_mut().next = links.next;
        } else {
            self.first_active = links.next();
        }
        if let Some(next) = links.next() {
            self.allocations[next as usize].active_mut().previous = links.previous;
        }
        let allocation = mem::replace(
            &mut self.allocations[index as usize],
            Allocation::Vacant(self.free),
        );
        self.free = Some(index);
        self.backing_bytes -= bytes;
        allocation
    }

    /// All fallible ownership transfers precede initialized publication. Typed
    /// placement cannot fail after writing T; partial struct bytes need no Drop.
    pub(super) fn insert(
        &mut self,
        descriptor: Arc<Descriptor>,
        initialize: impl FnOnce(NonNull<u8>) -> NativeResult<()>,
    ) -> NativeResult<PayloadLocation> {
        let id = self.descriptors.admit(descriptor.clone())?;
        let result = catch_unwind(AssertUnwindSafe(|| {
            self.insert_admitted(&descriptor, id, initialize)
        }));
        match result {
            Ok(Ok(location)) => Ok(location),
            Ok(Err(error)) => {
                self.descriptors.release(id);
                Err(error)
            }
            Err(error) => {
                self.descriptors.release(id);
                resume_unwind(error)
            }
        }
    }

    fn insert_admitted(
        &mut self,
        descriptor: &Descriptor,
        id: DescriptorId,
        initialize: impl FnOnce(NonNull<u8>) -> NativeResult<()>,
    ) -> NativeResult<PayloadLocation> {
        let (index, slot, pointer) =
            if let Some(group) = Self::class(descriptor.layout, descriptor.edges) {
                let index = if let Some(index) = self.available[group] {
                    index
                } else {
                    let index = self.publish_allocation(Allocation::Small(Small::new(group)?))?;
                    self.link_available(index);
                    index
                };
                let page = self.small_mut(index);
                let slot = page.reserve();
                let pointer = page.pointer(slot);
                if page.state().free == 0 {
                    self.unlink_available(index);
                }
                (index, slot, pointer)
            } else {
                let large = Large::new(descriptor.layout)?;
                let pointer = large.backing().pointer();
                let index = self.publish_allocation(Allocation::Large(large))?;
                (index, 0, pointer)
            };
        let location = PayloadLocation {
            kind: descriptor.kind,
            page: index,
            slot: u16::try_from(slot).expect("bounded slab slot"),
        };
        // SAFETY: the descriptor reserves both the header and the complete payload.
        // A zero-sized payload may point just past the header and is never read.
        let payload = unsafe { pointer.add(descriptor.payload_offset()) };
        match catch_unwind(AssertUnwindSafe(|| initialize(payload))) {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                self.finish_slot(location);
                return Err(error);
            }
            Err(error) => {
                self.finish_slot(location);
                resume_unwind(error);
            }
        }
        // SAFETY: initialization succeeded in an exclusively reserved slot.
        // Occupancy is published below; descriptor and allocation IDs remain
        // admitted through detach/disposal/finish, including unwinding Drop.
        unsafe {
            ObjectHeader {
                descriptor_id: id.get(),
                allocation_id: index,
            }
            .write(pointer);
        }
        match &mut self.allocations[index as usize] {
            Allocation::Small(page) => {
                let parts = page.parts_mut();
                let words = parts.bits.len() / 2;
                parts.bits[words + slot / 64] &= !(1 << (slot % 64));
                parts.bits[slot / 64] |= 1 << (slot % 64);
            }
            Allocation::Large(large) => *large.descriptor_mut() = Some(id),
            Allocation::Vacant(_) => unreachable!(),
        }
        Ok(location)
    }

    pub(super) fn direct_mark(&self, location: PayloadLocation) -> Option<MarkRef<'_>> {
        self.allocations[location.page as usize]
            .marks()
            .map(|marks| {
                marks.group_slot(
                    WorkGroup {
                        domain: Domain::Direct,
                        block: location.page as usize,
                    },
                    location.slot as usize,
                )
            })
    }

    pub(super) fn direct_reference(&self, location: PayloadLocation) -> Option<GcRef> {
        Some(self.get(location)?.direct_reference())
    }

    pub(super) fn next_gray_direct(&self, block: usize, epoch: u64) -> (Option<GcRef>, bool) {
        let (index, more) = self.allocations[block]
            .marks()
            .expect("queued direct page")
            .next_gray(epoch);
        let reference = index.map(|index| {
            let pointer = match &self.allocations[block] {
                Allocation::Small(page) => page.pointer(index),
                Allocation::Large(large) => large.backing().pointer(),
                Allocation::Vacant(_) => unreachable!("queued direct allocation"),
            };
            // SAFETY: a gray page slot remains initialized until mark closure.
            // Resolve its header through the same owner proof as ordinary access.
            let reference = unsafe { GcRef::from_allocation(pointer) };
            self.resolve_reference(reference)
                .expect("gray owner retains its physical allocation");
            reference
        });
        (reference, more)
    }

    /// Resolve a retained reference to its original physical allocation.
    /// The caller retains the referenced object; checking an ID cannot revive it.
    pub(super) fn direct_location(&self, reference: GcRef) -> Option<PayloadLocation> {
        self.resolve_reference(reference)
            .map(|(location, _)| location)
    }

    /// Resolve location, initialized payload and descriptor under one owner borrow.
    /// The retained reference has the same liveness obligation as direct_location.
    pub(super) fn resolve_reference(
        &self,
        reference: GcRef,
    ) -> Option<(PayloadLocation, CheckedPayload<'_>)> {
        // SAFETY: internal transport is protected by its source frame/root/edge.
        // A foreign live allocation is readable, but cannot match this owner's base.
        let header = unsafe { ObjectHeader::read(reference.pointer()) };
        let (slot, id) = match self.allocations.get(header.allocation_id as usize)? {
            Allocation::Small(page) => {
                let offset = reference
                    .pointer()
                    .as_ptr()
                    .addr()
                    .checked_sub(page.backing().pointer().as_ptr().addr())?;
                if offset % page.stride() != 0 || offset / page.stride() >= page.slot_count() {
                    return None;
                }
                let slot = offset / page.stride();
                if page.bits()[slot / 64] & (1 << (slot % 64)) == 0 {
                    return None;
                }
                (slot, DescriptorId::from_header(header.descriptor_id)?)
            }
            Allocation::Large(large) if large.backing().pointer() == reference.pointer() => {
                (0, large.header().descriptor?)
            }
            _ => return None,
        };
        let descriptor = self.descriptors.get(id)?;
        let location = PayloadLocation {
            kind: descriptor.kind,
            page: header.allocation_id,
            slot: u16::try_from(slot).ok()?,
        };
        let payload = CheckedPayload::new(location, reference.pointer(), id, descriptor)?;
        Some((location, payload))
    }

    pub(super) fn get(&self, location: PayloadLocation) -> Option<CheckedPayload<'_>> {
        let (pointer, id) = match self.allocations.get(location.page as usize)? {
            Allocation::Small(page) => {
                let id = page.descriptor(location.slot as usize)?;
                (page.pointer(location.slot as usize), id)
            }
            Allocation::Large(large) if location.slot == 0 => {
                (large.backing().pointer(), large.header().descriptor?)
            }
            _ => return None,
        };
        CheckedPayload::new(location, pointer, id, self.descriptors.get(id)?)
    }

    pub(super) fn occupied(&self) -> impl Iterator<Item = PayloadLocation> + '_ {
        iter::successors(self.first_active, |&index| {
            self.allocations[index as usize].active().unwrap().next()
        })
        .flat_map(move |index| {
            let allocation = &self.allocations[index as usize];
            let count = match allocation {
                Allocation::Small(page) => page.slot_count(),
                Allocation::Large(_) => 1,
                Allocation::Vacant(_) => 0,
            };
            (0..count).filter_map(move |slot| {
                let id = match allocation {
                    Allocation::Small(page) => page.descriptor(slot)?,
                    Allocation::Large(large) => large.header().descriptor?,
                    Allocation::Vacant(_) => return None,
                };
                Some(PayloadLocation {
                    kind: self
                        .descriptors
                        .get(id)
                        .expect("admitted occupied descriptor")
                        .kind,
                    page: index,
                    slot: slot as u16,
                })
            })
        })
    }

    pub(super) fn detach(&mut self, location: PayloadLocation) -> Option<Retired> {
        self.get(location)?;
        let (backing, id, offset) = match &mut self.allocations[location.page as usize] {
            Allocation::Small(page) => {
                let slot = location.slot as usize;
                let id = page.descriptor(slot)?;
                let parts = page.parts_mut();
                let words = parts.bits.len() / 2;
                parts.bits[slot / 64] &= !(1 << (slot % 64));
                parts.bits[words + slot / 64] |= 1 << (slot % 64);
                (page.backing().clone(), id, slot * page.stride())
            }
            Allocation::Large(large) => {
                (large.backing().clone(), large.descriptor_mut().take()?, 0)
            }
            Allocation::Vacant(_) => unreachable!(),
        };
        Some(Retired {
            location,
            backing,
            descriptor: self.descriptors.get(id)?.clone(),
            descriptor_id: id,
            offset,
            initialized: true,
        })
    }

    pub(super) fn finish(&mut self, retired: Retired) -> (usize, Retired) {
        let released = self.finish_payload(&retired);
        (released, retired)
    }

    pub(in crate::gc::directory) fn finish_payload(&mut self, retired: &Retired) -> usize {
        assert!(retired.is_disposed());
        let (location, descriptor_id) = (retired.location, retired.descriptor_id);
        let before = self.backing_bytes;
        self.finish_slot(location);
        self.descriptors.release(descriptor_id);
        // Keep the last backing/descriptor owners until the caller releases its
        // storage borrow. Descriptor provenance can itself own drop-bearing state.
        before - self.backing_bytes
    }

    fn finish_slot(&mut self, location: PayloadLocation) {
        match &mut self.allocations[location.page as usize] {
            Allocation::Small(page) => {
                let slot = location.slot as usize;
                let parts = page.parts_mut();
                let words = parts.bits.len() / 2;
                assert_ne!(parts.bits[words + slot / 64] & (1 << (slot % 64)), 0);
                parts.bits[words + slot / 64] &= !(1 << (slot % 64));
                let was_full = parts.state.free == 0;
                parts.state.free += 1;
                if parts.state.free == parts.count {
                    if !was_full {
                        self.unlink_available(location.page);
                    }
                    self.release_allocation(location.page);
                } else if was_full {
                    self.link_available(location.page);
                }
            }
            Allocation::Large(large) => {
                assert!(large.header().descriptor.is_none());
                self.release_allocation(location.page);
            }
            Allocation::Vacant(_) => unreachable!("retiring allocation"),
        }
    }

    #[cfg(test)]
    pub(super) fn corrupt_structure(&mut self, location: PayloadLocation) {
        let id = match &self.allocations[location.page as usize] {
            Allocation::Small(page) => page.descriptor(location.slot as usize).unwrap(),
            Allocation::Large(large) => large.header().descriptor.unwrap(),
            _ => unreachable!(),
        };
        self.descriptors.corrupt_structure(id);
    }

    #[cfg(any(test, feature = "execution-diagnostics"))]
    pub(super) fn usage(&self) -> PageUsage {
        let mut usage = PageUsage {
            #[cfg(feature = "execution-diagnostics")]
            directory: self.allocations.capacity() * size_of::<Allocation>(),
            ..PageUsage::default()
        };
        for allocation in &self.allocations {
            if allocation.marks().is_some() {
                usage.mark_pages += 1;
                usage.mark_bytes += size_of::<PageMarks>();
            }
            match allocation {
                Allocation::Small(page) => {
                    let bitmaps = mem::size_of_val(page.bits());
                    let header = page.control_bytes();
                    let payload = page.backing().payload_bytes();
                    usage.slabs += payload;
                    usage.bitmaps += bitmaps;
                    usage.small_controls += header;
                    usage.padding += page.backing().layout().size() - payload - bitmaps - header;
                    usage.backing_controls += Backing::control_bytes();
                }
                Allocation::Large(large) => {
                    let payload = large.backing().payload_bytes();
                    let header = large.control_bytes();
                    usage.large += payload;
                    usage.large_controls += header;
                    usage.padding += large.backing().layout().size() - payload - header;
                    usage.backing_controls += Backing::control_bytes();
                }
                Allocation::Vacant(_) => {}
            }
        }
        usage
    }
}

#[cfg(any(test, feature = "execution-diagnostics"))]
#[derive(Default)]
pub(super) struct PageUsage {
    pub(super) mark_pages: usize,
    pub(super) mark_bytes: usize,
    pub(super) slabs: usize,
    pub(super) large: usize,
    pub(super) bitmaps: usize,
    #[cfg(feature = "execution-diagnostics")]
    pub(super) directory: usize,
    pub(super) small_controls: usize,
    pub(super) large_controls: usize,
    pub(super) backing_controls: usize,
    pub(super) padding: usize,
}

impl Drop for Pages {
    fn drop(&mut self) {
        // Fallback owner destruction also handles outstanding detached tokens:
        // they retain their own backing/descriptor and are not enumerated here.
        loop {
            let location = self.occupied().next();
            let Some(location) = location else {
                break;
            };
            let mut retired = self.detach(location).unwrap();
            retired.dispose();
            let (_, retired) = self.finish(retired);
            retired.release();
        }
    }
}
