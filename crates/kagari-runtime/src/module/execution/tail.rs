//! Scalar parallel moves prepared once from verified self-tail facts.
use crate::module::execution::{frames::CalleeLayout, layout::Location};
use kagari_gc::error::GcError;
use std::collections::HashMap;

#[derive(Debug)]
pub(crate) struct TailCall {
    pub(crate) call: usize,
    moves: Box<[Move]>,
}

#[derive(Debug)]
enum Move {
    Copy { source: usize, destination: usize },
    Save(usize),
    Restore(usize),
}

impl TailCall {
    pub(super) fn prepare(call: usize, arguments: &[Location], frame: &CalleeLayout) -> Self {
        assert_eq!(arguments.len(), frame.parameter_count);
        assert_eq!(frame.slot_layout.managed_count, 0);
        let moves = arguments
            .iter()
            .zip(&frame.parameter_slots)
            .filter_map(|(source, &home)| {
                let destination = frame.slot_layout.locations[home].operand.scalar()?;
                let source = source
                    .operand
                    .scalar()
                    .expect("verified scalar tail argument");
                (source.index() != destination.index())
                    .then_some((source.index(), destination.index()))
            })
            .collect();
        Self {
            call,
            moves: schedule(moves),
        }
    }

    /// Arguments are validated before destructive writes. All indices come from
    /// this exact sealed layout; no GC, callback or fallible step runs mid-move.
    pub(crate) fn apply(
        &self,
        payloads: &mut [u64],
        initialized: &mut [bool],
        frame: &CalleeLayout,
    ) -> Result<(), GcError> {
        if payloads.len() != frame.slot_layout.scalar_count || initialized.len() != payloads.len() {
            return Err(GcError::Representation);
        }
        let mut scratch = 0;
        for operation in &self.moves {
            match *operation {
                Move::Copy {
                    source,
                    destination,
                } => payloads[destination] = payloads[source],
                Move::Save(source) => scratch = payloads[source],
                Move::Restore(destination) => payloads[destination] = scratch,
            }
        }
        // A fresh invocation exposes only parameters as initialized. Retained
        // payload bits are inert until the new body writes their logical slots.
        initialized.fill(false);
        for &home in &frame.parameter_slots {
            if let Some(slot) = frame.slot_layout.locations[home].operand.scalar() {
                initialized[slot.index()] = true;
            }
        }
        Ok(())
    }
}

/// Destinations are unique. Drain leaves before cycles; each remaining component
/// is a simple cycle, so one scratch payload suffices. Work/storage are linear
/// in the argument count, including duplicated sources and independent cycles.
fn schedule(moves: Vec<(usize, usize)>) -> Box<[Move]> {
    let mut destinations = HashMap::new();
    let mut uses = HashMap::<usize, usize>::new();
    for (index, &(source, destination)) in moves.iter().enumerate() {
        assert!(destinations.insert(destination, index).is_none());
        *uses.entry(source).or_default() += 1;
    }
    let mut ready: Vec<_> = moves
        .iter()
        .enumerate()
        .filter_map(|(index, &(_, destination))| {
            (!uses.contains_key(&destination)).then_some(index)
        })
        .collect();
    let mut pending: Vec<_> = moves.into_iter().map(Some).collect();
    let mut result = Vec::new();
    while let Some(index) = ready.pop() {
        let (source, destination) = pending[index].take().expect("one destination per move");
        result.push(Move::Copy {
            source,
            destination,
        });
        let count = uses.get_mut(&source).expect("counted source");
        *count -= 1;
        if *count == 0
            && let Some(&next) = destinations.get(&source)
            && pending[next].is_some()
        {
            ready.push(next);
        }
    }
    for start in 0..pending.len() {
        let Some((_, saved)) = pending[start] else {
            continue;
        };
        result.push(Move::Save(saved));
        let mut current = start;
        loop {
            let (source, destination) = pending[current].take().expect("closed move cycle");
            if source == saved {
                result.push(Move::Restore(destination));
                break;
            }
            result.push(Move::Copy {
                source,
                destination,
            });
            current = destinations[&source];
        }
    }
    result.into_boxed_slice()
}
