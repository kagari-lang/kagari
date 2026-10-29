use super::*;
use crate::{
    Runtime,
    gc::{directory::pages::allocation::LargeHeader, metrics::IndivisibleMeasurements},
    native::storage::scan::{ScanBudget, ScanStatus},
};
use std::mem::align_of;

fn maintain_descriptors(pages: &mut Pages) {
    let mut cursor = pages.descriptors.begin_maintenance();
    loop {
        let step = pages.descriptors.maintenance_slice(
            &mut cursor,
            &mut ScanBudget::new(4),
            Default::default(),
        );
        assert!(step.removed_structure.is_none());
        if step.status == ScanStatus::Complete {
            break;
        }
    }
}

fn bytes(size: usize, align: usize, edges: bool) -> Arc<Descriptor> {
    Arc::new(Descriptor::new(
        PayloadKind::Struct,
        Layout::from_size_align(size, align).unwrap(),
        edges,
        |_| {},
    ))
}

fn insert(pages: &mut Pages, directory: u32, descriptor: Arc<Descriptor>) -> PayloadLocation {
    let size = descriptor.layout.size();
    pages
        .insert(directory, descriptor, |pointer| {
            // SAFETY: this descriptor represents initialized raw bytes, not a Rust value.
            unsafe { pointer.as_ptr().write_bytes(0x5a, size) };
            Ok(())
        })
        .unwrap()
}

fn release(pages: &mut Pages, location: PayloadLocation) {
    let mut retired = pages.detach(location).unwrap();
    assert!(!retired.dispose());
    pages.finish(retired);
}

#[test]
fn classes_alignment_full_page_reuse_and_immediate_empty_release() {
    for align in [1, 2, 4, 8, 16, 32, 64] {
        for size in 0..=1050 {
            let layout = Layout::from_size_align(size, align).unwrap();
            let class = Pages::class(layout, false);
            if size > 1024 || align > 16 {
                assert!(class.is_none());
            } else {
                let group = class.unwrap();
                let stride = CLASSES[group / 2];
                assert!(stride >= size.max(1));
                assert_eq!(stride % align, 0);
                assert!(
                    CLASSES[..group / 2]
                        .iter()
                        .all(|&prior| prior < size.max(1) || prior % align != 0)
                );
                assert_eq!(Pages::class(layout, true), Some(group + 1));
            }
        }
    }
    #[cfg(feature = "execution-diagnostics")]
    {
        let ((backing, offset), counts) = crate::diagnostics::allocations::measure(|| {
            Backing::new(
                Layout::from_size_align(17, 64).unwrap(),
                Layout::new::<LargeHeader>(),
            )
            .unwrap()
        });
        assert_eq!(counts.requests, 2);
        assert_eq!(offset % align_of::<LargeHeader>(), 0);
        assert!(offset >= 17);
        assert_eq!(
            counts.net_bytes,
            (backing.layout().size() + Backing::control_bytes()) as i128
        );
        drop(backing);
    }
    assert_eq!(size_of::<Slot>(), 8);
    assert!(
        size_of::<LargeHeader>() + Backing::control_bytes() + size_of::<Allocation>() <= 96,
        "large control={}, backing control={}, directory record={}",
        size_of::<LargeHeader>(),
        Backing::control_bytes(),
        size_of::<Allocation>()
    );
    for stride in CLASSES {
        let mut pages = Pages::default();
        let descriptor = bytes(stride, 1, false);
        let count = SLAB_BYTES / stride;
        let mut locations = Vec::new();
        for index in 0..=count {
            locations.push(insert(&mut pages, index as u32, descriptor.clone()));
        }
        assert_eq!(pages.backing_bytes, 2 * SLAB_BYTES);
        assert_eq!(
            pages.usage().bitmaps,
            2 * count.div_ceil(64) * 2 * size_of::<u64>()
        );
        let survivor = locations[1];
        let address = pages.get(survivor).unwrap().0;
        let first = locations[0];
        release(&mut pages, first);
        assert!(pages.get(first).is_none());
        let reused = insert(&mut pages, 999, descriptor.clone());
        assert_eq!(reused, first, "newly nonfull page is immediately reusable");
        assert_eq!(pages.get(survivor).unwrap().0, address);
        assert_eq!(pages.occupied().count(), count + 1);
        for location in locations {
            release(&mut pages, location);
        }
        assert_eq!(pages.backing_bytes, 0);
        assert!(
            pages
                .allocations
                .iter()
                .all(|entry| matches!(entry, Allocation::Vacant(_)))
        );
        assert!(pages.available.iter().all(Option::is_none));
        // Reuse a page ID with a different class and alignment, not its old backing.
        let aligned = insert(&mut pages, 1000, bytes(24, 16, true));
        assert_eq!(pages.get(aligned).unwrap().0.as_ptr().addr() % 16, 0);
        release(&mut pages, aligned);
        assert_eq!(pages.backing_bytes, 0);
    }
}

