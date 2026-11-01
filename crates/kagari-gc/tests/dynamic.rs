use kagari_gc::{
    access::{EdgeField, Scope},
    dynamic::DynamicLayoutBuilder,
    error::GcError,
    heap::{Heap, HeapConfig, Phase},
    object::{AnyObject, Edge, Gc, GcObject},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus, WorkBudget},
};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

struct Anchor {
    next: Option<Edge<AnyObject>>,
    drops: Arc<AtomicUsize>,
}

impl Anchor {
    // SAFETY: fixed traced slot with no coupling to charge or other invariants.
    const NEXT: EdgeField<Self, AnyObject> =
        unsafe { EdgeField::new(|object| &object.next, |object| &mut object.next) };

    fn alloc<'gc>(cx: &mut Scope<'gc>, drops: &Arc<AtomicUsize>) -> Result<Gc<'gc, Self>, GcError> {
        // SAFETY: no initial edges, fixed charge, and later writes use checked fields.
        unsafe {
            cx.alloc_with(|_| {
                Ok(Self {
                    next: None,
                    drops: drops.clone(),
                })
            })
        }
    }
}

// SAFETY: the one optional erased edge is always visited. Cursor owns no pointers,
// writes use checked fields and Drop touches only the independent host counter.
unsafe impl GcObject for Anchor {
    type ScanState = bool;

    const SCAN_KIND: ScanKind = ScanKind::Bounded { max_work: 1 };

    fn begin_scan(&self) -> bool {
        false
    }

    fn scan(
        &self,
        done: &mut bool,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        if !*done {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            if let Some(next) = &self.next
                && unsafe { visitor.try_visit_stored(next, budget)? } == ScanStatus::Pending
            {
                return Ok(ScanStatus::Pending);
            }
            *done = true;
        }
        Ok(ScanStatus::Complete)
    }
}

impl Drop for Anchor {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::Relaxed);
    }
}

fn finish_cycle(heap: &mut Heap) -> Result<(), GcError> {
    for _ in 0..10000 {
        if heap.stats().phase == Phase::Idle {
            return Ok(());
        }
        let step = heap.step(WorkBudget::new(8))?;
        assert!(step.work <= 8);
        assert!(step.retired_owners <= 32);
    }
    panic!("finite dynamic graph did not finish");
}

#[test]
fn field_order_does_not_inflate_size_or_change_key_identity() -> Result<(), GcError> {
    let mut heap = Heap::default();
    let expected = (size_of::<i64>() + 2).div_ceil(align_of::<i64>()) * align_of::<i64>();
    for integer_index in 0..3 {
        let mut builder = DynamicLayoutBuilder::new();
        let mut integer = None;
        let mut flags = Vec::new();
        for position in 0..3 {
            if position == integer_index {
                integer = Some(builder.scalar::<i64>());
            } else {
                flags.push(builder.scalar::<bool>());
            }
        }
        let layout = heap.access()?.register_dynamic(builder)?;
        assert_eq!(layout.size(), expected);
        assert_eq!(layout.alignment(), align_of::<i64>());
        let integer = layout.scalar(integer.unwrap())?;
        let first = layout.scalar(flags.remove(0))?;
        let second = layout.scalar(flags.remove(0))?;
        heap.scope(|cx| {
            let object = cx.alloc_dynamic(&layout)?;
            integer.set(cx, object, i64::MIN)?;
            first.set(cx, object, true)?;
            second.set(cx, object, false)?;
            assert_eq!(integer.get(cx, object)?, i64::MIN);
            assert!(first.get(cx, object)?);
            assert!(!second.get(cx, object)?);
            Ok(())
        })?;
    }
    Ok(())
}

