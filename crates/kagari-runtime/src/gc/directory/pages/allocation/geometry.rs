//! Compile-target layout facts shared by both edge groups of each small class.
#[cfg(test)]
mod tests;

use super::{Slot, SmallHeader};
use crate::gc::directory::pages::{CLASSES, SLAB_BYTES};
use std::alloc::Layout;

pub(super) const PAYLOAD_LAYOUT: Layout = match Layout::from_size_align(SLAB_BYTES, 16) {
    Ok(layout) => layout,
    Err(_) => panic!("invalid slab payload layout"),
};
pub(super) const HEADER_OFFSET: usize = match PAYLOAD_LAYOUT.extend(Layout::new::<SmallHeader>()) {
    Ok((_, offset)) => offset,
    Err(_) => panic!("invalid slab header layout"),
};
pub(super) static GEOMETRIES: [ClassGeometry; CLASSES.len()] = {
    let mut entries = [ClassGeometry::new(CLASSES[0]); CLASSES.len()];
    let mut index = 0;
    while index < CLASSES.len() {
        entries[index] = ClassGeometry::new(CLASSES[index]);
        index += 1;
    }
    entries
};

pub(super) static STRUCT_GEOMETRIES: [ClassGeometry; CLASSES.len()] = {
    let mut entries = [ClassGeometry::with_metadata(CLASSES[0], false); CLASSES.len()];
    let mut index = 0;
    while index < CLASSES.len() {
        entries[index] = ClassGeometry::with_metadata(CLASSES[index], false);
        index += 1;
    }
    entries
};

#[derive(Debug, Clone, Copy)]
pub(super) struct ClassGeometry {
    pub(super) stride: usize,
    pub(super) slot_count: usize,
    pub(super) metadata_count: usize,
    /// Both occupied and retiring planes; each has half these words.
    pub(super) bitmap_words: usize,
    pub(super) slots_offset: usize,
    pub(super) bits_offset: usize,
    pub(super) trailer: Layout,
}

impl ClassGeometry {
    const fn new(stride: usize) -> Self {
        Self::with_metadata(stride, true)
    }

    const fn with_metadata(stride: usize, indexed: bool) -> Self {
        assert!(stride != 0 && stride <= SLAB_BYTES);
        let slot_count = SLAB_BYTES / stride;
        let bitmap_words = slot_count.div_ceil(64) * 2;
        let metadata_count = if indexed { slot_count } else { 0 };
        let slots = match Layout::array::<Slot>(metadata_count) {
            Ok(layout) => layout,
            Err(_) => panic!("invalid slab slot layout"),
        };
        let bits = match Layout::array::<u64>(bitmap_words) {
            Ok(layout) => layout,
            Err(_) => panic!("invalid slab bitmap layout"),
        };
        let (trailer, slots_offset) = match Layout::new::<SmallHeader>().extend(slots) {
            Ok(layout) => layout,
            Err(_) => panic!("invalid slab slot trailer"),
        };
        let (trailer, bits_offset) = match trailer.extend(bits) {
            Ok(layout) => layout,
            Err(_) => panic!("invalid slab bitmap trailer"),
        };
        let (layout, header_offset) = match PAYLOAD_LAYOUT.extend(trailer) {
            Ok(layout) => layout,
            Err(_) => panic!("invalid slab composite layout"),
        };
        assert!(header_offset == HEADER_OFFSET);
        // Keep Layout::extend's exact size, without trailing pad_to_align.
        assert!(layout.size() == header_offset + bits_offset + bits.size());
        Self {
            stride,
            slot_count,
            metadata_count,
            bitmap_words,
            slots_offset: header_offset + slots_offset,
            bits_offset: header_offset + bits_offset,
            trailer,
        }
    }
}
