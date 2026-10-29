//! Header publication and detached ownership share the physical allocator lifetime.
use super::*;
use crate::{
    Runtime, gc::graph::marks::Color, module::layout_storage::StructStorageLayout,
    native::storage::scan::ScanBudget, test_support::layouts::layout,
};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn struct_header_survives_failed_initialization_and_detached_owner_teardown() {
    assert!(
        ObjectHeader::layout(Layout::from_size_align(isize::MAX as usize, 1).unwrap()).is_none()
    );
    for count in [0, 1, 129] {
        let mut runtime = Runtime::default();
        let names: Vec<_> = (0..count).map(|index| format!("field{index}")).collect();
        let fields: Vec<_> = names
            .iter()
            .map(|name| (name.as_str(), Ty::Builtin(BuiltinType::I64), true))
            .collect();
        let shape = layout(&mut runtime, "HeaderTrial", &fields);
        let descriptor =
            Descriptor::structure(StructStorageLayout::prepare(shape).unwrap()).unwrap();
        let weak = Arc::downgrade(&descriptor);
        let size = count * 8;
        let mut pages = Pages::default();
        let initialize = |pointer: NonNull<u8>| {
            // SAFETY: exact packed i64 payload, including the valid empty case.
            unsafe { pointer.as_ptr().write_bytes(0x5a, size) };
            Ok(())
        };
        let failed = pages.insert(u32::MAX, descriptor.clone(), |pointer| {
            initialize(pointer)?;
            Err(RuntimeError::module_validation(
                "injected struct encoding failure",
            ))
        });
        assert!(failed.is_err());
        assert_eq!(pages.occupied().count(), 0);
        assert_eq!(pages.backing_bytes, 0);
        let panicked = catch_unwind(AssertUnwindSafe(|| {
            pages.insert(u32::MAX, descriptor.clone(), |_| {
                panic!("injected initializer unwind")
            })
        }));
        assert!(panicked.is_err());
        assert_eq!(pages.backing_bytes, 0);
        let location = pages
            .insert(u32::MAX, descriptor.clone(), initialize)
            .unwrap();
        let payload = pages.get(location).unwrap().pointer;
        let reference = pages.direct_reference(location).unwrap();
        pages.direct_mark(location).unwrap().set(7, Color::Black);
        // SAFETY: the published struct has a header immediately before its payload.
        let base = unsafe { payload.sub(ObjectHeader::SIZE) };
        assert_eq!(base.as_ptr().addr() % align_of::<ObjectHeader>(), 0);
        // SAFETY: occupied allocation retained by pages, no overlapping writer.
        let header = unsafe { ObjectHeader::read(base) };
        assert_eq!(header.allocation_id, location.page);
        assert_ne!(header.descriptor_id, 0);
        pages.begin_sweep();
        for invalid in [
            ObjectHeader {
                descriptor_id: 0,
                ..header
            },
            ObjectHeader {
                allocation_id: header.allocation_id ^ 1,
                ..header
            },
        ] {
            // SAFETY: no payload or mark borrow remains; inject corruption only
            // while this test exclusively owns the live allocation, then restore it.
            unsafe { invalid.write(base) };
            assert!(pages.get(location).is_none());
            assert!(pages.resolve_reference(reference).is_none());
            assert!(
                pages
                    .scanner()
                    .peek(&mut ScanBudget::new(usize::MAX))
                    .is_none()
            );
            // SAFETY: restore the exact admitted header before teardown or reuse.
            unsafe { header.write(base) };
        }
        let scanned = pages
            .scanner()
            .peek(&mut ScanBudget::new(usize::MAX))
            .unwrap()
            .unwrap();
        assert_eq!(scanned.location, location);
        assert_eq!(scanned.payload.pointer, payload);
        assert_eq!(scanned.mark.unwrap().color(7), Color::Black);
        if size != 0 {
            // SAFETY: both bytes were initialized above and are in the live payload.
            assert_eq!(unsafe { payload.as_ptr().read() }, 0x5a);
            assert_eq!(unsafe { payload.add(size - 1).as_ptr().read() }, 0x5a);
        }
        let mut retired = pages.detach(location).unwrap();
        assert!(pages.get(location).is_none());
        assert_eq!(pages.direct_mark(location).unwrap().color(7), Color::Black);
        pages
            .insert(u32::MAX, descriptor.clone(), initialize)
            .unwrap();
        assert_eq!(pages.direct_mark(location).unwrap().color(7), Color::Black);
        drop(descriptor);
        drop(pages);
        assert!(weak.upgrade().is_some());
        assert_eq!(retired.pointer(), payload);
        // SAFETY: detached token retains the header, payload and exact descriptor;
        // owner teardown cannot reclaim or reuse this outstanding allocation.
        assert_eq!(unsafe { ObjectHeader::read(base) }, header);
        assert!(!retired.dispose());
        drop(retired);
        assert!(weak.upgrade().is_none());
    }
}

