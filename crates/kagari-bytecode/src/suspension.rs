//! Non-serializable suspension facts derived from checked executable flow.
use kagari_common::cancellation::CancellationToken;

/// Values read by execution or debugging at and after one reachable await.
/// Bits index the verified unified logical slot layout.
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

pub(crate) struct FlowBudget {
    pub cancel: CancellationToken,
    pub work: usize,
    pub retained_bytes: usize,
}

impl FlowBudget {
    pub fn new(cancel: &CancellationToken) -> Self {
        Self {
            cancel: cancel.clone(),
            work: 0,
            retained_bytes: 0,
        }
    }
}