#[cfg(feature = "execution-diagnostics")]
#[test]
fn coallocated_controls_have_two_allocations_and_disjoint_complete_accounting() {
    use crate::diagnostics::allocations::measure;

    let small = (0..CLASSES.len() * 2)
        .map(|group| measure(|| Allocation::Small(Small::new(group).unwrap())));
    let large = [(0, 64), (17, 64), (1025, 1), (1201, 256)]
        .into_iter()
        .map(|(size, align)| {
            measure(|| {
                Allocation::Large(
                    Large::new(Layout::from_size_align(size, align).unwrap(), 0).unwrap(),
                )
            })
        });
    for (allocation, counts) in small.chain(large) {
        assert_eq!(
            counts.requests, 2,
            "one composite region and one Arc control"
        );
        let raw_bytes = match &allocation {
            Allocation::Small(page) => page.backing().layout().size(),
            Allocation::Large(large) => large.backing().layout().size(),
            Allocation::Vacant(_) => unreachable!(),
        };
        assert_eq!(counts.requested_bytes, raw_bytes + Backing::control_bytes());
        assert_eq!(counts.net_bytes, counts.requested_bytes as i128);
        let payload = allocation.bytes();
        let mut pages = Pages::default();
        let index = pages.publish_allocation(allocation).unwrap();
        let usage = pages.usage();
        assert_eq!(
            pages.backing_bytes, payload,
            "runtime charge excludes controls"
        );
        assert_eq!(usage.slabs + usage.large, payload);
        assert_eq!(
            usage.slabs
                + usage.large
                + usage.slots
                + usage.bitmaps
                + usage.small_controls
                + usage.large_controls
                + usage.padding,
            raw_bytes
        );
        assert_eq!(usage.backing_controls, Backing::control_bytes());
        let (_, released) = measure(|| pages.release_allocation(index));
        assert_eq!(released.requests, 0);
        assert_eq!(released.net_bytes, -counts.net_bytes);
        assert_eq!(pages.backing_bytes, 0);
    }
}

#[test]
fn trailer_layout_rejects_overflow_and_mutation_does_not_borrow_payload() {
    let largest = Layout::from_size_align(isize::MAX as usize, 1).unwrap();
    assert!(Backing::new(largest, Layout::new::<LargeHeader>()).is_err());

    let mut small = Small::new(0).unwrap();
    let first = small.reserve();
    let pointer = small.pointer(first).cast::<u64>();
    // SAFETY: reserved aligned eight-byte payload; initialize before borrowing.
    unsafe { pointer.as_ptr().write(41) };
    // SAFETY: the test owns this payload exclusively. Trailer operations below
    // must not create a reference covering it, even transiently.
    let value = unsafe { &mut *pointer.as_ptr() };
    let second = small.reserve();
    small.state_mut().active.next = 7;
    small.parts_mut().slots[second].directory = 9;
    *value += 1;
    assert_eq!(*value, 42);
    assert_eq!(small.slots()[second].directory, 9);

    let mut large = Large::new(Layout::from_size_align(17, 64).unwrap(), 1).unwrap();
    let pointer = large.backing().pointer();
    assert_eq!(pointer.as_ptr().addr() % 64, 0);
    assert_eq!(
        (large.header() as *const LargeHeader).addr() % align_of::<LargeHeader>(),
        0
    );
    // SAFETY: initialize and exclusively borrow the entire payload prefix only.
    unsafe { pointer.as_ptr().write_bytes(5, 17) };
    let payload = unsafe { std::slice::from_raw_parts_mut(pointer.as_ptr(), 17) };
    large.active_mut().next = 2;
    payload[16] = 7;
    assert_eq!(payload[0], 5);
    assert_eq!(payload[16], 7);
    assert_eq!(large.header().directory, 1);
    assert_eq!(large.header().active.next, 2);
}

