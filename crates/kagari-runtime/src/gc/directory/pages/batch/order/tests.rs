use super::*;
use crate::gc::directory::{
    PayloadKind, PayloadLocation,
    descriptors::{Descriptor, Descriptors},
};
use std::{alloc::Layout, cell::Cell, sync::Arc};

thread_local! {
    static OPERATIONS: Cell<[usize; 5]> = const { Cell::new([0; 5]) };
}

pub(super) fn record(category: usize, amount: usize) {
    OPERATIONS.with(|operations| {
        let mut counts = operations.get();
        counts[category] += amount;
        operations.set(counts);
    });
}

#[test]
fn every_prefix_reserves_disjointness_work_without_reordering_fragments() {
    let descriptor = Descriptors::default()
        .admit(Arc::new(Descriptor::new(
            PayloadKind::String,
            Layout::new::<u64>(),
            size_of::<u64>(),
            false,
            |_| {},
        )))
        .unwrap();
    for count in 0..=RETIRE_LIMIT {
        // Odd multiplication permutes all 32 indices. Prefixes also remain unique;
        // shared page IDs make an incorrectly sorted prefix fail adjacency.
        for multiplier in (1..32).step_by(2) {
            let mut fragments = [None; RETIRE_LIMIT];
            for (index, fragment) in fragments[..count].iter_mut().enumerate() {
                *fragment = Some(Fragment {
                    start: PayloadLocation {
                        kind: PayloadKind::String,
                        page: 7,
                        slot: ((index * multiplier + 19) % 32 * 2) as u16,
                    },
                    descriptor,
                    count: 1,
                    small: true,
                });
            }
            let before = fragments;
            OPERATIONS.set([0; 5]);
            assert!(disjoint(&fragments[..count]));
            let counts = OPERATIONS.get();
            assert!(counts.iter().sum::<usize>() <= ordering_work(count));
            if count < 2 {
                assert_eq!(counts, [0; 5]);
            } else if counts[0] != 0 {
                let passes = usize::BITS - (count - 1).leading_zeros();
                assert_eq!(counts[0], 64);
                assert_eq!(counts[2], count * passes as usize);
                assert_eq!(counts[3], count - 1);
                if count == 32 {
                    assert!(counts[1] <= 129);
                }
                assert!((1..count).contains(&counts[4]));
            } else {
                assert_eq!(counts, [0, 0, 0, 0, count - 1]);
            }
            for (actual, original) in fragments.iter().zip(before) {
                assert_eq!(
                    actual.map(|fragment| fragment.start),
                    original.map(|f| f.start)
                );
            }
        }
        if count != 0 {
            assert!(ordering_work(count) >= ordering_work(count - 1));
        }
    }
    assert_eq!(ordering_work(2), 69);
    assert_eq!(ordering_work(17), 246);
    assert_eq!(ordering_work(32), 415);
}

#[test]
fn page_runs_and_reused_indices_preserve_pairwise_overlap_rejection() {
    let descriptor = Descriptors::default()
        .admit(Arc::new(Descriptor::new(
            PayloadKind::String,
            Layout::new::<u64>(),
            size_of::<u64>(),
            false,
            |_| {},
        )))
        .unwrap();
    for count in 2..=RETIRE_LIMIT {
        for multiplier in (1..32).step_by(2) {
            for descending in [false, true] {
                let mut fragments = [None; RETIRE_LIMIT];
                for (index, fragment) in fragments[..count].iter_mut().enumerate() {
                    let ordinal = (index * multiplier) % RETIRE_LIMIT;
                    let page = (ordinal / 4) as u32;
                    *fragment = Some(Fragment {
                        start: PayloadLocation {
                            kind: PayloadKind::String,
                            page: if descending { 7 - page } else { page },
                            slot: (ordinal % 4 * 3) as u16,
                        },
                        descriptor,
                        count: 2,
                        small: true,
                    });
                }
                for corrupt in [false, true] {
                    if corrupt {
                        // A nonadjacent revisit can overlap even though every
                        // neighboring pair describes disjoint physical pages.
                        let mut overlap = fragments[0].unwrap();
                        overlap.start.slot += 1;
                        fragments[count - 1] = Some(overlap);
                    }
                    let intervals = &fragments[..count];
                    let expected = intervals.iter().enumerate().all(|(index, left)| {
                        let left = left.unwrap();
                        intervals[index + 1..].iter().all(|right| {
                            let right = right.unwrap();
                            left.start.page != right.start.page
                                || left.start.slot as usize + left.count
                                    <= right.start.slot as usize
                                || right.start.slot as usize + right.count
                                    <= left.start.slot as usize
                        })
                    });
                    OPERATIONS.set([0; 5]);
                    assert_eq!(disjoint(intervals), expected);
                    assert!(OPERATIONS.get().iter().sum::<usize>() <= ordering_work(count));
                }
            }
        }
    }
}
