//! Non-serializable suspension facts derived from checked executable flow.

/// Values read by execution or debugging at and after one reachable await.
/// Logical registers precede locals, matching the canonical frame layout.
#[derive(Debug, Clone)]
pub struct AwaitLiveness {
    pub(crate) instruction: usize,
    pub(crate) live: Box<[u64]>,
}

impl AwaitLiveness {
    pub fn instruction(&self) -> usize {
        self.instruction
    }

    pub fn live_slots(&self) -> impl Iterator<Item = usize> + '_ {
        self.live.iter().enumerate().flat_map(|(word, &bits)| {
            (0..64).filter_map(move |bit| (bits & (1 << bit) != 0).then_some(word * 64 + bit))
        })
    }
}

pub(crate) type ProgramSuspensions = Vec<Vec<Vec<AwaitLiveness>>>;

#[derive(Default)]
pub(crate) struct FlowBudget {
    pub work: usize,
    pub retained_bytes: usize,
}