#[test]
fn failed_partial_byte_initialization_restores_occupancy_and_nonfull_links() {
    for (size, align, keep_survivor) in [(32, 16), (1025, 1), (17, 64)]
        .into_iter()
        .flat_map(|(size, align)| [false, true].map(|keep| (size, align, keep)))
    {
        let mut pages = Pages::default();
        let descriptor = bytes(size, align, false);
        let survivor = keep_survivor.then(|| insert(&mut pages, 0, descriptor.clone()));
        let before = pages.backing_bytes;
        let result = pages.insert(1, descriptor.clone(), |pointer| {
            // SAFETY: write a prefix before a fallible byte-codec step.
            unsafe { pointer.as_ptr().write_bytes(1, 3) };
            Err(RuntimeError::module_validation("injected encoding failure"))
        });
        assert!(result.is_err());
        assert_eq!(pages.backing_bytes, before);
        assert_eq!(pages.occupied().count(), usize::from(keep_survivor));
        let result = catch_unwind(AssertUnwindSafe(|| {
            pages.insert(1, descriptor.clone(), |_| {
                panic!("injected initializer panic")
            })
        }));
        assert!(result.is_err());
        assert_eq!(pages.backing_bytes, before);
        let good = insert(&mut pages, 2, descriptor);
        if let Some(survivor) = survivor {
            release(&mut pages, survivor);
        }
        release(&mut pages, good);
        assert_eq!(pages.backing_bytes, 0);
    }
}

#[test]
fn descriptor_ids_remain_owned_through_retirement_and_recycle_only_after_last_token() {
    let mut pages = Pages::default();
    let first = insert(&mut pages, 0, bytes(8, 8, false));
    let mut retired = pages.detach(first).unwrap();
    let Retired::Payload {
        descriptor_id: first_id,
        ..
    } = &retired
    else {
        unreachable!()
    };
    let first_id = *first_id;
    maintain_descriptors(&mut pages);
    assert!(pages.descriptors.get(first_id).is_some());
    let mut foreign = Pages::default();
    assert!(
        foreign
            .insert(0, pages.descriptors.get(first_id).unwrap().clone(), |_| {
                panic!("foreign descriptor must reject before initialization")
            })
            .is_err()
    );
    assert_eq!(foreign.backing_bytes, 0);
    let second = insert(&mut pages, 1, bytes(8, 8, false));
    let second_descriptor = pages.get(second).unwrap().1 as *const Descriptor;
    assert!(!std::ptr::eq(
        second_descriptor,
        pages.descriptors.get(first_id).unwrap().as_ref()
    ));
    assert!(!retired.dispose());
    pages.finish(retired);
    maintain_descriptors(&mut pages);
    assert!(pages.descriptors.get(first_id).is_none());
    let third = insert(&mut pages, 2, bytes(8, 8, false));
    let third_descriptor = pages.get(third).unwrap().1 as *const Descriptor;
    assert!(std::ptr::eq(
        third_descriptor,
        pages.descriptors.get(first_id).unwrap().as_ref()
    ));
    release(&mut pages, second);
    release(&mut pages, third);
    assert_eq!(pages.backing_bytes, 0);
}

#[test]
fn detached_token_retains_exact_layout_and_descriptor_after_page_owner_drops() {
    for (size, align) in [(0, 64), (1025, 1), (1200, 256)] {
        let mut pages = Pages::default();
        let descriptor = bytes(size, align, false);
        let weak = Arc::downgrade(&descriptor);
        let location = insert(&mut pages, 0, descriptor);
        assert_eq!(pages.backing_bytes, size.max(1));
        let mut retired = pages.detach(location).unwrap();
        assert!(pages.get(location).is_none());
        assert_eq!(retired.pointer().as_ptr().addr() % align, 0);
        drop(pages);
        assert!(weak.upgrade().is_some());
        if size != 0 {
            // SAFETY: token still retains initialized bytes and the exact allocation.
            assert_eq!(unsafe { retired.pointer().as_ptr().read() }, 0x5a);
        }
        assert!(!retired.dispose());
        assert!(!retired.dispose());
        drop(retired);
        assert!(weak.upgrade().is_none());
    }
}

