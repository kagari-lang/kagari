//! Header publication and detached ownership share the physical allocator lifetime.
use super::*;
use crate::{
    Runtime, gc::graph::marks::Color, module::layout_storage::StructStorageLayout,
    test_support::layouts::layout,
};
use kagari_types::{scalar::BuiltinType, ty::Ty};

#[test]
fn struct_header_survives_failed_initialization_and_detached_owner_teardown() {
    assert!(ObjectHeader::struct_layout(usize::MAX).is_none());
    assert!(ObjectHeader::struct_layout(isize::MAX as usize).is_none());
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
        let payload = pages.get(location).unwrap().0;
        pages.struct_mark(location).set(7, Color::Black);
        // SAFETY: the published struct has a header immediately before its payload.
        let base = unsafe { payload.sub(ObjectHeader::SIZE) };
        assert_eq!(base.as_ptr().addr() % align_of::<ObjectHeader>(), 0);
        // SAFETY: occupied allocation retained by pages, no overlapping writer.
        let header = unsafe { ObjectHeader::read(base) };
        assert_eq!(header.allocation_id, location.page);
        assert_ne!(header.descriptor_id, 0);
        if size != 0 {
            // SAFETY: both bytes were initialized above and are in the live payload.
            assert_eq!(unsafe { payload.as_ptr().read() }, 0x5a);
            assert_eq!(unsafe { payload.add(size - 1).as_ptr().read() }, 0x5a);
        }
        let mut retired = pages.detach(location).unwrap();
        assert!(pages.get(location).is_none());
        assert_eq!(pages.struct_mark(location).color(7), Color::Black);
        pages
            .insert(u32::MAX, descriptor.clone(), initialize)
            .unwrap();
        assert_eq!(pages.struct_mark(location).color(7), Color::Black);
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
