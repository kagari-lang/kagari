mod support;

use kagari_gc::{
    error::GcError,
    heap::{Heap, Phase},
    object::{Edge, GcObject},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus, WorkBudget},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use support::{Node, ObjectVec, VecScan};

fn finish_cycle(heap: &mut Heap) -> Result<(), GcError> {
    for _ in 0..10000 {
        if heap.stats().phase == Phase::Idle {
            return Ok(());
        }
        let step = heap.step(WorkBudget::new(16))?;
        assert!(step.work <= 16);
        assert!(step.retired_owners <= 32);
    }
    panic!("finite graph did not complete");
}

fn partial_vector(
    heap: &mut Heap,
    vector: &kagari_gc::root::Root<ObjectVec>,
) -> Result<VecScan, GcError> {
    for _ in 0..1000 {
        heap.step(WorkBudget::new(16))?;
        if let Some(state) = ObjectVec::cursor(&mut heap.access()?, vector)?
            && state.next > 2
            && state.next + 3 < state.end
        {
            return Ok(state);
        }
    }
    panic!("expected a partially scanned vector");
}

#[test]
fn ordered_removal_repairs_each_cursor_boundary_without_losing_targets() -> Result<(), GcError> {
    // Distinct boundaries, all through an external provider and actual GC steps.
    for position in ["before", "at", "after", "tail"] {
        let drops = Arc::new(AtomicUsize::new(0));
        let mut heap = Heap::default();
        let vector = {
            let mut access = heap.access()?;
            let nodes = (0..96)
                .map(|value| Node::alloc(&mut access, value, &drops))
                .collect::<Result<Vec<_>, _>>()?;
            ObjectVec::alloc(&mut access, &nodes)?
        };
        let before = partial_vector(&mut heap, &vector)?;
        let index = match position {
            "before" => 0,
            "at" => before.next,
            "after" => before.next + 1,
            "tail" => before.end - 1,
            _ => unreachable!(),
        };
        let removed = ObjectVec::remove(&mut heap.access()?, &vector, index)?;
        assert_eq!(
            ObjectVec::cursor(&mut heap.access()?, &vector)?,
            Some(VecScan {
                next: before.next - usize::from(index < before.next),
                end: before.end - 1,
            })
        );
        finish_cycle(&mut heap)?;
        // Check before dereferencing any remaining edge; a missed target must fail
        // observably here, not be deliberately accessed after premature destruction.
        assert_eq!(drops.load(Ordering::Relaxed), 0, "boundary {position}");
        {
            let mut access = heap.access()?;
            assert_eq!(Node::value(&mut access, &removed)?, index as i64);
            let values = ObjectVec::values(&mut access, &vector)?;
            let actual = values
                .iter()
                .map(|root| Node::value(&mut access, root))
                .collect::<Result<Vec<_>, _>>()?;
            let expected = (0..96)
                .filter(|value| *value != index as i64)
                .collect::<Vec<_>>();
            assert_eq!(actual, expected);
        }
        drop(removed);
        drop(vector);
        heap.collect()?;
        assert_eq!(drops.load(Ordering::Relaxed), 96);
        assert_eq!(heap.stats().backing_bytes, 0);
    }
    Ok(())
}

#[test]
fn append_and_repeated_removal_preserve_a_captured_scan_extent() -> Result<(), GcError> {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut heap = Heap::default();
    let vector = {
        let mut access = heap.access()?;
        let nodes = (0..96)
            .map(|value| Node::alloc(&mut access, value, &drops))
            .collect::<Result<Vec<_>, _>>()?;
        ObjectVec::alloc(&mut access, &nodes)?
    };
    let before = partial_vector(&mut heap, &vector)?;
    assert_eq!(heap.stats().provider_units, 96);
    {
        let mut access = heap.access()?;
        let appended = Node::alloc(&mut access, 96, &drops)?;
        ObjectVec::push(&mut access, &vector, &appended)?;
        assert_eq!(ObjectVec::cursor(&mut access, &vector)?, Some(before));
        let tail = ObjectVec::remove(&mut access, &vector, 96)?;
        assert_eq!(Node::value(&mut access, &tail)?, 96);
        assert_eq!(ObjectVec::cursor(&mut access, &vector)?, Some(before));
        for _ in 0..3 {
            drop(ObjectVec::remove(&mut access, &vector, 0)?);
        }
    }
    assert_eq!(heap.stats().provider_units, 93);
    finish_cycle(&mut heap)?;
    // Conservative publication/rooting may keep removed objects for this cycle.
    heap.collect()?;
    assert_eq!(drops.load(Ordering::Relaxed), 4);
    {
        let mut access = heap.access()?;
        let values = ObjectVec::values(&mut access, &vector)?;
        assert_eq!(values.len(), 93);
        assert_eq!(Node::value(&mut access, &values[0])?, 3);
        assert_eq!(Node::value(&mut access, values.last().expect("last"))?, 95);
    }
    drop(vector);
    heap.collect()?;
    assert_eq!(drops.load(Ordering::Relaxed), 97);
    assert_eq!(heap.stats().provider_units, 0);
    Ok(())
}