#[test]
fn bounded_ranges_keep_unprocessed_slots_and_contain_each_destructor_panic() {
    use crate::resource::ResourceState;
    use std::sync::atomic::{AtomicUsize, Ordering};
    thread_local! {
        static RESOURCES: ResourceState = ResourceState::new(Default::default());
    }
    static DROPS: AtomicUsize = AtomicUsize::new(0);
    static QUARANTINED_DROPS: AtomicUsize = AtomicUsize::new(0);
    unsafe fn destroy(_: NonNull<u8>) {
        let count = DROPS.fetch_add(1, Ordering::SeqCst);
        if count == 3 {
            panic!("page destructor probe");
        }
        if RESOURCES.with(ResourceState::is_quarantined) {
            QUARANTINED_DROPS.fetch_add(1, Ordering::SeqCst);
        }
    }
    let mut pages = Pages::default();
    let descriptor = Arc::new(Descriptor::new(
        PayloadKind::String,
        Layout::from_size_align(24, 8).unwrap(),
        false,
        destroy,
    ));
    let weak = Arc::downgrade(&descriptor);
    let mut locations = Vec::new();
    for index in 0..42 {
        locations.push(insert(&mut pages, index, descriptor.clone()));
    }
    drop(descriptor);
    let address = pages.get(locations[32]).unwrap().0;
    let mut retired = pages.prepare_range(locations[0], 32).unwrap().detach();
    assert_eq!(pages.backing_bytes, SLAB_BYTES);
    assert_eq!(pages.occupied().count(), 10);
    assert!(
        locations[..32]
            .iter()
            .all(|&location| pages.get(location).is_none())
    );
    assert!(
        locations[32..]
            .iter()
            .all(|&location| pages.get(location).is_some())
    );
    assert_eq!(pages.get(locations[32]).unwrap().0, address);
    let runtime = Runtime::default();
    let heap = runtime.gc();
    heap.start_collection_observation().unwrap();
    heap.stats
        .borrow_mut()
        .observation
        .as_mut()
        .unwrap()
        .collecting = true;
    let clock = heap.observation_clock();
    let mut measurements = IndivisibleMeasurements::default();
    assert!(retired.dispose(Some((clock, &mut measurements)), || {
        RESOURCES.with(|resources| {
            resources.quarantine("page destructor probe");
        });
    }));
    assert_eq!(DROPS.load(Ordering::SeqCst), 32);
    assert_eq!(QUARANTINED_DROPS.load(Ordering::SeqCst), 28);
    assert_eq!(
        measurements.count, 32,
        "panicking destructor is also observed"
    );
    assert_eq!(measurements.total_items, 32);
    assert!(
        !retired.dispose(Some((clock, &mut measurements)), || panic!(
            "already disposed"
        ))
    );
    assert_eq!(measurements.count, 32, "no repeated destruction or sample");
    assert_eq!(pages.finish_range(&retired), 0);
    assert!(!retired.release());
    assert!(weak.upgrade().is_some());
    let mut retired = pages.prepare_range(locations[32], 10).unwrap().detach();
    assert_eq!(pages.occupied().count(), 0);
    assert_eq!(pages.backing_bytes, SLAB_BYTES, "committed until disposal");
    assert!(
        !retired.dispose(Some((clock, &mut measurements)), || panic!(
            "unexpected second panic"
        ))
    );
    assert_eq!(DROPS.load(Ordering::SeqCst), 42);
    assert_eq!(measurements.count, 42);
    assert_eq!(measurements.total_known_bytes, 42 * 24);
    assert_eq!(measurements.unknown_size_count, 0);
    assert_eq!(pages.finish_range(&retired), SLAB_BYTES);
    assert!(
        weak.upgrade().is_some(),
        "dispatch retained outside storage borrow"
    );
    let replacement = insert(&mut pages, 99, bytes(32, 8, false));
    assert_eq!(replacement.page, locations[0].page);
    assert!(!retired.release());
    assert!(weak.upgrade().is_none());
    assert!(pages.get(replacement).is_some());
    release(&mut pages, replacement);
    assert_eq!(pages.backing_bytes, 0);
}

