use super::*;
use crate::gc::directory::pages::allocation::{Small, SmallState};
use std::ptr;

#[test]
fn class_geometry_preserves_exact_layout_and_is_shared_by_both_scan_groups() {
    // Authenticate the replaced word, including target-dependent alignment/padding.
    struct PriorHeader {
        _stride: usize,
        _group: usize,
        _state: SmallState,
    }
    assert_eq!(Layout::new::<SmallHeader>(), Layout::new::<PriorHeader>());
    assert_eq!(
        size_of::<SmallHeader>(),
        2 * size_of::<usize>() + size_of::<SmallState>()
    );
    for (index, stride) in CLASSES.into_iter().enumerate() {
        let count = SLAB_BYTES / stride;
        let words = count.div_ceil(64) * 2;
        let (trailer, slots_offset) = Layout::new::<PriorHeader>()
            .extend(Layout::array::<Slot>(count).unwrap())
            .unwrap();
        let (trailer, bits_offset) = trailer
            .extend(Layout::array::<u64>(words).unwrap())
            .unwrap();
        let (expected, header_offset) = PAYLOAD_LAYOUT.extend(trailer).unwrap();
        for group in [index * 2, index * 2 + 1] {
            let page = Small::new(group).unwrap();
            let base = page.backing().pointer().as_ptr().addr();
            assert!(ptr::eq(page.geometry(), &GEOMETRIES[index]));
            assert_eq!(page.group(), group);
            assert_eq!(page.stride(), stride);
            assert_eq!(page.slots().len(), count);
            assert_eq!(page.bits().len(), words);
            assert_eq!(page.state().free, count);
            assert!(
                page.slots()
                    .iter()
                    .all(|slot| slot.directory == 0 && slot.descriptor.is_none())
            );
            assert!(page.bits().iter().all(|&bits| bits == 0));
            assert_eq!(page.backing().layout(), expected);
            assert_eq!(page.backing().payload_bytes(), SLAB_BYTES);
            assert_eq!(
                page.slots().as_ptr().addr() - base,
                header_offset + slots_offset
            );
            assert_eq!(
                page.bits().as_ptr().addr() - base,
                header_offset + bits_offset
            );
            assert_eq!(
                page.pointer(count - 1).as_ptr().addr() - base,
                (count - 1) * stride
            );
            assert_eq!(
                expected.size(),
                header_offset + bits_offset + words * size_of::<u64>()
            );
        }
    }
}