#[test]
fn scalar_domains_alignment_large_and_empty_layouts_remain_valid() -> Result<(), GcError> {
    let mut heap = Heap::default();
    let mut builder = DynamicLayoutBuilder::new();
    let byte = builder.scalar::<i8>();
    let wide = builder.scalar::<i128>();
    let flag = builder.scalar::<bool>();
    let character = builder.scalar::<char>();
    let float = builder.scalar::<f64>();
    let edge = builder.reference();
    let layout = heap.access()?.register_dynamic(builder)?;
    let byte = layout.scalar(byte)?;
    let wide = layout.scalar(wide)?;
    let flag = layout.scalar(flag)?;
    let character = layout.scalar(character)?;
    let float = layout.scalar(float)?;
    let edge = layout.reference(edge)?;
    assert_eq!(layout.alignment(), align_of::<i128>());
    assert_eq!(layout.field_count(), 6);
    let mut builder = DynamicLayoutBuilder::new();
    let mut last = None;
    for _ in 0..96 {
        last = Some(builder.scalar::<u128>());
    }
    let large = heap.access()?.register_dynamic(builder)?;
    let last = large.scalar(last.unwrap())?;
    assert!(large.size() > 1024);
    let empty = heap
        .access()?
        .register_dynamic(DynamicLayoutBuilder::new())?;
    assert_eq!(empty.size(), 0);
    let root = heap.scope(|cx| {
        let object = cx.alloc_dynamic(&layout)?;
        assert_eq!(byte.get(cx, object)?, 0);
        assert_eq!(wide.get(cx, object)?, 0);
        assert!(!flag.get(cx, object)?);
        assert_eq!(character.get(cx, object)?, '\0');
        assert_eq!(float.get(cx, object)?, 0.0);
        assert!(edge.get(cx, object)?.is_none());
        byte.set(cx, object, i8::MIN)?;
        wide.set(cx, object, i128::MIN)?;
        flag.set(cx, object, true)?;
        character.set(cx, object, '\u{10ffff}')?;
        float.set(cx, object, f64::NAN)?;
        let other = cx.alloc_dynamic(&large)?;
        last.set(cx, other, u128::MAX)?;
        edge.set(cx, object, Some(other))?;
        let _ = cx.alloc_dynamic(&empty)?;
        cx.retain(object)
    })?;
    heap.collect()?;
    assert_eq!(heap.stats().objects, 2);
    heap.scope(|cx| {
        let object = cx.get(&root)?;
        assert_eq!(byte.get(cx, object)?, i8::MIN);
        assert_eq!(wide.get(cx, object)?, i128::MIN);
        assert!(flag.get(cx, object)?);
        assert_eq!(character.get(cx, object)?, '\u{10ffff}');
        assert!(float.get(cx, object)?.is_nan());
        let other = edge.get(cx, object)?.expect("large target");
        assert_eq!(last.get(cx, other)?, u128::MAX);
        Ok(())
    })?;
    drop(root);
    heap.collect()?;
    assert_eq!(heap.stats().objects, 0);
    assert_eq!(heap.stats().backing_bytes, 0);
    Ok(())
}

#[test]
fn layout_tokens_pin_exact_identity_and_erasure_never_fabricates_a_rust_type() -> Result<(), GcError>
{
    let mut heap = Heap::default();
    let mut builder = DynamicLayoutBuilder::new();
    let first_key = builder.scalar::<u64>();
    let first_reference = builder.reference();
    let first = heap.access()?.register_dynamic(builder)?;
    let mut builder = DynamicLayoutBuilder::new();
    let second_key = builder.scalar::<u64>();
    let second_reference = builder.reference();
    let second = heap.access()?.register_dynamic(builder)?;
    // Equal field types/positions in equal-shaped schemas do not confer ownership.
    assert!(matches!(
        first.scalar(second_key.clone()),
        Err(GcError::Representation)
    ));
    assert!(matches!(
        second.scalar(first_key.clone()),
        Err(GcError::Representation)
    ));
    assert!(matches!(
        first.reference(second_reference),
        Err(GcError::Representation)
    ));
    assert!(matches!(
        second.reference(first_reference),
        Err(GcError::Representation)
    ));
    let orphan = {
        let mut discarded = DynamicLayoutBuilder::new();
        discarded.scalar::<u64>()
    };
    assert!(matches!(first.scalar(orphan), Err(GcError::Representation)));
    let field = first.scalar(first_key)?;
    let second_field = second.scalar(second_key)?;
    drop(first);
    heap.collect()?; // the field token must keep its descriptor registration alive
    let mut foreign = Heap::default();
    let mut builder = DynamicLayoutBuilder::new();
    let foreign_field = builder.scalar::<u64>();
    let foreign_layout = foreign.access()?.register_dynamic(builder)?;
    let foreign_field = foreign_layout.scalar(foreign_field)?;
    let root = heap.scope(|cx| {
        let object = cx.alloc_dynamic(&second)?;
        assert!(matches!(
            field.set(cx, object, 1),
            Err(GcError::Representation)
        ));
        assert!(matches!(
            foreign_field.borrow_mut(cx, object),
            Err(GcError::ForeignHeap)
        ));
        assert!(matches!(
            cx.alloc_dynamic(&foreign_layout),
            Err(GcError::ForeignHeap)
        ));
        assert!(matches!(
            cx.downcast::<u64>(object),
            Err(GcError::Representation)
        ));
        let rust = cx.alloc_leaf(42u64)?;
        assert!(matches!(
            second_field.get(cx, rust.erase()),
            Err(GcError::Representation)
        ));
        let restored = cx.downcast::<u64>(rust.erase())?;
        assert_eq!(cx.read(restored, |value| *value), 42);
        cx.retain(object)
    })?;
    assert_eq!(heap.stats().objects, 2); // rejected foreign allocation creates nothing
    foreign.scope(|cx| {
        assert!(matches!(cx.get(&root), Err(GcError::ForeignHeap)));
        Ok(())
    })?;
    heap.shutdown();
    assert!(matches!(
        heap.scope(|cx| cx.get(&root).map(|_| ())),
        Err(GcError::Closed)
    ));
    Ok(())
}

