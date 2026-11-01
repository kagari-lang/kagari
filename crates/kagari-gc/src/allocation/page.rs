//! Coallocated slab controls; adapted from the Runtime's reviewed page geometry.

use crate::{error::GcError, marking::PageMarks, object::ObjectHeader};
use std::{
    alloc::{Layout, alloc, dealloc},
    ptr::NonNull,
    slice,
    sync::Arc,
};

pub(crate) const SLAB_BYTES: usize = 1024;
pub(crate) const STRIDES: [usize; 24] = [
    8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 448, 512, 640,
    768, 896, 1024,
];
pub(crate) const GROUPS: usize = 96;

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Links {
    pub(crate) previous: Option<u32>,
    pub(crate) next: Option<u32>,
}

#[derive(Debug)]
pub(crate) struct Backing {
    pub(crate) pointer: NonNull<u8>,
    pub(crate) layout: Layout,
}

// SAFETY: only Send objects are admitted; Arc grants no safe payload references.
unsafe impl Send for Backing {}

// SAFETY: trailer and payload access require the sole non-Clone page owner/token.
unsafe impl Sync for Backing {}

impl Backing {
    fn new(layout: Layout) -> Result<Arc<Self>, GcError> {
        // SAFETY: every composite layout contains a nonzero header and controls.
        let pointer = NonNull::new(unsafe { alloc(layout) }).ok_or(GcError::Allocation)?;
        Ok(Arc::new(Self { pointer, layout }))
    }
}

impl Drop for Backing {
    fn drop(&mut self) {
        // SAFETY: last token after all initialized objects have been disposed.
        unsafe { dealloc(self.pointer.as_ptr(), self.layout) };
    }
}

#[derive(Debug)]
struct SmallHeader {
    geometry: &'static Geometry,
    group: usize,
    free: usize,
    active: Links,
    available: Links,
}

#[derive(Debug, Clone, Copy)]
struct Geometry {
    stride: usize,
    slots: usize,
    words: usize,
    bits_offset: usize,
    marks_offset: usize,
    trailer: Layout,
}

impl Geometry {
    const fn new(stride: usize) -> Self {
        let slots = SLAB_BYTES / stride;
        let words = slots.div_ceil(64);
        let bits = match Layout::array::<u64>(words * 2) {
            Ok(layout) => layout,
            Err(_) => panic!("invalid constant bitmap layout"),
        };
        let (trailer, bits_offset) = match Layout::new::<SmallHeader>().extend(bits) {
            Ok(layout) => layout,
            Err(_) => panic!("invalid constant bitmap trailer"),
        };
        let (trailer, marks_offset) = match trailer.extend(Layout::new::<PageMarks>()) {
            Ok(layout) => layout,
            Err(_) => panic!("invalid constant mark trailer"),
        };
        Self {
            stride,
            slots,
            words,
            bits_offset: SLAB_BYTES + bits_offset,
            marks_offset: SLAB_BYTES + marks_offset,
            trailer,
        }
    }
}

static GEOMETRIES: [Geometry; STRIDES.len()] = {
    let mut geometries = [Geometry::new(STRIDES[0]); STRIDES.len()];
    let mut index = 0;
    while index < STRIDES.len() {
        geometries[index] = Geometry::new(STRIDES[index]);
        index += 1;
    }
    geometries
};

#[derive(Debug)]
struct LargeHeader {
    active: Links,
    occupied: bool,
    unavailable: bool,
    marks: PageMarks,
}

#[derive(Debug)]
pub(crate) enum Page {
    Small {
        backing: Arc<Backing>,
    },
    Large {
        backing: Arc<Backing>,
        trailer_offset: usize,
    },
}

impl Page {
    pub(crate) fn group(layout: Layout, edge_free: bool) -> Option<usize> {
        let tier = match layout.align() {
            1..=16 => 0,
            17..=64 => 1,
            _ => return None,
        };
        STRIDES
            .iter()
            .position(|stride| *stride >= layout.size() && *stride % layout.align() == 0)
            .map(|class| tier * 48 + class * 2 + usize::from(!edge_free))
    }

    pub(crate) fn new_small(group: usize) -> Result<Self, GcError> {
        let geometry = &GEOMETRIES[(group % 48) / 2];
        let layout = Self::backing_layout(Layout::new::<()>(), Some(group))?;
        let backing = Backing::new(layout)?;
        // SAFETY: disjoint aligned regions in the exclusively owned new allocation.
        unsafe {
            backing
                .pointer
                .add(SLAB_BYTES)
                .cast::<SmallHeader>()
                .as_ptr()
                .write(SmallHeader {
                    geometry,
                    group,
                    free: geometry.slots,
                    active: Links::default(),
                    available: Links::default(),
                });
            backing
                .pointer
                .add(geometry.bits_offset)
                .cast::<u64>()
                .as_ptr()
                .write_bytes(0, geometry.words * 2);
            backing
                .pointer
                .add(geometry.marks_offset)
                .cast::<PageMarks>()
                .as_ptr()
                .write(PageMarks::default());
        }
        Ok(Self::Small { backing })
    }

