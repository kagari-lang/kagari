use super::*;
use crate::gc::directory::{
    PayloadKind, PayloadLocation,
    descriptors::{Descriptor, Descriptors},
};
use std::{alloc::Layout, cell::Cell, sync::Arc};

thread_local! {
    static OPERATIONS: Cell<[usize; 4]> = const { Cell::new([0; 4]) };
}

pub(super) fn record(category: usize, amount: usize) {
    OPERATIONS.with(|operations| {
        let mut counts = operations.get();
        counts[category] += amount;
        operations.set(counts);
    });
}

#[test]
fn every_prefix_reserves_all_actual_sort_work_without_reordering_fragments() {
    let descriptor = Descriptors::default()
        .admit(Arc::new(Descriptor::new(
            PayloadKind::String,
            Layout::new::<u64>(),
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
            OPERATIONS.set([0; 4]);
            assert!(disjoint(&fragments[..count]));
            let counts = OPERATIONS.get();
            assert!(counts.iter().sum::<usize>() <= ordering_work(count));
            if count < 2 {
                assert_eq!(counts, [0; 4]);
            } else {
                let passes = usize::BITS - (count - 1).leading_zeros();
                assert_eq!(counts[0], 64);
                assert_eq!(counts[2], count * passes as usize);
                assert_eq!(counts[3], count - 1);
                if count == 32 {
                    assert!(counts[1] <= 129);
                }
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
    assert_eq!(ordering_work(2), 68);
    assert_eq!(ordering_work(17), 230);
    assert_eq!(ordering_work(32), 384);
}
