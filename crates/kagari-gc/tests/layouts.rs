use kagari_gc::{
    access::ScalarField,
    error::GcError,
    heap::{Heap, HeapConfig},
    object::GcObject,
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};

#[repr(C, align(64))]
struct Aligned<const SIZE: usize>([u8; SIZE]);

impl<const SIZE: usize> Aligned<SIZE> {
    // SAFETY: the byte array has no edges, indirect charge or coupled invariants.
    const BYTES: ScalarField<Self, [u8; SIZE]> =
        unsafe { ScalarField::new(|object| &object.0, |object| &mut object.0) };
}

// SAFETY: only initialized bytes, no edges or indirect resources.
unsafe impl<const SIZE: usize> GcObject for Aligned<SIZE> {
    type ScanState = ();

    const SCAN_KIND: ScanKind = ScanKind::EdgeFree;

    fn begin_scan(&self) {}

    fn scan(
        &self,
        _: &mut (),
        _: &mut ScanBudget,
        _: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        Ok(ScanStatus::Complete)
    }
}

fn check_layout<const SIZE: usize>(heap: &mut Heap) -> Result<(), GcError> {
    let root = heap.access()?.alloc_leaf(Aligned([0xa5; SIZE]))?;
    heap.collect()?;
    heap.access()?.with_object(&root, |view| {
        assert_eq!((view.object() as *const Aligned<SIZE>).addr() % 64, 0);
        assert!(view.object().0.iter().all(|byte| *byte == 0xa5));
    })?;
    heap.scope(|cx| {
        let object = cx.get(&root)?;
        cx.read(object, |value| {
            assert_eq!((value as *const Aligned<SIZE>).addr() % 64, 0);
            assert!(value.0.iter().all(|byte| *byte == 0xa5));
        });
        Aligned::BYTES.set(cx, object, [0x5a; SIZE]);
        assert_eq!(Aligned::BYTES.get(cx, object), [0x5a; SIZE]);
        Ok(())
    })?;
    heap.collect()?;
    heap.access()?.with_object(&root, |view| {
        assert_eq!(view.object().0, [0x5a; SIZE]);
    })?;
    drop(root);
    heap.collect()?;
    assert_eq!(heap.stats().backing_bytes, 0);
    Ok(())
}

#[test]
fn small_aligned_large_and_zero_sized_objects_keep_their_layout_contract() -> Result<(), GcError> {
    let mut heap = Heap::default();
    let empty = heap.access()?.alloc_leaf(())?;
    let another = heap.access()?.alloc_leaf(())?;
    heap.access()?
        .with_object(&empty, |view| assert_eq!(*view.object(), ()))?;
    heap.scope(|cx| {
        let object = cx.get(&empty)?;
        cx.read(object, |value| assert_eq!(*value, ()));
        Ok(())
    })?;
    heap.collect()?;
    assert_eq!(heap.stats().objects, 2);
    drop((empty, another));
    heap.collect()?;
    check_layout::<0>(&mut heap)?;
    check_layout::<1>(&mut heap)?;
    check_layout::<1024>(&mut heap)?;
    check_layout::<4097>(&mut heap)?;
    assert_eq!(heap.stats().objects, 0);
    Ok(())
}

#[test]
fn rejected_backing_admission_has_no_initialized_object_or_live_root() -> Result<(), GcError> {
    let mut heap = Heap::new(HeapConfig {
        max_backing_bytes: 0,
        ..HeapConfig::default()
    })?;
    assert!(matches!(
        heap.access()?.alloc_leaf(7u64),
        Err(GcError::Limit("backing bytes"))
    ));
    assert_eq!(heap.stats().objects, 0);
    assert_eq!(heap.stats().root_registrations, 0);
    assert_eq!(heap.stats().backing_bytes, 0);
    Ok(())
}
