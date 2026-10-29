//! Compile-target layout facts shared by both edge groups of each small class.
#[cfg(test)]
mod tests;

use super::SmallHeader;
use crate::gc::{
    directory::pages::{CLASSES, SLAB_BYTES},
    graph::marks::PageMarks,
};
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

#[derive(Debug, Clone, Copy)]
pub(super) struct ClassGeometry {
    pub(super) stride: usize,
    pub(super) slot_count: usize,
    /// Both occupied and retiring planes; each has half these words.
    pub(super) bitmap_words: usize,
    pub(super) bits_offset: usize,
    pub(super) marks_offset: usize,
    pub(super) trailer: Layout,
}

impl ClassGeometry {
    const fn new(stride: usize) -> Self {
        assert!(stride != 0 && stride <= SLAB_BYTES);
        let slot_count = SLAB_BYTES / stride;
        let bitmap_words = slot_count.div_ceil(64) * 2;
        let bits = match Layout::array::<u64>(bitmap_words) {
            Ok(layout) => layout,
            Err(_) => panic!("invalid slab bitmap layout"),
        };
        let (trailer, bits_offset) = match Layout::new::<SmallHeader>().extend(bits) {
            Ok(layout) => layout,
            Err(_) => panic!("invalid slab bitmap trailer"),
        };
        let (trailer, marks_offset) = match trailer.extend(Layout::new::<PageMarks>()) {
            Ok(layout) => layout,
            Err(_) => panic!("invalid slab mark trailer"),
        };
        let (layout, header_offset) = match PAYLOAD_LAYOUT.extend(trailer) {
            Ok(layout) => layout,
            Err(_) => panic!("invalid slab composite layout"),
        };
        assert!(header_offset == HEADER_OFFSET);
        // Keep Layout::extend's exact size, without trailing pad_to_align.
        assert!(layout.size() == header_offset + trailer.size());
        Self {
            stride,
            slot_count,
            bitmap_words,
            bits_offset: header_offset + bits_offset,
            marks_offset: header_offset + marks_offset,
            trailer,
        }
    }
}
