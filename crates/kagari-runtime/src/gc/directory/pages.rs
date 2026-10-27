//! One nonmoving allocator and disposal protocol with small-slab and large controls.
mod backing;
pub(super) mod retirement;
mod sweep;
#[cfg(test)]
mod tests;

use self::{backing::Backing, sweep::Cursor};
use crate::{
    error::RuntimeError,
    gc::directory::{
        PayloadKind, PayloadLocation,
        descriptors::{Descriptor, DescriptorId, Descriptors},
    },
    native::{binding::NativeResult, payload::take::NativeTake, storage::NativeObject},
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

pub(super) const SLAB_BYTES: usize = 1024;
pub(super) const CLASSES: [usize; 24] = [
    8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 448, 512, 640,
    768, 896, 1024,
];

#[derive(Debug, Default)]
struct Slot {
    directory: u32,
    descriptor: Option<DescriptorId>,
}

/// Allocation admission reserves MAX as the absent link, avoiding two padded
/// Option<u32> fields per list while retaining full owner-local mark epochs.
#[derive(Debug, Clone, Copy)]
struct Links {
    previous: u32,
    next: u32,
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
    fn previous(self) -> Option<u32> {
        (self.previous != u32::MAX).then_some(self.previous)
    }

    fn next(self) -> Option<u32> {
        (self.next != u32::MAX).then_some(self.next)
    }
}

#[derive(Debug)]
struct Small {
    backing: Arc<Backing>,
    slots: Box<[Slot]>,
    // Occupied then retiring words, sized to this class's actual slot count.
    bits: Box<[u64]>,
    stride: usize,
    group: usize,
    free: usize,
    available: Links,
    active: Links,
}

impl Small {
    fn new(group: usize) -> NativeResult<Self> {
        let stride = CLASSES[group / 2];
        let count = SLAB_BYTES / stride;
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(count)
            .map_err(|_| RuntimeError::resource_limit("slab slots"))?;
        slots.resize_with(count, Slot::default);
        let mut bits = Vec::new();
        bits.try_reserve_exact(count.div_ceil(64) * 2)
            .map_err(|_| RuntimeError::resource_limit("slab bitmaps"))?;
        bits.resize(count.div_ceil(64) * 2, 0);
        Ok(Self {
            backing: Backing::new(Layout::from_size_align(SLAB_BYTES, 16).unwrap())?,
            slots: slots.into_boxed_slice(),
            bits: bits.into_boxed_slice(),
            stride,
            group,
            free: count,
            available: Links::default(),
            active: Links::default(),
        })
    }

    fn pointer(&self, slot: usize) -> NonNull<u8> {
        assert!(slot < self.slots.len());
        // SAFETY: checked slab range, stride is aligned for every admitted layout.
        unsafe { self.backing.pointer.add(slot * self.stride) }
    }

    fn reserve(&mut self) -> usize {
        let words = self.bits.len() / 2;
        for word in 0..words {
            let bits = !(self.bits[word] | self.bits[words + word]);
            let slot = word * 64 + bits.trailing_zeros() as usize;
            if bits != 0 && slot < self.slots.len() {
                self.bits[words + word] |= 1 << (slot % 64);
                self.free -= 1;
                return slot;
            }
        }
        unreachable!("nonfull slab")
    }
}

/// No slot array or slab-sized bitmaps on the large/overaligned path.
#[derive(Debug)]
struct Large {
    backing: Arc<Backing>,
    descriptor: Option<DescriptorId>,
    directory: u32,
    active: Links,
}

#[derive(Debug)]
enum Allocation {
    Small(Box<Small>),
    Large(Box<Large>),
    Vacant(Option<u32>),
}

impl Allocation {
    fn active(&self) -> Option<&Links> {
        match self {
            Self::Small(page) => Some(&page.active),
            Self::Large(large) => Some(&large.active),
            Self::Vacant(_) => None,
        }
    }

    fn active_mut(&mut self) -> &mut Links {
        match self {
            Self::Small(page) => &mut page.active,
            Self::Large(large) => &mut large.active,
            Self::Vacant(_) => unreachable!("active allocation"),
        }
    }

    fn bytes(&self) -> usize {
        match self {
            Self::Small(page) => page.backing.layout.size(),
            Self::Large(large) => large.backing.layout.size(),
            Self::Vacant(_) => 0,
        }
    }
}

#[derive(Debug)]
pub(super) struct Pages {
    allocations: Vec<Allocation>,
    free: Option<u32>,
    available: [Option<u32>; 48],
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
            available: [None; 48],
            first_active: None,
            sweep: Cursor::default(),
            descriptors: Descriptors::default(),
            backing_bytes: 0,
            _not_sync: PhantomData,
        }
    }
}