#[test]
fn range_retirement_rejects_holes_mixed_descriptors_and_out_of_bounds() {
    let mut pages = Pages::default();
    let descriptor = bytes(256, 8, false);
    let first = insert(&mut pages, 0, descriptor.clone());
    assert!(pages.prepare_range(first, 2).is_none());
    assert!(pages.prepare_range(first, 0).is_none());
    assert!(pages.prepare_range(first, 33).is_none());
    for index in 1..4 {
        insert(&mut pages, index, descriptor.clone());
    }
    let mut pending = pages.detach(first).unwrap();
    assert!(pages.prepare_range(first, 4).is_none());
    pending.dispose();
    let (_, pending) = pages.finish(pending);
    pending.release();
    let replacement = insert(&mut pages, 4, bytes(256, 8, false));
    assert!(pages.prepare_range(replacement, 4).is_none());
    assert_eq!(pages.occupied().count(), 4);
}

#[test]
fn checked_range_crosses_bitmap_words_without_mutating_failed_or_abandoned_proofs() {
    for count in [8, 32] {
        let mut pages = Pages::default();
        let descriptor = bytes(8, 8, false);
        let locations: Vec<_> = (0..128)
            .map(|index| insert(&mut pages, index, descriptor.clone()))
            .collect();
        let page = locations[60].page;
        let original = pages.small(page).bits().to_vec();
        // The later bitmap word must be checked before any slot changes.
        pages.small_mut(page).parts_mut().bits[1] &= !1;
        assert!(pages.prepare_range(locations[60], count).is_none());
        assert_eq!(pages.small(page).bits()[0], original[0]);
        assert_eq!(pages.occupied().count(), 128);
        pages.small_mut(page).parts_mut().bits[1] = original[1];
        pages.small_mut(page).parts_mut().bits[3] |= 1;
        assert!(pages.prepare_range(locations[60], count).is_none());
        pages.small_mut(page).parts_mut().bits[3] = original[3];
        {
            let _abandoned = pages.prepare_range(locations[60], count).unwrap();
        }
        assert_eq!(pages.small(page).bits(), original);
        assert_eq!(pages.small(page).state().free, 0);

        let mut retired = pages.prepare_range(locations[60], count).unwrap().detach();
        // An independent retiring sibling must remain unavailable. Total detached
        // ownership still fits the 32-owner contract in both cases.
        let mut sibling = (count == 8).then(|| pages.detach(locations[100]).unwrap());
        assert_eq!(
            pages.occupied().count(),
            128 - count - usize::from(sibling.is_some())
        );
        assert!(!retired.dispose(Default::default(), || panic!("raw byte descriptor")));
        let retiring = pages.small(page).bits().to_vec();
        pages.small_mut(page).parts_mut().bits[3] &= !1;
        let invalid = pages.small(page).bits().to_vec();
        assert!(catch_unwind(AssertUnwindSafe(|| pages.finish_range(&retired))).is_err());
        assert_eq!(
            pages.small(page).bits(),
            invalid,
            "finish preflights both words"
        );
        assert_eq!(pages.small(page).state().free, 0);
        pages.small_mut(page).parts_mut().bits[3] = retiring[3];
        assert_eq!(pages.finish_range(&retired), 0);
        assert_eq!(pages.small(page).state().free, count);
        assert!(!retired.release());
        assert!(pages.get(locations[59]).is_some());
        assert!(pages.get(locations[60 + count]).is_some());
        if let Some(mut sibling) = sibling.take() {
            assert!(pages.get(locations[100]).is_none());
            assert!(!sibling.dispose());
            let (_, sibling) = pages.finish(sibling);
            assert!(!sibling.release());
            assert_eq!(pages.small(page).state().free, count + 1);
        }
        let remaining: Vec<_> = pages.occupied().map(|(_, location)| location).collect();
        for location in remaining {
            release(&mut pages, location);
        }
        assert_eq!(pages.backing_bytes, 0);
    }
}

