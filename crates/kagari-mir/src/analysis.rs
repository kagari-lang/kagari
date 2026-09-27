//! Revision-bound execution facts, computed before a verified module is sealed.
use crate::{BlockId, LocalId, TempId};

/// A dense set of logical slots. Temporaries and locals have distinct namespaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotSet {
    pub(crate) words: Vec<u64>,
    pub(crate) temps: usize,
    pub(crate) locals: usize,
}

impl SlotSet {
    pub fn contains_temp(&self, temp: TempId) -> bool {
        temp.index() < self.temps && self.contains(temp.index())
    }

    pub fn contains_local(&self, local: LocalId) -> bool {
        local.index() < self.locals && self.contains(self.temps + local.index())
    }

    pub fn temps(&self) -> impl Iterator<Item = TempId> + '_ {
        (0..self.temps)
            .map(TempId::new)
            .filter(|&id| self.contains_temp(id))
    }

    pub fn locals(&self) -> impl Iterator<Item = LocalId> + '_ {
        (0..self.locals)
            .map(LocalId::new)
            .filter(|&id| self.contains_local(id))
    }

    fn contains(&self, index: usize) -> bool {
        self.words[index / 64] & (1 << (index % 64)) != 0
    }
}

/// Conservative boundaries at which execution can enter runtime services or
/// transfer control. Native lowering must preserve operand roots during the call,
/// and publish an allocated result before the next boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafepointKind {
    Runtime,
    ControlFlow,
}

/// Facts immediately before an instruction or terminator executes. A destination
/// defined by that operation is not yet available. Debugger reads participate in
/// liveness, so a visible heap local remains rooted even without a script read.
#[derive(Debug, Clone)]
pub struct PointAnalysis {
    pub(crate) live: SlotSet,
    pub(crate) roots: SlotSet,
    pub(crate) debug_available: SlotSet,
    pub(crate) safepoint: Option<SafepointKind>,
}

impl PointAnalysis {
    pub fn live(&self) -> &SlotSet {
        &self.live
    }
    pub fn roots(&self) -> &SlotSet {
        &self.roots
    }
    /// Definitely initialized locals whose lexical scopes contain this point.
    /// Unreachable code has no available values; temporaries are never exposed.
    pub fn debug_available(&self) -> &SlotSet {
        &self.debug_available
    }
    pub fn safepoint(&self) -> Option<SafepointKind> {
        self.safepoint
    }
}

#[derive(Debug, Clone)]
pub struct BlockAnalysis {
    pub(crate) reachable: bool,
    // Instructions in source order, followed by the terminator.
    pub(crate) points: Vec<PointAnalysis>,
}

impl BlockAnalysis {
    pub fn reachable(&self) -> bool {
        self.reachable
    }
    pub fn instruction(&self, index: usize) -> Option<&PointAnalysis> {
        self.points.get(..self.points.len() - 1)?.get(index)
    }
    pub fn terminator(&self) -> &PointAnalysis {
        self.points.last().expect("verified block has a terminator")
    }
}

#[derive(Debug, Clone)]
pub struct FunctionAnalysis {
    pub(crate) blocks: Vec<BlockAnalysis>,
}

impl FunctionAnalysis {
    pub fn block(&self, id: BlockId) -> Option<&BlockAnalysis> {
        self.blocks.get(id.index())
    }
}