#[repr(align(64))]
struct AlignedPayload(Arc<AtomicUsize>);

impl Drop for AlignedPayload {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn builtin_header_preserves_alignment_and_detached_payload_destruction() {
    let destroyed = Arc::new(AtomicUsize::new(0));
    let mut pages = Pages::default();
    // Allocator-only fixture: no tuple view is constructed for this typed payload.
    let descriptor = pages
        .descriptors
        .builtin::<AlignedPayload>(PayloadKind::Tuple, true)
        .unwrap();
    let offset = descriptor.payload_offset();
    let weak = Arc::downgrade(&descriptor);
    let location = pages
        .insert(u32::MAX, descriptor, |pointer| {
            assert_eq!(pointer.as_ptr().addr() % align_of::<AlignedPayload>(), 0);
            // SAFETY: the descriptor reserves aligned storage for this exact type.
            unsafe {
                pointer
                    .cast::<AlignedPayload>()
                    .as_ptr()
                    .write(AlignedPayload(destroyed.clone()));
            }
            Ok(())
        })
        .unwrap();
    let payload = pages.get(location).unwrap().pointer;
    let reference = pages.direct_reference(location).unwrap();
    let (resolved, pointer, _) = pages.resolve_reference(reference).unwrap();
    assert_eq!(resolved, location);
    assert_eq!(pointer, payload);
    assert!(pages.direct_mark(location).is_some());
    // SAFETY: the live descriptor supplies the offset within this same allocation.
    let base = unsafe { payload.sub(offset) };
    // SAFETY: placement published the initialized header before occupancy.
    let header = unsafe { ObjectHeader::read(base) };
    assert_eq!(header.allocation_id, location.page);
    assert_ne!(header.descriptor_id, 0);
    pages.begin_sweep();
    // SAFETY: exclusively owned test storage, restored before retirement.
    unsafe {
        ObjectHeader {
            descriptor_id: 0,
            ..header
        }
        .write(base);
    }
    assert!(pages.get(location).is_none());
    assert!(pages.resolve_reference(reference).is_none());
    assert!(
        pages
            .scanner()
            .peek(&mut ScanBudget::new(usize::MAX))
            .is_none()
    );
    // SAFETY: restore original admitted dispatch before accessing/dropping T.
    unsafe { header.write(base) };
    let mut retired = pages.detach(location).unwrap();
    drop(pages);
    assert_eq!(destroyed.load(Ordering::SeqCst), 0);
    assert!(weak.upgrade().is_some());
    assert_eq!(retired.pointer(), payload);
    // SAFETY: the detached token retains header storage and descriptor provenance.
    assert_eq!(unsafe { ObjectHeader::read(base) }, header);
    assert!(!retired.dispose());
    assert_eq!(destroyed.load(Ordering::SeqCst), 1);
    assert!(!retired.dispose());
    assert_eq!(destroyed.load(Ordering::SeqCst), 1);
    drop(retired);
    assert!(weak.upgrade().is_none());
}

#[test]
fn direct_page_dispatch_preserves_mixed_kinds_and_rejects_foreign_live_addresses() {
    let mut runtime = Runtime::default();
    let names: Vec<_> = (0..size_of::<String>() / 4)
        .map(|index| format!("f{index}"))
        .collect();
    let fields: Vec<_> = names
        .iter()
        .map(|name| (name.as_str(), Ty::Builtin(BuiltinType::I32), true))
        .collect();
    let shape = layout(&mut runtime, "MixedPage", &fields);
    let mut pages = Pages::default();
    let structure = Descriptor::structure(StructStorageLayout::prepare(shape).unwrap()).unwrap();
    let string = pages
        .descriptors
        .builtin::<String>(PayloadKind::String, false)
        .unwrap();
    assert_eq!(
        Pages::class(structure.layout, structure.edges),
        Pages::class(string.layout, string.edges)
    );
    let text = pages
        .insert(u32::MAX, string, |pointer| {
            // SAFETY: the exact String descriptor reserves initialized typed storage.
            unsafe {
                pointer
                    .cast::<String>()
                    .as_ptr()
                    .write(String::from("retained"))
            };
            Ok(())
        })
        .unwrap();
    let record = pages
        .insert(u32::MAX, structure, |pointer| {
            // SAFETY: packed i32 fields, each valid with an all-zero encoding.
            unsafe { pointer.as_ptr().write_bytes(0, fields.len() * 4) };
            Ok(())
        })
        .unwrap();
    assert_eq!(text.page, record.page);
    assert_ne!(text.slot, record.slot);
    let mut foreign = Pages::default();
    let descriptor = foreign
        .descriptors
        .builtin::<String>(PayloadKind::String, false)
        .unwrap();
    let foreign_location = foreign
        .insert(u32::MAX, descriptor, |pointer| {
            // SAFETY: the descriptor supplies precisely one String payload.
            unsafe {
                pointer
                    .cast::<String>()
                    .as_ptr()
                    .write(String::from("foreign"))
            };
            Ok(())
        })
        .unwrap();
    let foreign_reference = foreign.direct_reference(foreign_location).unwrap();
    assert!(pages.resolve_reference(foreign_reference).is_none());
    for location in [text, record] {
        pages.direct_mark(location).unwrap().set(9, Color::Gray);
    }
    assert!(pages.direct_mark(text).unwrap().queue_group().unwrap().1);
    for (location, more) in [(text, true), (record, false)] {
        let (reference, pending) = pages.next_gray_direct(location.page as usize, 9);
        assert_eq!(pending, more);
        let reference = reference.unwrap();
        let (resolved, _, descriptor) = pages.resolve_reference(reference).unwrap();
        assert_eq!(resolved, location);
        assert_eq!(descriptor.kind, location.kind);
        pages.direct_mark(location).unwrap().set(9, Color::Black);
    }
    assert!(pages.direct_mark(text).unwrap().queue_group().unwrap().1);
    assert_eq!(pages.next_gray_direct(text.page as usize, 9), (None, false));
    pages.begin_sweep();
    for location in [text, record] {
        let slot = pages.sweep_slot().unwrap();
        assert_eq!(slot.location, location);
        assert_eq!(slot.directory, u32::MAX);
        assert_eq!(slot.mark.unwrap().color(9), Color::Black);
        pages.advance_sweep();
    }
    assert!(pages.sweep_done());
    for (owner, location) in [(&mut pages, text), (&mut foreign, foreign_location)] {
        let mut retired = owner.detach(location).unwrap();
        assert!(!retired.dispose());
        let (_, retired) = owner.finish(retired);
        assert!(!retired.release());
    }
    let mut retired = pages.detach(record).unwrap();
    assert!(!retired.dispose());
    let (_, retired) = pages.finish(retired);
    assert!(!retired.release());
    assert_eq!(pages.backing_bytes, 0);
    assert_eq!(foreign.backing_bytes, 0);
}