/// Detached payloads retain dispatch and backing independently of the live owner.
/// Interface wrappers have no allocation or destructor, but use the same retirement.
#[derive(Debug)]
pub(in crate::gc) enum Retired {
    Interface,
    Payload {
        location: PayloadLocation,
        backing: Arc<Backing>,
        descriptor: Arc<Descriptor>,
        descriptor_id: DescriptorId,
        offset: usize,
        initialized: bool,
    },
}

impl Retired {
    pub(super) fn is_disposed(&self) -> bool {
        matches!(
            self,
            Self::Interface
                | Self::Payload {
                    initialized: false,
                    ..
                }
        )
    }

    pub(super) fn pointer(&self) -> NonNull<u8> {
        let Self::Payload {
            backing, offset, ..
        } = self
        else {
            unreachable!("physical payload")
        };
        // SAFETY: checked offset and backing retained through disposal.
        unsafe { backing.pointer.add(*offset) }
    }

    pub(in crate::gc) fn take_native<T: NativeTake>(&mut self) -> T {
        let Self::Payload {
            descriptor,
            initialized: true,
            ..
        } = self
        else {
            unreachable!("live native take")
        };
        assert_eq!(descriptor.kind, PayloadKind::Native);
        // SAFETY: exclusively owned initialized Native header, checked TypeId inside take.
        unsafe {
            self.pointer()
                .cast::<NativeObject>()
                .as_mut()
                .take_payload::<T>()
        }
    }

    /// Release descriptor provenance outside table borrows, containing a possible
    /// final-owner destructor panic separately from payload destruction.
    pub(super) fn release(self) -> bool {
        catch_unwind(AssertUnwindSafe(|| drop(self))).is_err()
    }