#[test]
fn replacing_a_root_slot_behind_the_cursor_publishes_the_target() -> Result<(), GcError> {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut heap = Heap::default();
    let (pack, node) = {
        let mut access = heap.access()?;
        (
            access.create_roots(96)?,
            Node::alloc(&mut access, 42, &drops)?,
        )
    };
    heap.step(WorkBudget::new(16))?;
    assert_eq!(heap.stats().phase, Phase::Roots);
    heap.scope(|cx| {
        let node = cx.get(&node)?;
        cx.set_root(&pack, 0, Some(node))
    })?;
    drop(node);
    finish_cycle(&mut heap)?;
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    {
        let mut access = heap.access()?;
        let result = access.root_at::<Node>(&pack, 0)?;
        assert_eq!(Node::value(&mut access, &result)?, 42);
    }
    drop(pack);
    heap.collect()?;
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    Ok(())
}

#[test]
fn allocation_during_sweep_survives_and_dead_pages_are_reused_safely() -> Result<(), GcError> {
    let mut heap = Heap::default();
    for _ in 0..256 {
        drop(heap.access()?.alloc_leaf(1u64)?);
    }
    for _ in 0..1000 {
        heap.step(WorkBudget::new(8))?;
        if heap.stats().phase == Phase::Sweep {
            break;
        }
    }
    assert_eq!(heap.stats().phase, Phase::Sweep);
    let root = heap.access()?.alloc_leaf(123u64)?;
    finish_cycle(&mut heap)?;
    assert_eq!(
        heap.access()?.with_object(&root, |view| *view.object())?,
        123
    );
    assert_eq!(heap.stats().objects, 1);
    drop(root);
    heap.collect()?;
    assert_eq!(heap.stats().objects, 0);
    assert_eq!(heap.stats().backing_bytes, 0);
    let root = heap.access()?.alloc_leaf(456u64)?;
    heap.collect()?;
    assert_eq!(
        heap.access()?.with_object(&root, |view| *view.object())?,
        456
    );
    Ok(())
}

#[test]
fn cycles_finish_while_new_root_releases_arrive() -> Result<(), GcError> {
    let mut heap = Heap::default();
    // Arrival exceeds the per-service retirement cap. The root-release snapshot
    // must remain finite even while later arrivals keep the live queue nonempty.
    for _ in 0..128 {
        {
            let mut access = heap.access()?;
            for value in 0..33 {
                drop(access.alloc_leaf(value as u64)?);
            }
        }
        let report = heap.step(WorkBudget::NORMAL)?;
        assert!(report.retired_owners <= 32);
    }
    assert!(heap.stats().completed_cycles >= 2);
    heap.collect()?;
    assert_eq!(heap.stats().objects, 0);
    assert_eq!(heap.stats().root_registrations, 0);
    assert_eq!(heap.stats().backing_bytes, 0);
    Ok(())
}

#[test]
fn checked_edge_write_keeps_a_target_after_its_unscanned_source_is_cleared() -> Result<(), GcError>
{
    let drops = Arc::new(AtomicUsize::new(0));
    let tail_drops = Arc::new(AtomicUsize::new(0));
    let mut heap = Heap::default();
    let original_owner = Node::alloc(&mut heap.access()?, -1, &drops)?;
    let vector = {
        let mut access = heap.access()?;
        let mut nodes = (0..95)
            .map(|value| Node::alloc(&mut access, value, &drops))
            .collect::<Result<Vec<_>, _>>()?;
        nodes.push(Node::alloc(&mut access, 95, &tail_drops)?);
        ObjectVec::alloc(&mut access, &nodes)?
    };
    // Allocate the owner before the vector (lowest slot on its page), but register
    // its surviving root last (LIFO page jobs). This makes it black before the
    // vector starts, even when both representation sizes use the same page group.
    let owner = heap.scope(|cx| {
        let owner = cx.get(&original_owner)?;
        cx.retain(owner)
    })?;
    drop(original_owner);
    partial_vector(&mut heap, &vector)?;
    heap.scope(|cx| {
        let owner = cx.get(&owner)?;
        let vector = cx.get(&vector)?;
        let tail = cx
            .follow(vector, |vector| vector.item_edge(95))
            .expect("tail");
        Node::NEXT.set(cx, owner, Some(tail));
        // SAFETY: clear changes no capacity and drops no external resources. The
        // shared mutation kernel invalidates the suspended vector scan by default.
        unsafe {
            cx.with_object_mut(vector, |vector, _| {
                vector.clear();
                Ok(())
            })?;
        }
        Ok(())
    })?;
    finish_cycle(&mut heap)?;
    // Some prefix nodes were already marked. A fresh cycle removes that floating
    // garbage; check live-target preservation before dereferencing the new edge.
    assert_eq!(tail_drops.load(Ordering::Relaxed), 0);
    heap.collect()?;
    assert_eq!(drops.load(Ordering::Relaxed), 95);
    heap.scope(|cx| {
        let owner = cx.get(&owner)?;
        let tail = Node::NEXT.get(cx, owner).expect("published tail");
        assert_eq!(Node::VALUE.get(cx, tail), 95);
        Ok(())
    })?;
    drop((owner, vector));
    heap.collect()?;
    assert_eq!(drops.load(Ordering::Relaxed), 96);
    assert_eq!(tail_drops.load(Ordering::Relaxed), 1);
    Ok(())
}

