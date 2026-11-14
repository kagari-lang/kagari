mod support;

use kagari_gc::{
    access::inspection::EdgeInspector,
    dynamic::DynamicLayoutBuilder,
    error::GcError,
    heap::{Heap, HeapConfig, Phase},
    scan::WorkBudget,
};
use std::{
    cell::Cell,
    collections::HashSet,
    ops::ControlFlow,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use support::{Node, ObjectVec};

#[test]
fn shared_scope_reads_compose_across_aliases_roots_and_dynamic_edges() -> Result<(), GcError> {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut heap = Heap::new(HeapConfig {
        max_roots: 2,
        ..HeapConfig::default()
    })?;
    let root = Node::alloc(&mut heap.access()?, 20, &drops)?;
    let pack = heap.access()?.create_roots(1)?;
    let mut builder = DynamicLayoutBuilder::new();
    let scalar = builder.scalar::<u64>();
    let edge = builder.reference();
    let layout = heap.access()?.register_dynamic(builder)?;
    let scalar = layout.scalar(scalar)?;
    let edge = layout.reference(edge)?;
    let identity = heap.scope(|cx| {
        let a = cx.get(&root)?;
        let b = Node::alloc_scoped(cx, 22, &drops)?;
        assert_ne!(format!("{a:p}"), format!("{b:p}"));
        Node::NEXT.set(cx, a, Some(b));
        let record = cx.alloc_dynamic(&layout)?;
        scalar.set(cx, record, 100)?;
        edge.set(cx, record, Some(b.erase()))?;
        cx.set_root(&pack, 0, Some(record))?;

        // Only shared access is available throughout the nested callbacks.
        let shared = &*cx;
        let sum = shared.read(a, |first| -> Result<i64, GcError> {
            let alias = shared.get(&root)?;
            assert_eq!(format!("{a:p}"), format!("{alias:p}"));
            assert!(shared.read(alias, |again| ptr::eq(first, again)));
            let next = shared
                .follow(alias, |node| {
                    assert_eq!(Node::VALUE.get(shared, alias), 20);
                    node.next_edge()
                })
                .unwrap();
            assert_eq!(
                Node::VALUE.get(shared, Node::NEXT.get(shared, alias).unwrap()),
                22
            );
            let dynamic = shared.get_root(&pack, 0)?;
            assert_eq!(scalar.get(shared, dynamic)?, 100);
            let child = shared.downcast::<Node>(edge.get(shared, dynamic)?.unwrap())?;
            assert_eq!(Node::VALUE.get(shared, child), 22);
            shared.read(next, |_| {
                Ok(Node::VALUE.get(shared, a) + Node::VALUE.get(shared, next))
            })
        })?;
        assert_eq!(sum, 42);
        // Ending the shared reads permits an ordinary write without a guard.
        Node::VALUE.set(cx, b, 23);
        assert_eq!(Node::VALUE.get(cx, b), 23);
        Ok(format!("{a:p}"))
    })?;
    assert_eq!(heap.stats().root_registrations, 2);
    heap.collect()?;
    heap.scope(|cx| {
        assert_eq!(format!("{:p}", cx.get(&root)?), identity);
        Ok(())
    })?;
    drop((root, pack));
    heap.collect()?;
    assert_eq!(drops.load(Ordering::Relaxed), 2);
    assert_eq!(heap.stats().objects, 0);
    Ok(())
}

#[test]
fn scoped_edge_inspection_preserves_marking_and_supports_closed_cancellation() -> Result<(), GcError>
{
    let drops = Arc::new(AtomicUsize::new(0));
    let mut heap = Heap::default();
    let node = Node::alloc(&mut heap.access()?, 42, &drops)?;
    let objects = ObjectVec::alloc(&mut heap.access()?, &vec![node.clone(); 2049])?;
    let mut cursor = None;
    for _ in 0..20 {
        heap.step(WorkBudget::new(32))?;
        cursor = ObjectVec::cursor(&mut heap.access()?, &objects)?;
        if cursor.is_some() {
            break;
        }
    }
    let cursor = cursor.expect("large object has a suspended collector scan");
    let before = heap.stats();
    assert_eq!(before.phase, Phase::Mark);
    heap.scope(|cx| {
        let object = cx.get(&objects)?;
        let target = cx.get(&node)?;
        let mut scratch = EdgeInspector::default();
        let mut distinct = HashSet::new();
        let mut visits = 0;
        let mut checkpoints = 0;
        let status = cx.inspect_edges(
            object.erase(),
            &mut scratch,
            |child| {
                assert!(child.ptr_eq(target));
                distinct.insert(child);
                visits += 1;
                ControlFlow::<()>::Continue(())
            },
            || {
                checkpoints += 1;
                ControlFlow::Continue(())
            },
        )?;
        assert_eq!(status, ControlFlow::Continue(()));
        assert_eq!(visits, 2049);
        assert_eq!(distinct.len(), 1);
        assert!(checkpoints > 1);
        let capacity = scratch.capacity_bytes();
        let mut visited = 0;
        let status = cx.inspect_edges(
            object,
            &mut scratch,
            |_| {
                visited += 1;
                if visited == 3 {
                    ControlFlow::Break("stop")
                } else {
                    ControlFlow::Continue(())
                }
            },
            || ControlFlow::Continue(()),
        )?;
        assert_eq!(status, ControlFlow::Break("stop"));
        assert_eq!(visited, 3);
        // An early stop does not leave a cursor/proof on the reusable workspace.
        visited = 0;
        let status = cx.inspect_edges(
            object,
            &mut scratch,
            |_| {
                visited += 1;
                ControlFlow::<()>::Continue(())
            },
            || ControlFlow::Continue(()),
        )?;
        assert_eq!(status, ControlFlow::Continue(()));
        assert_eq!(visited, 2049);
        assert_eq!(scratch.capacity_bytes(), capacity);
        Ok(())
    })?;
    let after = heap.stats();
    assert_eq!(after.phase, before.phase);
    assert_eq!(after.completed_cycles, before.completed_cycles);
    assert_eq!(after.root_registrations, before.root_registrations);
    assert_eq!(after.scan_workspace_bytes, before.scan_workspace_bytes);
    let unchanged = ObjectVec::cursor(&mut heap.access()?, &objects)?.unwrap();
    assert_eq!((unchanged.next, unchanged.end), (cursor.next, cursor.end));
    heap.collect()?;
    assert_eq!(drops.load(Ordering::Relaxed), 0);

    let mut layout = DynamicLayoutBuilder::new();
    for _ in 0..2049 {
        layout.reference();
    }
    let layout = heap.access()?.register_dynamic(layout)?;
    heap.scope(|cx| {
        let holes = cx.alloc_dynamic(&layout)?;
        let checkpoints = Cell::new(0);
        let status = cx.inspect_edges(
            holes,
            &mut EdgeInspector::default(),
            |_| panic!("all dynamic fields are empty"),
            || {
                checkpoints.set(checkpoints.get() + 1);
                if checkpoints.get() == 2 {
                    ControlFlow::Break("cancelled")
                } else {
                    ControlFlow::Continue(())
                }
            },
        )?;
        assert_eq!(status, ControlFlow::Break("cancelled"));
        assert_eq!(checkpoints.get(), 2);
        Ok(())
    })?;
    drop(objects);
    drop(node);
    heap.collect()?;
    assert_eq!(heap.stats().objects, 0);
    assert_eq!(heap.stats().root_registrations, 0);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    Ok(())
}

#[test]
fn cycles_survive_real_roots_and_die_after_the_final_lease() -> Result<(), GcError> {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut heap = Heap::default();
    let first;
    {
        let mut access = heap.access()?;
        first = Node::alloc(&mut access, 10, &drops)?;
        let second = Node::alloc(&mut access, 20, &drops)?;
        Node::set_next(&mut access, &first, Some(&second))?;
        Node::set_next(&mut access, &second, Some(&first))?;
    }
    heap.collect()?;
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    let clone = first.clone();
    drop(first);
    heap.collect()?;
    {
        let mut access = heap.access()?;
        assert_eq!(Node::value(&mut access, &clone)?, 10);
        let second = Node::next(&mut access, &clone)?.expect("cycle edge");
        assert_eq!(Node::value(&mut access, &second)?, 20);
    }
    drop(clone);
    heap.collect()?;
    assert_eq!(drops.load(Ordering::Relaxed), 2);
    assert_eq!(heap.stats().objects, 0);
    assert_eq!(heap.stats().backing_bytes, 0);
    assert_eq!(heap.stats().root_registrations, 0);
    Ok(())
}

#[test]
fn fixed_root_pack_replacement_transfers_retention() -> Result<(), GcError> {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut heap = Heap::default();
    let pack;
    {
        let mut access = heap.access()?;
        pack = access.create_roots(2)?;
        let node = Node::alloc(&mut access, 42, &drops)?;
        access.replace_root(&pack, 1, &node)?;
    }
    heap.collect()?;
    {
        let mut access = heap.access()?;
        let retained = access.root_at::<Node>(&pack, 1)?;
        assert_eq!(Node::value(&mut access, &retained)?, 42);
        assert!(matches!(
            access.root_at::<u64>(&pack, 1),
            Err(GcError::Representation)
        ));
        access.clear_root(&pack, 1)?;
    }
    heap.collect()?;
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    drop(pack);
    heap.collect()?;
    assert_eq!(heap.stats().root_registrations, 0);
    Ok(())
}

#[test]
fn foreign_and_shutdown_handles_are_rejected_without_address_access() -> Result<(), GcError> {
    let mut first = Heap::default();
    let root = first.access()?.alloc_leaf(99u64)?;
    let mut second = Heap::default();
    assert!(matches!(
        second.access()?.with_object(&root, |view| *view.object()),
        Err(GcError::ForeignHeap)
    ));
    let pack = first.access()?.create_roots(1)?;
    second.scope(|cx| {
        assert!(matches!(cx.get(&root), Err(GcError::ForeignHeap)));
        assert!(matches!(
            cx.get_root::<u64>(&pack, 0),
            Err(GcError::ForeignHeap)
        ));
        let local = cx.alloc_leaf(7u64)?;
        assert!(matches!(
            cx.set_root(&pack, 0, Some(local)),
            Err(GcError::ForeignHeap)
        ));
        assert_eq!(cx.read(local, |value| *value), 7);
        Ok(())
    })?;
    let descriptor = first.access()?.register::<u64>()?;
    first.shutdown();
    assert!(matches!(first.access(), Err(GcError::Closed)));
    // The old registration/handle remain inert after their physical heap disappears.
    drop(first);
    assert!(matches!(
        unsafe { second.access()?.alloc_with(&descriptor, |_| Ok(1u64)) },
        Err(GcError::ForeignHeap)
    ));
    drop(root);
    drop(descriptor);
    Ok(())
}

#[test]
fn owning_handles_and_the_idle_heap_can_transfer_between_threads() -> Result<(), GcError> {
    let mut heap = Heap::default();
    let root = heap.access()?.alloc_leaf(7u64)?;
    let clone = root.clone();
    std::thread::spawn(move || drop(clone))
        .join()
        .expect("release thread");
    std::thread::spawn(move || {
        assert_eq!(heap.access()?.with_object(&root, |view| *view.object())?, 7);
        drop(root);
        heap.collect()?;
        assert_eq!(heap.stats().objects, 0);
        Ok::<(), GcError>(())
    })
    .join()
    .expect("heap transfer")
}

#[test]
fn root_limit_failure_does_not_expose_or_leak_an_unrooted_object() -> Result<(), GcError> {
    let mut heap = Heap::new(HeapConfig {
        max_roots: 1,
        ..HeapConfig::default()
    })?;
    let root = heap.access()?.alloc_leaf(1u64)?;
    assert!(matches!(
        heap.access()?.alloc_leaf(2u64),
        Err(GcError::Limit("root registrations"))
    ));
    assert_eq!(heap.stats().objects, 1);
    drop(root);
    heap.collect()?;
    let replacement = heap.access()?.alloc_leaf(3u64)?;
    assert_eq!(
        heap.access()?
            .with_object(&replacement, |view| *view.object())?,
        3
    );
    Ok(())
}

#[test]
fn removed_result_outlives_its_container_and_source_roots() -> Result<(), GcError> {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut heap = Heap::default();
    let result;
    {
        let mut access = heap.access()?;
        let node = Node::alloc(&mut access, 15, &drops)?;
        let vector = ObjectVec::alloc(&mut access, &[node])?;
        result = ObjectVec::remove(&mut access, &vector, 0)?;
    }
    heap.collect()?;
    assert_eq!(Node::value(&mut heap.access()?, &result)?, 15);
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    drop(result);
    heap.collect()?;
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    Ok(())
}

#[test]
fn collect_finishes_the_carried_cycle_and_runs_a_fresh_one() -> Result<(), GcError> {
    let mut heap = Heap::default();
    let pack = heap.access()?.create_roots(128)?;
    heap.step(WorkBudget::new(8))?;
    assert_ne!(heap.stats().phase, Phase::Idle);
    let report = heap.collect()?;
    assert_eq!(report.completed_cycles, 2);
    drop(pack);
    let report = heap.collect()?;
    assert_eq!(report.completed_cycles, 1);
    Ok(())
}

#[test]
fn following_stored_edges_works_when_root_capacity_is_exhausted() -> Result<(), GcError> {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut heap = Heap::new(HeapConfig {
        max_roots: 2,
        ..HeapConfig::default()
    })?;
    let (first, second) = {
        let mut access = heap.access()?;
        let first = Node::alloc(&mut access, 10, &drops)?;
        let second = Node::alloc(&mut access, 20, &drops)?;
        Node::set_next(&mut access, &first, Some(&second))?;
        Node::set_next(&mut access, &second, Some(&first))?;
        (first, second)
    };
    drop(second);
    heap.collect()?;
    // The second node now has only the graph edge as retention. An empty pack
    // occupies the remaining registration so no traversal hop can create a Root.
    let full = heap.access()?.create_roots(0)?;
    assert_eq!(heap.stats().root_registrations, 2);
    heap.access()?.with_object(&first, |view| {
        let mut current = view.object();
        for _ in 0..100 {
            current = view.follow(current.next_edge().expect("cyclic next"));
        }
        assert!(ptr::eq(current, view.object()));
        Ok::<_, GcError>(())
    })??;
    assert_eq!(heap.stats().root_registrations, 2);
    drop((first, full));
    heap.collect()?;
    assert_eq!(drops.load(Ordering::Relaxed), 2);
    Ok(())
}

#[test]
fn following_an_edge_borrowed_from_another_heap_is_rejected() -> Result<(), GcError> {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut source = Heap::default();
    let (owner, child) = {
        let mut access = source.access()?;
        let owner = Node::alloc(&mut access, 1, &drops)?;
        let child = Node::alloc(&mut access, 2, &drops)?;
        Node::set_next(&mut access, &owner, Some(&child))?;
        (owner, child)
    };
    let mut destination = Heap::default();
    let local = Node::alloc(&mut destination.access()?, 3, &drops)?;
    source.access()?.with_object(&owner, |source_view| {
        let edge = source_view.object().next_edge().expect("source child");
        destination.access()?.with_object(&local, |view| {
            assert!(catch_unwind(AssertUnwindSafe(|| view.follow(edge))).is_err());
        })?;
        destination.scope(|cx| {
            let object = cx.get(&local)?;
            // Rejection happens before access, not after changing either heap.
            Node::VALUE.set(cx, object, 4);
            assert_eq!(Node::VALUE.get(cx, object), 4);
            Node::VALUE.set(cx, object, 3);
            Ok(())
        })
    })??;
    // Both heaps remain usable; no foreign payload reference was exposed.
    assert_eq!(Node::value(&mut source.access()?, &child)?, 2);
    assert_eq!(Node::value(&mut destination.access()?, &local)?, 3);
    Ok(())
}

#[test]
fn lending_mutation_can_revisit_a_cycle_without_registering_roots() -> Result<(), GcError> {
    let mut heap = Heap::new(HeapConfig {
        max_roots: 2,
        ..HeapConfig::default()
    })?;
    let drops = Arc::new(AtomicUsize::new(0));
    let (first, second) = {
        let mut access = heap.access()?;
        let first = Node::alloc(&mut access, 10, &drops)?;
        let second = Node::alloc(&mut access, 20, &drops)?;
        Node::set_next(&mut access, &first, Some(&second))?;
        Node::set_next(&mut access, &second, Some(&first))?;
        (first, second)
    };
    heap.scope(|cx| {
        let mut current = cx.get(&first)?;
        for _ in 0..100 {
            let next = Node::NEXT.get(cx, current).expect("cycle");
            *Node::VALUE.borrow_mut(cx, current) += 1;
            current = next;
        }
        Ok(())
    })?;
    assert_eq!(heap.stats().root_registrations, 2);
    assert_eq!(Node::value(&mut heap.access()?, &first)?, 60);
    assert_eq!(Node::value(&mut heap.access()?, &second)?, 70);
    Ok(())
}

#[test]
fn scoped_graph_edits_and_frame_slots_need_no_per_object_roots() -> Result<(), GcError> {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut heap = Heap::new(HeapConfig {
        max_roots: 1,
        ..HeapConfig::default()
    })?;
    let frame = heap.access()?.create_roots(2)?;
    heap.scope(|cx| {
        let a = Node::alloc_scoped(cx, 10, &drops)?;
        let b = Node::alloc_scoped(cx, 20, &drops)?;
        Node::NEXT.set(cx, a, Some(b));
        let child = Node::NEXT.get(cx, a).expect("child");
        Node::NEXT.set(cx, a, None);
        // The child identity survives disconnection without retaining a root or
        // borrowing its old source. Self-edges do not overlap payload loans.
        Node::VALUE.set(cx, child, 21);
        Node::NEXT.set(cx, child, Some(child));
        Node::NEXT.set(cx, a, Some(child));
        cx.set_root(&frame, 0, Some(a))?;
        cx.set_root(&frame, 1, Some(child))?;
        assert!(matches!(
            cx.retain(child),
            Err(GcError::Limit("root registrations"))
        ));
        Ok(())
    })?;
    assert_eq!(heap.stats().root_registrations, 1);
    heap.collect()?;
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    heap.scope(|cx| {
        let a = cx.get_root::<Node>(&frame, 0)?;
        let b = cx.get_root::<Node>(&frame, 1)?;
        assert!(matches!(
            cx.get_root::<u64>(&frame, 0),
            Err(GcError::Representation)
        ));
        cx.set_root::<Node>(&frame, 1, None)?;
        Node::NEXT.set(cx, a, None);
        Node::NEXT.set(cx, b, None);
        Node::VALUE.set(cx, b, 22);
        cx.set_root(&frame, 0, Some(b))?;
        Ok(())
    })?;
    heap.collect()?;
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    heap.scope(|cx| {
        let b = cx.get_root::<Node>(&frame, 0)?;
        assert_eq!(Node::VALUE.get(cx, b), 22);
        cx.set_root::<Node>(&frame, 0, None)
    })?;
    heap.collect()?;
    assert_eq!(drops.load(Ordering::Relaxed), 2);
    drop(frame);
    heap.collect()?;
    assert_eq!(heap.stats().root_registrations, 0);
    Ok(())
}

#[test]
fn scoped_allocation_with_no_root_budget_is_reclaimed_after_the_interval() -> Result<(), GcError> {
    let mut heap = Heap::new(HeapConfig {
        max_roots: 0,
        ..HeapConfig::default()
    })?;
    let result = heap.scope(|cx| {
        let value = cx.alloc_leaf(42u64)?;
        Ok(cx.read(value, |value| *value))
    })?;
    assert_eq!(result, 42);
    assert_eq!(heap.stats().root_registrations, 0);
    assert_eq!(heap.stats().objects, 1);
    heap.collect()?;
    assert_eq!(heap.stats().objects, 0);
    assert_eq!(heap.stats().backing_bytes, 0);
    Ok(())
}

#[test]
fn released_root_history_does_not_increase_empty_collection_work() -> Result<(), GcError> {
    let baseline = Heap::default().collect()?.work;
    let mut heap = Heap::default();
    let roots = (0..10000)
        .map(|_| heap.access()?.create_roots(0))
        .collect::<Result<Vec<_>, GcError>>()?;
    drop(roots);
    heap.collect()?;
    assert_eq!(heap.stats().root_registrations, 0);
    assert_eq!(heap.collect()?.work, baseline);
    // Reuse the historical slots and keep an interleaved subset alive.
    let mut roots = (0..64)
        .map(|i| heap.access()?.alloc_leaf(i as u64).map(Some))
        .collect::<Result<Vec<_>, GcError>>()?;
    for root in roots.iter_mut().step_by(2) {
        *root = None;
    }
    heap.collect()?;
    heap.collect()?;
    assert_eq!(heap.stats().objects, 32);
    for (i, root) in roots.iter().enumerate() {
        if let Some(root) = root {
            assert_eq!(
                heap.scope(|cx| {
                    let value = cx.get(root)?;
                    Ok(cx.read(value, |v| *v))
                })?,
                i as u64
            );
        }
    }
    Ok(())
}

#[test]
fn prepared_scoped_allocation_preserves_heap_and_limit_admission() -> Result<(), GcError> {
    let mut heap = Heap::new(HeapConfig {
        max_objects: 1,
        max_roots: 0,
        ..HeapConfig::default()
    })?;
    let descriptor = heap.access()?.register::<u64>()?;
    let mut other = Heap::default();
    assert!(matches!(
        other.scope(|cx| descriptor.alloc_leaf(cx, 7).map(|_| ())),
        Err(GcError::ForeignHeap)
    ));
    heap.scope(|cx| {
        let object = descriptor.alloc_leaf(cx, 42)?;
        assert_eq!(cx.read(object, |v| *v), 42);
        assert!(matches!(
            descriptor.alloc_leaf(cx, 1),
            Err(GcError::Limit(_))
        ));
        Ok(())
    })?;
    heap.collect()?;
    heap.scope(|cx| descriptor.alloc_leaf(cx, 9).map(|_| ()))?;
    assert_eq!(heap.stats().root_registrations, 0);
    assert_eq!(heap.stats().objects, 1);
    Ok(())
}
