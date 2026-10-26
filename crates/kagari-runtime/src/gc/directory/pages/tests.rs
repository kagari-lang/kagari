use super::*;

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
        let (backing, counts) = crate::diagnostics::allocations::measure(|| {
            Backing::new(Layout::from_size_align(17, 64).unwrap()).unwrap()
        });
        assert_eq!(counts.net_bytes, (17 + Backing::control_bytes()) as i128);
        drop(backing);
    }
    assert_eq!(size_of::<Slot>(), 8);
    assert!(size_of::<Large>() + Backing::control_bytes() + size_of::<Allocation>() <= 96);
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

#[test]
fn failed_partial_byte_initialization_restores_occupancy_and_nonfull_links() {
    for keep_survivor in [false, true] {
        let mut pages = Pages::default();
        let descriptor = bytes(32, 16, false);
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
    pages.descriptors.prune();
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
    pages.descriptors.prune();
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