struct Pair([Edge<u64>; 2]);

// SAFETY: both initialized edges are enumerated, no indirect storage or Drop;
// the scanner consumes at most two inspections plus visitor reset work.
unsafe impl GcObject for Pair {
    type ScanState = ();
    const SCAN_KIND: ScanKind = ScanKind::Bounded { max_work: 2 };

    fn begin_scan(&self) {}

    fn scan(
        &self,
        _: &mut (),
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        for edge in &self.0 {
            assert!(budget.take(1));
            // SAFETY: both slots belong to this live same-heap scanned object.
            assert_eq!(
                unsafe { visitor.try_visit_stored(edge, budget)? },
                ScanStatus::Complete
            );
        }
        Ok(ScanStatus::Complete)
    }
}

#[test]
fn bounded_scanner_reports_a_recoverable_minimum_budget() -> Result<(), GcError> {
    let mut heap = Heap::default();
    let pair = {
        let mut access = heap.access()?;
        let a = access.alloc_leaf(1u64)?;
        let b = access.alloc_leaf(2u64)?;
        let descriptor = access.register::<Pair>()?;
        // SAFETY: both initialized fields import protected sources.
        unsafe {
            access.alloc_with(&descriptor, |mut init| {
                Ok(Pair([init.edge(&a)?, init.edge(&b)?]))
            })?
        }
    };
    let mut allowance = 8;
    let mut requested = false;
    for _ in 0..100 {
        let report = heap.step(WorkBudget::new(allowance))?;
        assert!(report.work <= allowance);
        if let Some(required) = report.required_budget {
            assert_eq!(required, 10);
            for _ in 0..3 {
                let retry = heap.step(WorkBudget::new(8))?;
                assert_eq!(retry.required_budget, Some(required));
                assert_eq!(retry.work, 0);
                assert_eq!(retry.phase, Phase::Mark);
            }
            requested = true;
            allowance = required;
        }
        if report.completed_cycles != 0 {
            break;
        }
    }
    assert!(requested);
    assert_eq!(heap.stats().completed_cycles, 1);
    assert_eq!(heap.stats().phase, Phase::Idle);
    assert_eq!(heap.stats().objects, 3);
    drop(pair);
    heap.collect()?;
    assert_eq!(heap.stats().objects, 0);
    Ok(())
}

#[test]
fn sweep_bitmap_handles_word_boundaries_holes_and_tiny_allowances() -> Result<(), GcError> {
    for allowance in [8, 16, 1024] {
        let mut heap = Heap::default();
        let roots = heap.access()?.create_roots(260)?;
        heap.scope(|cx| {
            for i in 0..260 {
                let object = cx.alloc_leaf(())?;
                cx.set_root(&roots, i, Some(object))?;
            }
            Ok(())
        })?;
        heap.collect()?;
        heap.scope(|cx| {
            for i in 0..260 {
                if ![0, 63, 64, 127, 128, 259].contains(&i) {
                    cx.set_root::<()>(&roots, i, None)?;
                }
            }
            Ok(())
        })?;
        for _ in 0..1000 {
            let step = heap.step(WorkBudget::new(allowance))?;
            assert!(step.work <= allowance && step.retired_owners <= 32);
            if step.completed_cycles != 0 {
                break;
            }
        }
        assert_eq!(heap.stats().phase, Phase::Idle);
        assert_eq!(heap.stats().objects, 6);
        heap.scope(|cx| {
            for i in [0, 63, 64, 127, 128, 259] {
                let value = cx.get_root::<()>(&roots, i)?;
                cx.read(value, |v| *v);
            }
            Ok(())
        })?;
        heap.collect()?;
        assert_eq!(heap.stats().objects, 6);
        drop(roots);
        heap.collect()?;
        assert_eq!(heap.stats().objects, 0);
    }
    Ok(())
}