#[test]
fn sweep_cursor_unlinks_before_reuse_and_skips_prepended_allocations() {
    let mut pages = Pages::default();
    let older = insert(&mut pages, 0, bytes(1040, 8, false));
    let descriptor = bytes(24, 8, false);
    let locations: Vec<_> = (1..=42)
        .map(|index| insert(&mut pages, index, descriptor.clone()))
        .collect();
    pages.begin_sweep();
    for directory in 1..=32 {
        assert_eq!(pages.sweep_slot().unwrap().directory, directory);
        pages.advance_sweep();
    }
    let mut retired = pages.prepare_range(locations[0], 32).unwrap().detach();
    assert!(!retired.dispose(Default::default(), || panic!("raw byte descriptor")));
    assert_eq!(pages.finish_range(&retired), 0);
    assert!(!retired.release());
    // Removal of the current page between slices repairs both page and ordinal.
    for &location in &locations[32..] {
        release(&mut pages, location);
    }
    let newborn = insert(&mut pages, 100, bytes(1100, 16, false));
    assert_eq!(newborn.page, locations[0].page);
    assert_eq!(pages.sweep_slot().unwrap().location, older);
    pages.advance_sweep();
    assert!(pages.sweep_done());
    release(&mut pages, older);
    let another = insert(&mut pages, 101, bytes(1200, 32, false));
    assert_eq!(another.page, older.page);
    assert!(
        pages.sweep_done(),
        "released index cannot revive an exhausted cursor"
    );

    pages.begin_sweep();
    // Removing an unvisited successor repairs the active link, not just the cursor.
    release(&mut pages, newborn);
    assert_eq!(pages.sweep_slot().unwrap().location, another);
    pages.advance_sweep();
    assert!(pages.sweep_done());
    release(&mut pages, another);
}

#[test]
fn multi_fragment_preflight_rejects_late_corruption_and_overlap_before_any_detach() {
    use crate::gc::directory::pages::batch::{Disposal, Fragment};
    let mut pages = Pages::default();
    let large = insert(&mut pages, 0, bytes(1056, 64, false));
    let descriptor = bytes(8, 8, false);
    let slots: Vec<_> = (1..=4)
        .map(|id| insert(&mut pages, id, descriptor.clone()))
        .collect();
    let id = |location: PayloadLocation, pages: &Pages| match &pages.allocations
        [location.page as usize]
    {
        Allocation::Small(page) => page.slots()[location.slot as usize].descriptor.unwrap(),
        Allocation::Large(large) => large.header().descriptor.unwrap(),
        _ => unreachable!(),
    };
    let first = Fragment {
        start: large,
        descriptor: id(large, &pages),
        count: 1,
        small: false,
    };
    let last = Fragment {
        start: slots[0],
        descriptor: id(slots[0], &pages),
        count: 4,
        small: true,
    };
    let fragments = [Some(first), Some(last)];
    let mut output = [const { None }; 32];
    let original = pages.small(slots[0].page).bits()[0];
    pages.small_mut(slots[0].page).parts_mut().bits[0] &= !4;
    assert!(pages.prepare_batch(&fragments, &mut output).is_none());
    assert!(output.iter().all(Option::is_none));
    assert!(pages.get(large).is_some());
    assert_eq!(pages.occupied().count(), 5);
    pages.small_mut(slots[0].page).parts_mut().bits[0] = original;
    assert!(
        pages
            .prepare_batch(&[Some(first), Some(last), Some(last)], &mut output)
            .is_none()
    );
    assert_eq!(pages.occupied().count(), 5);
    let wrong = Fragment {
        descriptor: first.descriptor,
        ..last
    };
    assert!(
        pages
            .prepare_batch(&[Some(first), Some(wrong)], &mut output)
            .is_none()
    );
    let nested = Fragment {
        start: slots[1],
        count: 2,
        ..last
    };
    let wrong_kind = Fragment {
        start: PayloadLocation {
            kind: PayloadKind::Native,
            ..last.start
        },
        ..last
    };
    let wrong_variant = Fragment {
        small: false,
        ..last
    };
    let late_invalid = Fragment {
        start: slots[3],
        count: 2,
        ..last
    };
    for invalid in [
        [Some(last), Some(first), Some(nested)],
        [Some(first), Some(last), Some(first)],
        [Some(last), Some(first), Some(wrong_kind)],
        [Some(first), Some(wrong_variant), None],
        [Some(first), Some(last), Some(late_invalid)],
        [
            Some(last),
            Some(Fragment {
                descriptor: last.descriptor,
                ..first
            }),
            None,
        ],
        [
            Some(last),
            Some(Fragment {
                start: PayloadLocation {
                    kind: PayloadKind::Native,
                    ..first.start
                },
                ..first
            }),
            None,
        ],
        [
            Some(last),
            Some(Fragment {
                start: PayloadLocation {
                    slot: 1,
                    ..first.start
                },
                ..first
            }),
            None,
        ],
    ] {
        assert!(pages.prepare_batch(&invalid, &mut output).is_none());
        assert!(output.iter().all(Option::is_none));
        assert_eq!(pages.occupied().count(), 5);
        assert_eq!(pages.small(slots[0].page).bits()[0], original);
        assert!(pages.get(large).is_some());
        assert!(slots.iter().all(|&location| pages.get(location).is_some()));
    }
    // Sorting proof ordinals must not change original token/destructor order.
    let fragments = [Some(last), Some(first)];
    pages
        .prepare_batch(&fragments, &mut output)
        .unwrap()
        .detach();
    assert!(matches!(output[0], Some(Disposal::Range(_))));
    assert!(matches!(output[1], Some(Disposal::Payload(_))));
    assert_eq!(pages.occupied().count(), 0);
    drop(pages);
    for token in output.iter_mut().flatten() {
        match token {
            Disposal::Payload(payload) => assert!(!payload.dispose()),
            Disposal::Range(range) => assert!(!range.dispose(None, || panic!("raw bytes"))),
        }
    }
}