#[test]
fn dynamic_and_rust_objects_form_collectible_cycles_through_one_frame_pack() -> Result<(), GcError>
{
    let mut heap = Heap::new(HeapConfig {
        max_roots: 1,
        ..HeapConfig::default()
    })?;
    let drops = Arc::new(AtomicUsize::new(0));
    let mut builder = DynamicLayoutBuilder::new();
    let next = builder.reference();
    let value = builder.scalar::<i64>();
    let layout = heap.access()?.register_dynamic(builder)?;
    let next = layout.reference(next)?;
    let value = layout.scalar(value)?;
    let frame = heap.access()?.create_roots(1)?;
    heap.scope(|cx| {
        let record = cx.alloc_dynamic(&layout)?;
        let anchor = Anchor::alloc(cx, &drops)?;
        value.set(cx, record, 17)?;
        next.set(cx, record, Some(anchor.erase()))?;
        Anchor::NEXT.set(cx, anchor, Some(record));
        cx.set_root(&frame, 0, Some(record))
    })?;
    assert_eq!(heap.stats().root_registrations, 1);
    let unwound = catch_unwind(AssertUnwindSafe(|| {
        heap.scope(|cx| -> Result<(), GcError> {
            let record = cx.get_root::<AnyObject>(&frame, 0)?;
            let scalar = value.borrow_mut(cx, record)?;
            *scalar += 1;
            panic!("application dynamic scalar edit failure");
        })
    }));
    assert!(unwound.is_err());
    heap.collect()?;
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    heap.scope(|cx| {
        let record = cx.get_root::<AnyObject>(&frame, 0)?;
        let anchor = next.get(cx, record)?.expect("Rust target");
        let anchor = cx.downcast::<Anchor>(anchor)?;
        let record_again = cx
            .follow(anchor, |anchor| anchor.next.as_ref())
            .expect("dynamic target");
        assert_eq!(value.get(cx, record_again)?, 18);
        cx.set_root::<AnyObject>(&frame, 0, None)
    })?;
    heap.collect()?;
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    assert_eq!(heap.stats().objects, 0);
    assert_eq!(heap.stats().backing_bytes, 0);
    Ok(())
}

#[test]
fn writing_behind_a_dynamic_scan_cursor_publishes_the_moved_target() -> Result<(), GcError> {
    let mut heap = Heap::default();
    let drops = Arc::new(AtomicUsize::new(0));
    let mut builder = DynamicLayoutBuilder::new();
    let mut keys = (0..128).map(|_| builder.reference()).collect::<Vec<_>>();
    let layout = heap.access()?.register_dynamic(builder)?;
    let last = layout.reference(keys.pop().unwrap())?;
    let first = layout.reference(keys.remove(0))?;
    let frame = heap.access()?.create_roots(1)?;
    heap.scope(|cx| {
        let record = cx.alloc_dynamic(&layout)?;
        let target = Anchor::alloc(cx, &drops)?;
        last.set(cx, record, Some(target.erase()))?;
        cx.set_root(&frame, 0, Some(record))
    })?;
    for _ in 0..10 {
        heap.step(WorkBudget::new(8))?;
        if heap.stats().phase == Phase::Mark {
            break;
        }
    }
    assert_eq!(heap.stats().phase, Phase::Mark);
    // One additional slice guarantees the first slot is behind the cursor. The
    // total work so far cannot scan 128 slots; its last source is still unvisited.
    heap.step(WorkBudget::new(8))?;
    assert_eq!(heap.stats().phase, Phase::Mark);
    heap.scope(|cx| {
        let record = cx.get_root::<AnyObject>(&frame, 0)?;
        let target = last.get(cx, record)?.expect("unvisited tail");
        first.set(cx, record, Some(target))?;
        last.set(cx, record, None)
    })?;
    finish_cycle(&mut heap)?;
    // Fail before any following scan/dereference if publication missed the target.
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    heap.scope(|cx| {
        let record = cx.get_root::<AnyObject>(&frame, 0)?;
        let target = first.get(cx, record)?.expect("published target");
        let _ = cx.downcast::<Anchor>(target)?;
        first.set(cx, record, None)
    })?;
    heap.collect()?;
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    drop(frame);
    heap.collect()?;
    assert_eq!(heap.stats().objects, 0);
    Ok(())
}