    pub(in crate::gc) fn dispose(&mut self) -> bool {
        let Self::Payload {
            descriptor,
            initialized,
            backing,
            offset,
            ..
        } = self
        else {
            return false;
        };
        if !*initialized {
            return false;
        }
        *initialized = false;
        let Some(destroy) = descriptor.destroy else {
            return false;
        };
        catch_unwind(AssertUnwindSafe(|| {
            // SAFETY: initialization consumed before dispatch, even on panic.
            unsafe { destroy(backing.pointer.add(*offset)) };
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
        if layout.align() > 16 || layout.size() > 1024 {
            return None;
        }
        CLASSES
            .iter()
            .position(|&stride| stride >= layout.size().max(1) && stride % layout.align() == 0)
            .map(|class| class * 2 + usize::from(edges))
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
        let group = self.small(index).group;
        let next = self.available[group];
        if let Some(next) = next {
            self.small_mut(next).available.previous = index;
        }
        self.small_mut(index).available = Links {
            previous: u32::MAX,
            next: next.unwrap_or(u32::MAX),
        };
        self.available[group] = Some(index);
    }

    fn unlink_available(&mut self, index: u32) {
        let page = self.small(index);
        let (group, links) = (page.group, page.available);
        if let Some(previous) = links.previous() {
            self.small_mut(previous).available.next = links.next;
        } else {
            self.available[group] = links.next();
        }
        if let Some(next) = links.next() {
            self.small_mut(next).available.previous = links.previous;
        }
        self.small_mut(index).available = Links::default();
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
        directory: u32,
        descriptor: Arc<Descriptor>,
        initialize: impl FnOnce(NonNull<u8>) -> NativeResult<()>,
    ) -> NativeResult<PayloadLocation> {
        let id = self.descriptors.admit(descriptor.clone())?;
        let result = catch_unwind(AssertUnwindSafe(|| {
            self.insert_admitted(directory, &descriptor, id, initialize)
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
        directory: u32,
        descriptor: &Descriptor,
        id: DescriptorId,
        initialize: impl FnOnce(NonNull<u8>) -> NativeResult<()>,
    ) -> NativeResult<PayloadLocation> {
        let (index, slot, pointer) =
            if let Some(group) = Self::class(descriptor.layout, descriptor.edges) {
                let index = if let Some(index) = self.available[group] {
                    index
                } else {
                    let index =
                        self.publish_allocation(Allocation::Small(Box::new(Small::new(group)?)))?;
                    self.link_available(index);
                    index
                };
                let page = self.small_mut(index);
                let slot = page.reserve();
                let pointer = page.pointer(slot);
                if page.free == 0 {
                    self.unlink_available(index);
                }
                (index, slot, pointer)
            } else {
                let backing = Backing::new(descriptor.layout)?;
                let pointer = backing.pointer;
                let index = self.publish_allocation(Allocation::Large(Box::new(Large {
                    backing,
                    descriptor: None,
                    directory,
                    active: Links::default(),
                })))?;
                (index, 0, pointer)
            };
        let location = PayloadLocation {
            kind: descriptor.kind,
            page: index,
            slot: u16::try_from(slot).expect("bounded slab slot"),
        };
        match catch_unwind(AssertUnwindSafe(|| initialize(pointer))) {
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
        match &mut self.allocations[index as usize] {
            Allocation::Small(page) => {
                let words = page.bits.len() / 2;
                page.bits[words + slot / 64] &= !(1 << (slot % 64));
                page.bits[slot / 64] |= 1 << (slot % 64);
                page.slots[slot] = Slot {
                    directory,
                    descriptor: Some(id),
                };
            }
            Allocation::Large(large) => large.descriptor = Some(id),
            Allocation::Vacant(_) => unreachable!(),
        }
        Ok(location)
    }

    pub(super) fn directory(&self, location: PayloadLocation) -> Option<u32> {
        self.get(location)?;
        match self.allocations.get(location.page as usize)? {
            Allocation::Small(page) => Some(page.slots[location.slot as usize].directory),
            Allocation::Large(large) => Some(large.directory),
            Allocation::Vacant(_) => None,
        }
    }

    pub(super) fn get(&self, location: PayloadLocation) -> Option<(NonNull<u8>, &Descriptor)> {
        let (pointer, id) = match self.allocations.get(location.page as usize)? {
            Allocation::Small(page) => {
                let slot = page.slots.get(location.slot as usize)?;
                (page.pointer(location.slot as usize), slot.descriptor?)
            }
            Allocation::Large(large) if location.slot == 0 => {
                (large.backing.pointer, large.descriptor?)
            }
            _ => return None,
        };
        let descriptor = self.descriptors.get(id)?;
        (descriptor.kind == location.kind).then_some((pointer, descriptor))
    }

    fn occupied_slots(&self) -> impl Iterator<Item = (u32, u32, u16, DescriptorId)> + '_ {
        iter::successors(self.first_active, |&index| {
            self.allocations[index as usize].active().unwrap().next()
        })
        .flat_map(move |index| {
            let allocation = &self.allocations[index as usize];
            let count = match allocation {
                Allocation::Small(page) => page.slots.len(),
                Allocation::Large(_) => 1,
                _ => 0,
            };
            (0..count).filter_map(move |slot| {
                let (directory, id) = match allocation {
                    Allocation::Small(page) => {
                        (page.slots[slot].directory, page.slots[slot].descriptor?)
                    }
                    Allocation::Large(large) => (large.directory, large.descriptor?),
                    _ => return None,
                };
                Some((directory, index, slot as u16, id))
            })
        })
    }

    pub(super) fn occupied(&self) -> impl Iterator<Item = (u32, PayloadLocation)> + '_ {
        self.occupied_slots().map(|(directory, page, slot, id)| {
            let descriptor = self
                .descriptors
                .get(id)
                .expect("admitted occupied descriptor");
            (
                directory,
                PayloadLocation {
                    kind: descriptor.kind,
                    page,
                    slot,
                },
            )
        })
    }

    pub(super) fn detach(&mut self, location: PayloadLocation) -> Option<Retired> {
        self.get(location)?;
        let (backing, id, offset) = match &mut self.allocations[location.page as usize] {
            Allocation::Small(page) => {
                let slot = location.slot as usize;
                let id = page.slots[slot].descriptor.take()?;
                let words = page.bits.len() / 2;
                page.bits[slot / 64] &= !(1 << (slot % 64));
                page.bits[words + slot / 64] |= 1 << (slot % 64);
                (page.backing.clone(), id, slot * page.stride)
            }
            Allocation::Large(large) => (large.backing.clone(), large.descriptor.take()?, 0),
            Allocation::Vacant(_) => unreachable!(),
        };
        Some(Retired::Payload {
            location,
            backing,
            descriptor: self.descriptors.get(id)?.clone(),
            descriptor_id: id,
            offset,
            initialized: true,
        })
    }

    pub(super) fn finish(&mut self, retired: Retired) -> (usize, Retired) {
        assert!(retired.is_disposed());
        let Retired::Payload {
            location,
            descriptor_id,
            ..
        } = &retired
        else {
            return (0, retired);
        };
        let (location, descriptor_id) = (*location, *descriptor_id);
        let before = self.backing_bytes;
        self.finish_slot(location);
        self.descriptors.release(descriptor_id);
        // Keep the last backing/descriptor owners until the caller releases its
        // storage borrow. Descriptor provenance can itself own drop-bearing state.
        (before - self.backing_bytes, retired)
    }

    fn finish_slot(&mut self, location: PayloadLocation) {
        match &mut self.allocations[location.page as usize] {
            Allocation::Small(page) => {
                let slot = location.slot as usize;
                let words = page.bits.len() / 2;
                assert_ne!(page.bits[words + slot / 64] & (1 << (slot % 64)), 0);
                page.bits[words + slot / 64] &= !(1 << (slot % 64));
                let was_full = page.free == 0;
                page.free += 1;
                if page.free == page.slots.len() {
                    if !was_full {
                        self.unlink_available(location.page);
                    }
                    self.release_allocation(location.page);
                } else if was_full {
                    self.link_available(location.page);
                }
            }
            Allocation::Large(large) => {
                assert!(large.descriptor.is_none());
                self.release_allocation(location.page);
            }
            Allocation::Vacant(_) => unreachable!("retiring allocation"),
        }
    }

    #[cfg(test)]
    pub(super) fn corrupt_structure(&mut self, location: PayloadLocation) {
        let id = match &self.allocations[location.page as usize] {
            Allocation::Small(page) => page.slots[location.slot as usize].descriptor.unwrap(),
            Allocation::Large(large) => large.descriptor.unwrap(),
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
            match allocation {
                Allocation::Small(page) => {
                    usage.slabs += page.backing.layout.size();
                    usage.slots += page.slots.len() * size_of::<Slot>();
                    usage.bitmaps += page.bits.len() * size_of::<u64>();
                    usage.small_controls += size_of::<Small>();
                    usage.backing_controls += Backing::control_bytes();
                }
                Allocation::Large(large) => {
                    usage.large += large.backing.layout.size();
                    usage.large_controls += size_of::<Large>();
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
    pub(super) slabs: usize,
    pub(super) large: usize,
    pub(super) slots: usize,
    pub(super) bitmaps: usize,
    #[cfg(feature = "execution-diagnostics")]
    pub(super) directory: usize,
    pub(super) small_controls: usize,
    pub(super) large_controls: usize,
    pub(super) backing_controls: usize,
}

impl Drop for Pages {
    fn drop(&mut self) {
        // Fallback owner destruction also handles outstanding detached tokens:
        // they retain their own backing/descriptor and are not enumerated here.
        loop {
            let location = self.occupied().next().map(|(_, location)| location);
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