#[test]
fn bitmap_sweep_charges_words_and_occupied_slots_not_vacant_runs() {
    for (stride, count, words) in [(24, 42, 2), (8, 128, 4)] {
        let mut pages = Pages::default();
        let descriptor = bytes(stride, 8, false);
        let locations: Vec<_> = (0..count)
            .map(|id| insert(&mut pages, id, descriptor.clone()))
            .collect();
        for &location in &locations[..count as usize - 1] {
            release(&mut pages, location);
        }
        pages.begin_sweep();
        let mut scanner = pages.scanner();
        let mut short = ScanBudget::new(words - 1);
        assert!(scanner.peek(&mut short).unwrap().is_none());
        assert_eq!(short.spent(), 0, "both planes reserved before reading");
        let mut bitmap_only = ScanBudget::new(words);
        assert!(scanner.peek(&mut bitmap_only).unwrap().is_none());
        assert_eq!(bitmap_only.spent(), words);
        // Publish verified hole-skipping progress, but not consumption of the owner.
        let cursor = scanner.cursor();
        pages.publish_sweep(cursor);
        let mut scanner = pages.scanner();
        let mut exact = ScanBudget::new(words + 1);
        let slot = scanner.peek(&mut exact).unwrap().unwrap();
        assert_eq!(slot.location, *locations.last().unwrap());
        assert_eq!(slot.directory, count - 1);
        scanner.advance();
        assert!(scanner.cursor().page.is_none());
        assert_eq!(exact.spent(), words + 1);
        release(&mut pages, slot.location);
        assert_eq!(pages.backing_bytes, 0);
    }
}

