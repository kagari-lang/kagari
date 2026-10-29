#![cfg(test)]
use super::{
    GraphIdentity, GraphMarker, GraphState, MarkPhase, ScanKind,
    marks::{self, Color, Domain, MarkBlocks, OwnerMark, PageMarks},
};
use crate::{
    gc::HeapObjectId,
    native::storage::scan::{ScanBudget, ScanStatus},
};

fn object(slot: usize, generation: u32) -> GraphIdentity {
    GraphIdentity::Object(HeapObjectId::new(1, slot, generation))
}

#[test]
fn physical_group_credit_requires_closed_marking_and_registers_again_on_reuse() {
    let state = GraphState::default();
    let mut marker = state.lock();
    let mut blocks = MarkBlocks::default();
    blocks.prepare(0).unwrap();
    let mark = blocks.slot(Domain::Struct, 0);
    let id = object(0, 0);
    marker.prepare_identity().unwrap();
    marker.admit_prepared(id, mark);
    assert!(marker.release_retired_group(mark).is_err());
    marker.begin_cycle().unwrap();
    marker.shade(id, mark).unwrap();
    assert!(marker.release_retired_group(mark).is_err());
    marker
        .next_owner(|group, epoch| {
            let (slot, more) = blocks.next_gray(group.block, epoch);
            Ok((slot.map(|_| id), more))
        })
        .unwrap();
    marker.finish_owner(id, mark).unwrap();
    marker.finish_roots().unwrap();
    marker.close_mark().unwrap();
    marker.retire_prepared();
    marker.release_retired_group(mark).unwrap();
    marker.release_retired_group(mark).unwrap();
    assert_eq!(marker.work_credits, 0);
    marker.prepare_identity().unwrap();
    marker.admit_prepared(object(0, 1), mark);
    assert_eq!(marker.work_credits, 1);
    assert_eq!(marker.credits, 1);
    assert!(marker.is_survivor(mark).unwrap());
}

fn admit(marker: &mut GraphMarker, id: GraphIdentity, mark: &OwnerMark) {
    marker.prepare_identity().unwrap();
    marker.admit_prepared(id, mark.as_ref());
}

#[test]
fn authoritative_phase_filters_idle_work_and_enforces_later_closure() {
    let state = GraphState::default();
    let observer = state.clone();
    let mark = OwnerMark::default();
    let mut marker = state.lock();
    admit(&mut marker, object(0, 0), &mark);
    // Holding the queue lock makes any accidental Idle slow path fail. The
    // shared phase changes immediately, not when the guard eventually unlocks.
    observer.shade(object(0, 0), mark.as_ref()).unwrap();
    assert!(observer.accessible(mark.as_ref()).unwrap());
    observer
        .with_publication(|| panic!("Idle needs no graph arguments"))
        .unwrap();
    assert!(
        observer
            .with_access(|| panic!("no condemnation outside Sweep"))
            .unwrap()
    );
    marker.begin_cycle().unwrap();
    assert_eq!(observer.phase(), MarkPhase::Roots);
    assert!(observer.shade(object(0, 0), mark.as_ref()).is_err());
    marker.finish_roots().unwrap();
    marker.close_mark().unwrap();
    assert_eq!(observer.phase(), MarkPhase::Sweep);
    drop(marker);
    assert!(!observer.accessible(mark.as_ref()).unwrap());
    assert!(observer.shade(object(0, 0), mark.as_ref()).is_err());
    state.lock().finish_sweep().unwrap();
    assert_eq!(observer.phase(), MarkPhase::Idle);
    observer.shade(object(0, 0), mark.as_ref()).unwrap();
    state.lock().fault();
    assert_eq!(observer.phase(), MarkPhase::Faulted);
    assert!(observer.shade(object(0, 0), mark.as_ref()).is_err());
    assert!(
        observer
            .with_publication(|| panic!("Faulted cannot prepare work"))
            .is_err()
    );
    assert!(
        observer.accessible(mark.as_ref()).unwrap(),
        "diagnostic access"
    );
}