    pub(crate) fn new_large(layout: Layout) -> Result<Self, GcError> {
        let (layout, trailer_offset) = extend(layout, Layout::new::<LargeHeader>())?;
        let backing = Backing::new(layout)?;
        // SAFETY: aligned, uniquely owned trailer disjoint from the object layout.
        unsafe {
            backing
                .pointer
                .add(trailer_offset)
                .cast::<LargeHeader>()
                .as_ptr()
                .write(LargeHeader {
                    active: Links::default(),
                    occupied: false,
                    unavailable: false,
                    marks: PageMarks::default(),
                });
        }
        Ok(Self::Large {
            backing,
            trailer_offset,
        })
    }

    pub(crate) fn backing_layout(layout: Layout, group: Option<usize>) -> Result<Layout, GcError> {
        if let Some(group) = group {
            let payload =
                Layout::from_size_align(SLAB_BYTES, if group / 48 == 0 { 16 } else { 64 })
                    .map_err(|_| GcError::Limit("slab layout"))?;
            let (layout, offset) = extend(payload, GEOMETRIES[(group % 48) / 2].trailer)?;
            assert_eq!(offset, SLAB_BYTES);
            Ok(layout)
        } else {
            extend(layout, Layout::new::<LargeHeader>()).map(|(layout, _)| layout)
        }
    }

    pub(crate) fn backing(&self) -> &Arc<Backing> {
        match self {
            Self::Small { backing } | Self::Large { backing, .. } => backing,
        }
    }

    fn small(&self) -> &SmallHeader {
        assert!(matches!(self, Self::Small { .. }));
        // SAFETY: only the sole page owner exposes its initialized trailer.
        unsafe {
            self.backing()
                .pointer
                .add(SLAB_BYTES)
                .cast::<SmallHeader>()
                .as_ref()
        }
    }

    fn small_mut(&mut self) -> &mut SmallHeader {
        assert!(matches!(self, Self::Small { .. }));
        // SAFETY: exclusive access to the sole page owner, not to payload bytes.
        unsafe {
            &mut *self
                .backing()
                .pointer
                .add(SLAB_BYTES)
                .cast::<SmallHeader>()
                .as_ptr()
        }
    }

    fn large(&self) -> &LargeHeader {
        let Self::Large {
            backing,
            trailer_offset,
        } = self
        else {
            unreachable!()
        };
        // SAFETY: retained backing and initialized aligned large trailer.
        unsafe {
            backing
                .pointer
                .add(*trailer_offset)
                .cast::<LargeHeader>()
                .as_ref()
        }
    }

    fn large_mut(&mut self) -> &mut LargeHeader {
        let Self::Large {
            backing,
            trailer_offset,
        } = self
        else {
            unreachable!()
        };
        // SAFETY: exclusive page control, disjoint from the initialized object.
        unsafe {
            &mut *backing
                .pointer
                .add(*trailer_offset)
                .cast::<LargeHeader>()
                .as_ptr()
        }
    }

    fn bits(&self) -> &[u64] {
        let geometry = self.small().geometry;
        // SAFETY: initialized exact geometry-sized bitmap region.
        unsafe {
            slice::from_raw_parts(
                self.backing()
                    .pointer
                    .add(geometry.bits_offset)
                    .cast::<u64>()
                    .as_ptr(),
                geometry.words * 2,
            )
        }
    }

    fn bits_mut(&mut self) -> &mut [u64] {
        let geometry = self.small().geometry;
        // SAFETY: exclusive trailer owner, disjoint bitmap region only.
        unsafe {
            slice::from_raw_parts_mut(
                self.backing()
                    .pointer
                    .add(geometry.bits_offset)
                    .cast::<u64>()
                    .as_ptr(),
                geometry.words * 2,
            )
        }
    }

    pub(crate) fn group_id(&self) -> Option<usize> {
        match self {
            Self::Small { .. } => Some(self.small().group),
            Self::Large { .. } => None,
        }
    }

    pub(crate) fn slots(&self) -> usize {
        match self {
            Self::Small { .. } => self.small().geometry.slots,
            Self::Large { .. } => 1,
        }
    }

    pub(crate) fn free(&self) -> usize {
        match self {
            Self::Small { .. } => self.small().free,
            Self::Large { .. } => usize::from(!self.large().occupied && !self.large().unavailable),
        }
    }

    pub(crate) fn occupied(&self, slot: usize) -> bool {
        if slot >= self.slots() {
            return false;
        }
        match self {
            Self::Small { .. } => self.bits()[slot / 64] & (1 << (slot % 64)) != 0,
            Self::Large { .. } => self.large().occupied,
        }
    }

