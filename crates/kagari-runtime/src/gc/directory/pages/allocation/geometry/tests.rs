use super::*;
use crate::gc::directory::pages::{GROUPS_PER_ALIGNMENT, allocation::Small};
use std::ptr;

#[test]
fn class_geometry_shares_disjoint_controls_across_scan_and_alignment_groups() {
    for (index, stride) in CLASSES.into_iter().enumerate() {
        let count = SLAB_BYTES / stride;
        let words = count.div_ceil(64) * 2;
        let (trailer, bits_offset) = Layout::new::<SmallHeader>()
            .extend(Layout::array::<u64>(words).unwrap())
            .unwrap();
        let (trailer, marks_offset) = trailer.extend(Layout::new::<PageMarks>()).unwrap();
        for group in (0..ALIGNMENTS.len()).flat_map(|tier| {
            [
                tier * GROUPS_PER_ALIGNMENT + index * 2,
                tier * GROUPS_PER_ALIGNMENT + index * 2 + 1,
            ]
        }) {
            let payload = PAYLOAD_LAYOUTS[group / GROUPS_PER_ALIGNMENT];
            let (expected, header_offset) = payload.extend(trailer).unwrap();
            let page = Small::new(group).unwrap();
            let base = page.backing().pointer().as_ptr().addr();
            assert_eq!(base % payload.align(), 0);
            assert!(ptr::eq(page.geometry(), &GEOMETRIES[index]));
            assert_eq!(page.group(), group);
            assert_eq!(page.stride(), stride);
            assert_eq!(page.slot_count(), count);
            assert_eq!(page.bits().len(), words);
            assert_eq!(page.state().free, count);
            assert!(page.bits().iter().all(|&bits| bits == 0));
            assert_eq!(page.backing().layout(), expected);
            assert_eq!(page.backing().payload_bytes(), SLAB_BYTES);
            assert_eq!(
                page.bits().as_ptr().addr() - base,
                header_offset + bits_offset
            );
            assert_eq!(
                (page.marks() as *const PageMarks).addr() - base,
                header_offset + marks_offset
            );
            assert_eq!(
                page.pointer(count - 1).as_ptr().addr() - base,
                (count - 1) * stride
            );
            assert_eq!(
                expected.size(),
                header_offset + marks_offset + size_of::<PageMarks>()
            );
        }
    }
}