#[test]
fn bitmap_sweep_reloads_numeric_prefix_across_words_take_and_newborns() {
    let mut pages = Pages::default();
    let descriptor = bytes(8, 8, false);
    let locations: Vec<_> = (0..128)
        .map(|id| insert(&mut pages, id, descriptor.clone()))
        .collect();
    // Keep slot 64 retiring: it is neither occupied nor free and must not be read.
    let mut retiring = pages.detach(locations[64]).unwrap();
    for (ordinal, &location) in locations.iter().enumerate() {
        if ![63, 64, 65].contains(&ordinal) {
            release(&mut pages, location);
        }
    }
    pages.begin_sweep();
    let mut scanner = pages.scanner();
    let mut first = ScanBudget::new(5);
    assert_eq!(
        scanner.peek(&mut first).unwrap().unwrap().location,
        locations[63]
    );
    scanner.advance();
    let cursor = scanner.cursor();
    pages.publish_sweep(cursor);
    assert_eq!(first.spent(), 5);
    release(&mut pages, locations[63]);
    let newborn = insert(&mut pages, 128, descriptor.clone());
    assert_eq!(
        newborn.slot, 0,
        "reused prefix is outside the captured cursor"
    );
    assert!(!retiring.dispose());
    pages.finish(retiring);
    // Reuse an unvisited slot in the same page: the next slice reloads its bitmap.
    let prefix: Vec<_> = (1..=64)
        .map(|id| insert(&mut pages, 128 + id, descriptor.clone()))
        .collect();
    assert_eq!(prefix.last().unwrap().slot, 64);
    let mut scanner = pages.scanner();
    let mut second = ScanBudget::new(6);
    assert_eq!(
        scanner.peek(&mut second).unwrap().unwrap().location,
        prefix[63]
    );
    scanner.advance();
    assert_eq!(
        scanner.peek(&mut second).unwrap().unwrap().location,
        locations[65]
    );
    scanner.advance();
    assert!(scanner.cursor().page.is_none());
    assert_eq!(
        second.spent(),
        6,
        "planes loaded once within an exclusive selection"
    );
}

#[test]
fn bitmap_sweep_rejects_corrupt_planes_cardinality_tail_and_missing_descriptor() {
    let mut pages = Pages::default();
    let location = insert(&mut pages, 0, bytes(24, 8, false));
    pages.begin_sweep();
    let descriptor = pages.small(location.page).slots()[0].descriptor;
    for (occupied, reserved, free, published) in [
        (0, 0, 41, descriptor),             // Missing occupied publication bit.
        (1, 1, 40, descriptor),             // Occupied and retiring cannot overlap.
        (1 | (1 << 42), 0, 40, descriptor), // Outside the 42-slot class.
        (1, 0, 42, descriptor),             // Free count contradicts occupied bits.
        (1, 0, 41, None),                   // An authoritative occupied bit must resolve a slot.
    ] {
        let page = pages.small_mut(location.page).parts_mut();
        page.bits[0] = occupied;
        page.bits[1] = reserved;
        page.state.free = free;
        page.slots[0].descriptor = published;
        let mut budget = ScanBudget::new(3);
        assert!(pages.scanner().peek(&mut budget).is_none());
        assert!(
            !pages.sweep_done(),
            "failed preview never publishes progress"
        );
        assert!(budget.spent() <= 3);
    }
    let page = pages.small_mut(location.page).parts_mut();
    page.bits[0] = 1;
    page.bits[1] = 0;
    page.state.free = 41;
    page.slots[0].descriptor = descriptor;
    release(&mut pages, location);
    assert!(pages.sweep_done());
    assert_eq!(pages.backing_bytes, 0);
}

#[test]
fn range_unwind_fallback_retains_backing_after_owner_teardown_and_never_retries_drop() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static DROPS: AtomicUsize = AtomicUsize::new(0);
    unsafe fn destroy(_: NonNull<u8>) {
        if DROPS.fetch_add(1, Ordering::SeqCst) == 3 {
            panic!("range drop probe");
        }
    }
    let mut pages = Pages::default();
    let descriptor = Arc::new(Descriptor::new(
        PayloadKind::String,
        Layout::from_size_align(80, 8).unwrap(),
        false,
        destroy,
    ));
    let weak = Arc::downgrade(&descriptor);
    let locations: Vec<_> = (0..12)
        .map(|index| insert(&mut pages, index, descriptor.clone()))
        .collect();
    drop(descriptor);
    let mut range = pages.prepare_range(locations[0], 12).unwrap().detach();
    drop(pages);
    assert!(weak.upgrade().is_some());
    let result = catch_unwind(AssertUnwindSafe(move || {
        range.dispose(Default::default(), || {
            panic!("injected quarantine callback unwind")
        });
    }));
    assert!(result.is_err());
    assert_eq!(DROPS.load(Ordering::SeqCst), 12);
    assert!(weak.upgrade().is_none());
}