    /// First occupied white slot in the admitted interval, or end. Inspect at
    /// most two bitmap words; callers still charge every skipped logical slot.
    pub(crate) fn next_dead(&self, start: usize, end: usize, epoch: u64) -> usize {
        debug_assert!(start < end && end <= self.slots());
        let marks = self.marks();
        for word in start / 64..=(end - 1) / 64 {
            let occupied = match self {
                Self::Small { .. } => self.bits()[word],
                Self::Large { .. } => u64::from(self.large().occupied),
            };
            let lower = if word == start / 64 { start % 64 } else { 0 };
            let candidates = occupied & !marks.black_word(word, epoch) & (u64::MAX << lower);
            if candidates != 0 {
                return (word * 64 + candidates.trailing_zeros() as usize).min(end);
            }
        }
        end
    }

    pub(crate) fn marks(&self) -> &PageMarks {
        match self {
            Self::Small { .. } => {
                let offset = self.small().geometry.marks_offset;
                // SAFETY: immutable geometry identifies initialized disjoint marks.
                unsafe {
                    self.backing()
                        .pointer
                        .add(offset)
                        .cast::<PageMarks>()
                        .as_ref()
                }
            }
            Self::Large { .. } => &self.large().marks,
        }
    }

    pub(crate) fn pointer(&self, slot: usize) -> NonNull<ObjectHeader> {
        assert!(slot < self.slots());
        let stride = match self {
            Self::Small { .. } => self.small().geometry.stride,
            Self::Large { .. } => 0,
        };
        // SAFETY: admitted slot in the retained page's object region.
        unsafe { self.backing().pointer.add(slot * stride).cast() }
    }

    pub(crate) fn slot_of(&self, pointer: NonNull<ObjectHeader>) -> Option<usize> {
        let offset = pointer
            .as_ptr()
            .addr()
            .checked_sub(self.backing().pointer.as_ptr().addr())?;
        let slot = match self {
            Self::Small { .. } => {
                let stride = self.small().geometry.stride;
                if !offset.is_multiple_of(stride) {
                    return None;
                }
                offset / stride
            }
            Self::Large { .. } if offset == 0 => 0,
            Self::Large { .. } => return None,
        };
        (slot < self.slots()).then_some(slot)
    }

    pub(crate) fn reserve(&mut self) -> usize {
        let slot = match self {
            Self::Small { .. } => {
                let words = self.small().geometry.words;
                let slot = (0..words)
                    .find_map(|word| {
                        let available = !(self.bits()[word] | self.bits()[words + word]);
                        let slot = word * 64 + available.trailing_zeros() as usize;
                        (available != 0 && slot < self.slots()).then_some(slot)
                    })
                    .expect("nonfull page");
                self.bits_mut()[words + slot / 64] |= 1 << (slot % 64);
                self.small_mut().free -= 1;
                slot
            }
            Self::Large { .. } => {
                assert_eq!(self.free(), 1);
                self.large_mut().unavailable = true;
                0
            }
        };
        self.marks().clear_slot(slot);
        slot
    }

    pub(crate) fn commit(&mut self, slot: usize) {
        match self {
            Self::Small { .. } => {
                let words = self.small().geometry.words;
                self.bits_mut()[words + slot / 64] &= !(1 << (slot % 64));
                self.bits_mut()[slot / 64] |= 1 << (slot % 64);
            }
            Self::Large { .. } => {
                self.large_mut().unavailable = false;
                self.large_mut().occupied = true;
            }
        }
    }

    pub(crate) fn detach(&mut self, slot: usize) {
        assert!(self.occupied(slot));
        match self {
            Self::Small { .. } => {
                let words = self.small().geometry.words;
                self.bits_mut()[slot / 64] &= !(1 << (slot % 64));
                self.bits_mut()[words + slot / 64] |= 1 << (slot % 64);
            }
            Self::Large { .. } => {
                self.large_mut().occupied = false;
                self.large_mut().unavailable = true;
            }
        }
    }

    pub(crate) fn release(&mut self, slot: usize) {
        match self {
            Self::Small { .. } => {
                let words = self.small().geometry.words;
                self.bits_mut()[words + slot / 64] &= !(1 << (slot % 64));
                self.small_mut().free += 1;
            }
            Self::Large { .. } => self.large_mut().unavailable = false,
        }
    }

    pub(crate) fn active(&self) -> Links {
        match self {
            Self::Small { .. } => self.small().active,
            Self::Large { .. } => self.large().active,
        }
    }

    pub(crate) fn active_mut(&mut self) -> &mut Links {
        match self {
            Self::Small { .. } => &mut self.small_mut().active,
            Self::Large { .. } => &mut self.large_mut().active,
        }
    }

    pub(crate) fn available(&self) -> Links {
        self.small().available
    }

    pub(crate) fn available_mut(&mut self) -> &mut Links {
        &mut self.small_mut().available
    }
}

fn extend(left: Layout, right: Layout) -> Result<(Layout, usize), GcError> {
    left.extend(right)
        .map_err(|_| GcError::Limit("page trailer layout"))
}