#[test]
fn sweep_allocation_and_admission_failure_preserve_physical_ownership() -> Result<(), GcError> {
    let mut heap = Heap::default();
    let drops = Arc::new(AtomicUsize::new(0));
    let mut builder = DynamicLayoutBuilder::new();
    let next = builder.reference();
    let layout = heap.access()?.register_dynamic(builder)?;
    let next = layout.reference(next)?;
    let frame = heap.access()?.create_roots(2)?;
    heap.scope(|cx| {
        let target = Anchor::alloc(cx, &drops)?;
        cx.set_root(&frame, 0, Some(target))?;
        for _ in 0..128 {
            let _ = cx.alloc_dynamic(&layout)?;
        }
        Ok(())
    })?;
    for _ in 0..1000 {
        heap.step(WorkBudget::new(8))?;
        if heap.stats().phase == Phase::Sweep {
            break;
        }
    }
    assert_eq!(heap.stats().phase, Phase::Sweep);
    heap.scope(|cx| {
        let target = cx.get_root::<AnyObject>(&frame, 0)?;
        let record = cx.alloc_dynamic(&layout)?;
        next.set(cx, record, Some(target))?;
        cx.set_root(&frame, 1, Some(record))?;
        cx.set_root::<AnyObject>(&frame, 0, None)
    })?;
    finish_cycle(&mut heap)?;
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    heap.collect()?;
    assert_eq!(heap.stats().objects, 2);
    drop(frame);
    heap.collect()?;
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    assert_eq!(heap.stats().backing_bytes, 0);
    let mut limited = Heap::new(HeapConfig {
        max_backing_bytes: 0,
        ..HeapConfig::default()
    })?;
    let mut builder = DynamicLayoutBuilder::new();
    for _ in 0..96 {
        builder.scalar::<u128>();
    }
    let layout = limited.access()?.register_dynamic(builder)?;
    assert!(matches!(
        limited.scope(|cx| cx.alloc_dynamic(&layout).map(|_| ())),
        Err(GcError::Limit("backing bytes"))
    ));
    assert_eq!(limited.stats().objects, 0);
    assert_eq!(limited.stats().backing_bytes, 0);
    assert_eq!(limited.stats().root_registrations, 0);
    Ok(())
}

#[test]
fn retired_layout_history_does_not_increase_empty_collection_work() -> Result<(), GcError> {
    let baseline = Heap::default().collect()?.work;
    let mut heap = Heap::default();
    for _ in 0..10000 {
        drop(
            heap.access()?
                .register_dynamic(DynamicLayoutBuilder::new())?,
        );
    }
    heap.collect()?;
    assert_eq!(heap.collect()?.work, baseline);
    // Pinned layouts survive swap-removal of their unpinned neighbours.
    let layouts = (0..64)
        .map(|_| {
            let mut builder = DynamicLayoutBuilder::new();
            let key = builder.scalar::<u64>();
            let layout = heap.access()?.register_dynamic(builder)?;
            Ok((layout, key))
        })
        .collect::<Result<Vec<_>, GcError>>()?;
    let kept: Vec<_> = layouts
        .into_iter()
        .enumerate()
        .filter_map(|(i, layout)| (i % 2 == 0).then_some(layout))
        .collect();
    heap.collect()?;
    heap.scope(|cx| {
        for (layout, key) in &kept {
            let object = cx.alloc_dynamic(layout)?;
            let field = layout.scalar(key.clone())?;
            field.set(cx, object, 42)?;
            assert_eq!(field.get(cx, object)?, 42);
        }
        Ok(())
    })?;
    heap.collect()?;
    drop(kept);
    heap.collect()?;
    assert_eq!(heap.collect()?.work, baseline);
    Ok(())
}