#[test]
fn reserved_credits_cover_active_pending_and_newborn_work_without_reallocation() {
    let state = GraphState::default();
    let mut marker = state.lock();
    let marks: [OwnerMark; 33] = std::array::from_fn(|_| OwnerMark::default());
    for (slot, mark) in marks[..32].iter().enumerate() {
        admit(&mut marker, object(slot, 0), mark);
    }
    marker.begin_cycle().unwrap();
    marker.shade(object(0, 0), marks[0].as_ref()).unwrap();
    assert_eq!(
        marker
            .next_owner(|_, _| panic!("singleton fixture"))
            .unwrap(),
        Some(object(0, 0))
    );
    admit(&mut marker, object(32, 0), &marks[32]);
    let capacity = marker.pending.capacity();
    marker.fail_reservation = true;
    for _ in 0..3 {
        for (slot, mark) in marks.iter().enumerate() {
            marker.shade(object(slot, 0), mark.as_ref()).unwrap();
        }
    }
    assert_eq!(marker.pending.capacity(), capacity);
    assert_eq!(marker.pending.len(), 32);
    assert!(marker.prepare_identity().is_err());
    assert_eq!(marker.credits, 33);
    assert!(marker.can_retire(marks[0].as_ref()).is_err());
    assert!(marker.close_mark().is_err());
    marker.finish_roots().unwrap();
    while let Some(id) = marker
        .next_owner(|_, _| panic!("singleton fixture"))
        .unwrap()
    {
        let GraphIdentity::Object(object) = id else {
            unreachable!()
        };
        marker
            .finish_owner(id, marks[object.index()].as_ref())
            .unwrap();
    }
    marker.close_mark().unwrap();
    for mark in &marks {
        assert!(marker.is_survivor(mark.as_ref()).unwrap());
    }
}

#[test]
fn closure_rejects_resurrection_but_accepts_survivors_and_sweep_newborns() {
    let state = GraphState::default();
    let mut marker = state.lock();
    let marks: [OwnerMark; 3] = std::array::from_fn(|_| OwnerMark::default());
    admit(&mut marker, object(0, 0), &marks[0]);
    admit(&mut marker, object(1, 0), &marks[1]);
    marker.begin_cycle().unwrap();
    marker.shade(object(0, 0), marks[0].as_ref()).unwrap();
    marker.finish_roots().unwrap();
    assert_eq!(
        marker
            .next_owner(|_, _| panic!("singleton fixture"))
            .unwrap(),
        Some(object(0, 0))
    );
    marker
        .finish_owner(object(0, 0), marks[0].as_ref())
        .unwrap();
    marker.close_mark().unwrap();
    assert!(marker.shade(object(1, 0), marks[1].as_ref()).is_err());
    marker.shade(object(0, 0), marks[0].as_ref()).unwrap();
    admit(&mut marker, object(2, 0), &marks[2]);
    assert!(marker.is_survivor(marks[2].as_ref()).unwrap());
    marker.can_retire(marks[1].as_ref()).unwrap();
    marker.release_singleton(marks[1].as_ref());
    marker.retire_prepared();
    marker.finish_sweep().unwrap();
    admit(&mut marker, object(1, 1), &marks[1]);
    assert_eq!(marks[1].as_ref().color(marker.epoch), Color::White);
}

#[test]
fn moved_aliases_and_reused_directories_have_independent_marks() {
    let state = GraphState::default();
    let mut marker = state.lock();
    let moved = GraphIdentity::Moved(HeapObjectId::new(1, 0, 0));
    let moved_mark = OwnerMark::default();
    let replacement = OwnerMark::default();
    admit(&mut marker, moved, &moved_mark);
    admit(&mut marker, object(0, 1), &replacement);
    marker.begin_cycle().unwrap();
    marker.shade(moved, moved_mark.as_ref()).unwrap();
    marker.finish_roots().unwrap();
    assert_eq!(
        marker
            .next_owner(|_, _| panic!("singleton fixture"))
            .unwrap(),
        Some(moved)
    );
    marker.finish_owner(moved, moved_mark.as_ref()).unwrap();
    marker.close_mark().unwrap();
    assert!(marker.is_survivor(moved_mark.as_ref()).unwrap());
    assert!(!marker.is_survivor(replacement.as_ref()).unwrap());
}

