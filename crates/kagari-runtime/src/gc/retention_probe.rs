//! Test-only, heap-filtered accounting for one explicitly scoped retention probe.
use crate::gc::{GcHeap, HeapObjectId, storage::HeapObject};
use crate::value::Value;
use std::{cell::RefCell, collections::HashSet};

#[derive(Debug, Default)]
pub(crate) struct Counts {
    pub proofs: usize,
    pub frames: usize,
    pub walks: usize,
    pub roots: usize,
    pub objects: usize,
    pub edges: usize,
    pub distinct_objects: HashSet<HeapObjectId>,
    pub distinct_edges: HashSet<(HeapObjectId, usize)>,
}

pub(crate) enum Event {
    Proof,
    Frame,
    Walk,
    Roots(usize),
    Object(HeapObjectId),
    Edges(HeapObjectId, usize),
}

thread_local! {
    static ACTIVE: RefCell<Option<(u64, Counts)>> = const { RefCell::new(None) };
}

pub(crate) fn record(heap: &GcHeap, event: Event) {
    ACTIVE.with_borrow_mut(|active| {
        let Some((owner, counts)) = active else {
            return;
        };
        if *owner != heap.owner {
            return;
        }
        match event {
            Event::Proof => counts.proofs += 1,
            Event::Frame => counts.frames += 1,
            Event::Walk => counts.walks += 1,
            Event::Roots(n) => counts.roots += n,
            Event::Object(id) => {
                counts.objects += 1;
                counts.distinct_objects.insert(id);
            }
            Event::Edges(id, n) => {
                counts.edges += n;
                counts
                    .distinct_edges
                    .extend((0..n).map(|index| (id, index)));
            }
        }
    });
}

pub(crate) fn measure<T>(heap: &GcHeap, run: impl FnOnce() -> T) -> (T, Counts) {
    struct Reset;

    impl Drop for Reset {
        fn drop(&mut self) {
            ACTIVE.with_borrow_mut(|active| *active = None);
        }
    }

    ACTIVE.with_borrow_mut(|active| {
        assert!(active.is_none(), "nested retention measurement");
        *active = Some((heap.owner, Counts::default()));
    });
    let reset = Reset;
    let result = run();
    let counts = ACTIVE.with_borrow_mut(|active| active.take().unwrap().1);
    drop(reset);
    (result, counts)
}

/// Inject a hidden edge for graph-safety tests without weakening mutation admission.
pub(crate) fn replace_cell(heap: &GcHeap, cell: HeapObjectId, value: Value) {
    let mut objects = heap.objects.borrow_mut();
    let Some(HeapObject::Cell { value: stored, .. }) = heap.object_mut(&mut objects, cell) else {
        panic!("cell fixture")
    };
    *stored = value;
}
