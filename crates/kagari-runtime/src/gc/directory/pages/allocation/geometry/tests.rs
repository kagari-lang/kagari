use super::*;
use crate::gc::directory::pages::allocation::Small;
use std::ptr;

#[test]
fn class_geometry_preserves_disjoint_controls_and_is_shared_by_both_scan_groups() {
    for (index, stride) in CLASSES.into_iter().enumerate() {
        let count = SLAB_BYTES / stride;
        let words = count.div_ceil(64) * 2;
        let (trailer, bits_offset) = Layout::new::<SmallHeader>()
            .extend(Layout::array::<u64>(words).unwrap())
            .unwrap();
        let (trailer, marks_offset) = trailer.extend(Layout::new::<PageMarks>()).unwrap();
        let (expected, header_offset) = PAYLOAD_LAYOUT.extend(trailer).unwrap();
        for group in [index * 2, index * 2 + 1] {
            let page = Small::new(group).unwrap();
            let base = page.backing().pointer().as_ptr().addr();
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