#[test]
fn faults_and_epoch_exhaustion_never_authorize_sweep() {
    let state = GraphState::default();
    let mut marker = state.lock();
    let mark = OwnerMark::default();
    admit(&mut marker, object(0, 0), &mark);
    marker.begin_cycle().unwrap();
    marker.shade(object(0, 0), mark.as_ref()).unwrap();
    marker
        .next_owner(|_, _| panic!("singleton fixture"))
        .unwrap();
    marker.fault();
    assert!(marker.finish_owner(object(0, 0), mark.as_ref()).is_err());
    assert!(marker.close_mark().is_err());
    assert!(marker.is_survivor(mark.as_ref()).is_err());
    assert_eq!(marker.active, Some(object(0, 0)));
    let exhausted_state = GraphState::default();
    let mut exhausted = exhausted_state.lock();
    exhausted.epoch = u64::MAX;
    assert!(exhausted.begin_cycle().is_err());
    assert_eq!(exhausted.phase(), MarkPhase::Faulted);
}

#[test]
fn page_marks_reset_lazily_charge_work_and_reinitialize_reused_slots() {
    let state = GraphState::default();
    let mut marker = state.lock();
    let page = PageMarks::default();
    for index in [0, 64, 127] {
        marker.prepare_identity().unwrap();
        marker.admit_prepared(object(index, 0), page.slot(index));
    }
    marker.begin_cycle().unwrap();
    let initialized = marks::initialized_words();
    let mut short = ScanBudget::new(3);
    assert_eq!(
        marker
            .shade_scanned(object(64, 0), page.slot(64), ScanKind::Edges, &mut short)
            .unwrap(),
        ScanStatus::Pending
    );
    assert_eq!(short.spent(), 0);
    assert_eq!(
        marks::initialized_words(),
        initialized,
        "no word reset before reservation"
    );
    assert_eq!(page.slot(64).color(marker.epoch), Color::White);
    let mut exact = ScanBudget::new(4);
    assert_eq!(
        marker
            .shade_scanned(object(64, 0), page.slot(64), ScanKind::Edges, &mut exact)
            .unwrap(),
        ScanStatus::Complete
    );
    assert_eq!(exact.spent(), 4);
    assert_eq!(marks::initialized_words() - initialized, 4);
    assert_eq!(page.slot(0).color(marker.epoch), Color::White);
    assert_eq!(page.slot(127).color(marker.epoch), Color::White);
    marker.finish_roots().unwrap();
    assert_eq!(
        marker
            .next_owner(|group, epoch| {
                assert_eq!(group.block, 0);
                let (slot, more) = page.next_gray(epoch);
                Ok((slot.map(|slot| object(slot, 0)), more))
            })
            .unwrap(),
        Some(object(64, 0))
    );
    marker.finish_owner(object(64, 0), page.slot(64)).unwrap();

    marker.close_mark().unwrap();
    marker.can_retire(page.slot(64)).unwrap();
    marker.retire_prepared();
    marker.prepare_identity().unwrap();
    marker.admit_prepared(object(64, 1), page.slot(64));
    assert!(marker.is_survivor(page.slot(64)).unwrap());
    marker.finish_sweep().unwrap();
    marker.begin_cycle().unwrap();
    assert_eq!(page.slot(64).color(marker.epoch), Color::White);
    marker.shade(object(127, 0), page.slot(127)).unwrap();
    assert_eq!(page.slot(64).color(marker.epoch), Color::White);
    assert_eq!(page.slot(127).color(marker.epoch), Color::Gray);
}

#[test]
fn publication_into_queued_block_needs_no_spare_queue_position() {
    let state = GraphState::default();
    let mut marker = state.lock();
    let mut marks = MarkBlocks::default();
    for slot in [0, 1, 128, 256, 384] {
        marker.prepare_identity().unwrap();
        marks.prepare(slot).unwrap();
        marker.admit_prepared(object(slot, 0), marks.slot(Domain::Object, slot));
    }
    marker.begin_cycle().unwrap();
    for slot in [0, 128, 256, 384] {
        marker
            .shade(object(slot, 0), marks.slot(Domain::Object, slot))
            .unwrap();
    }
    assert_eq!(marker.pending.len(), marker.pending.capacity());
    let capacity = marker.pending.capacity();
    marker
        .shade(object(1, 0), marks.slot(Domain::Object, 1))
        .unwrap();
    assert_eq!(marker.pending.len(), 4);
    assert_eq!(marker.pending.capacity(), capacity);
    assert_eq!(
        marks.slot(Domain::Object, 1).color(marker.epoch),
        Color::Gray
    );
    marker.finish_roots().unwrap();
    while let Some(id) = marker
        .next_owner(|group, epoch| {
            let (slot, more) = marks.next_gray(group.block, epoch);
            Ok((slot.map(|slot| object(slot, 0)), more))
        })
        .unwrap()
    {
        let GraphIdentity::Object(owner) = id else {
            unreachable!()
        };
        marker
            .finish_owner(id, marks.slot(Domain::Object, owner.index()))
            .unwrap();
    }

    marker.close_mark().unwrap();
}

#[test]
fn grouped_gray_work_deduplicates_reopens_and_survives_block_growth() {
    let state = GraphState::default();
    let mut marker = state.lock();
    let mut marks = MarkBlocks::default();
    for slot in 0..128 {
        marker.prepare_identity().unwrap();
        marks.prepare(slot).unwrap();
        marker.admit_prepared(object(slot, 0), marks.slot(Domain::Object, slot));
    }
    marker.begin_cycle().unwrap();
    let capacity = marker.pending.capacity();
    for slot in 0..127 {
        marker
            .shade(object(slot, 0), marks.slot(Domain::Object, slot))
            .unwrap();
    }
    assert_eq!(marker.pending.len(), 1);
    assert_eq!(marker.work_credits, 1);
    assert_eq!(marker.pending.capacity(), capacity);
    let resolve = |marks: &MarkBlocks, group: super::marks::WorkGroup, epoch| {
        let (slot, more) = marks.next_gray(group.block, epoch);
        Ok((slot.map(|slot| object(slot, 0)), more))
    };
    assert_eq!(
        marker
            .next_owner(|group, epoch| resolve(&marks, group, epoch))
            .unwrap(),
        Some(object(0, 0))
    );
    // Grow the original store while its first owner is suspended. Work stores no
    // reference into the mark Vec and the one active full identity remains intact.
    for slot in 128..641 {
        marker.prepare_identity().unwrap();
        marks.prepare(slot).unwrap();
        marker.admit_prepared(object(slot, 0), marks.slot(Domain::Object, slot));
    }
    assert_eq!(marker.active, Some(object(0, 0)));
    assert_eq!(marker.work_credits, 6);
    assert_eq!(marker.pending.len(), 6);
    let capacity = marker.pending.capacity();
    marker.finish_roots().unwrap();
    while let Some(id) = marker
        .next_owner(|group, epoch| resolve(&marks, group, epoch))
        .unwrap()
    {
        let GraphIdentity::Object(id_value) = id else {
            unreachable!()
        };
        marker
            .finish_owner(id, marks.slot(Domain::Object, id_value.index()))
            .unwrap();
    }
    // A late edge into an already drained block reopens just that block. Repeated
    // restoration cannot duplicate its job or need a reservation after commit.
    marker.fail_reservation = true;
    for _ in 0..3 {
        marker
            .shade(object(127, 0), marks.slot(Domain::Object, 127))
            .unwrap();
    }
    assert_eq!(marker.pending.len(), 1);
    assert_eq!(marker.pending.capacity(), capacity);
    assert_eq!(
        marker
            .next_owner(|group, epoch| resolve(&marks, group, epoch))
            .unwrap(),
        Some(object(127, 0))
    );
    marker
        .finish_owner(object(127, 0), marks.slot(Domain::Object, 127))
        .unwrap();

    marker.close_mark().unwrap();
    for slot in 0..641 {
        assert!(
            marker
                .is_survivor(marks.slot(Domain::Object, slot))
                .unwrap()
        );
    }
}

#[test]
fn completed_sparse_sweep_releases_pending_highwater_without_per_retirement_shrinks() {
    let state = GraphState::default();
    let mut marker = state.lock();
    let marks: Vec<_> = (0..4096).map(|_| OwnerMark::default()).collect();
    for (slot, mark) in marks.iter().enumerate() {
        admit(&mut marker, object(slot, 0), mark);
    }
    let peak = marker.pending.capacity();
    marker.begin_cycle().unwrap();
    marker.finish_roots().unwrap();
    marker.close_mark().unwrap();
    for mark in &marks[4..] {
        marker.can_retire(mark.as_ref()).unwrap();
        marker.release_singleton(mark.as_ref());
        marker.retire_prepared();
    }
    assert_eq!(marker.pending.capacity(), peak);
    marker.finish_sweep().unwrap();
    assert!(marker.pending.capacity() >= 4);
    assert!(marker.pending.capacity() <= 8);
}
