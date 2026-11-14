//! Rare control is Runtime-owned and survives session/frame-directory detachment.
//! It contains facts and cleanup authority, never independent managed roots.
#[cfg(test)]
mod tests;
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::storage::BufferUsage;
use crate::{
    execution::space::SpaceLease, module::state::initialization::StaticInitialization,
    objects::invocation::MethodInvocation,
};
use kagari_gc::error::GcError;
use std::{
    cell::{Ref, RefCell},
    fmt::{Debug, Formatter, Result as FmtResult},
    mem::size_of,
    num::NonZeroUsize,
};

/// A frame exclusively owns this private, non-Clone token until control cleanup.
/// Borrowed inspection cannot produce a token that could outlive slot recycling.
pub(crate) struct UncommonId(NonZeroUsize);

pub(crate) struct FrameUncommon {
    pub(super) invocation: Option<MethodInvocation>,
    pub(super) initializer: Option<StaticInitialization>,
    _space: SpaceLease,
}

impl FrameUncommon {
    pub(super) fn new(
        invocation: Option<MethodInvocation>,
        initializer: Option<StaticInitialization>,
        space: Option<SpaceLease>,
    ) -> Option<Self> {
        if invocation.is_none() && initializer.is_none() {
            return None;
        }
        Some(Self {
            invocation,
            initializer,
            _space: space.expect("uncommon frame space preflight"),
        })
    }
}

struct Entry {
    state: Option<FrameUncommon>,
    next_free: Option<NonZeroUsize>,
}

#[derive(Default)]
struct Records {
    entries: Vec<Entry>,
    free: Option<NonZeroUsize>,
}

/// Slots grow to concurrent rare-state demand, not common frame depth or total
/// calls. Freed entries retain backing only; Rust owners are detached and dropped
/// after source cleanup and after the pool loan ends.
#[derive(Default)]
pub(crate) struct UncommonFrames {
    records: RefCell<Records>,
}

impl UncommonFrames {
    pub(super) fn entry_bytes() -> usize {
        size_of::<Entry>()
    }

    /// Preflight before frame-source allocation. No callback or rare-state
    /// insertion intervenes before the corresponding infallible insert.
    pub(super) fn reserve(&self) -> Result<(), GcError> {
        let mut records = self.records.borrow_mut();
        if records.free.is_none() {
            records.entries.try_reserve(1)?;
        }
        Ok(())
    }

    pub(super) fn insert(&self, state: FrameUncommon) -> UncommonId {
        let mut records = self.records.borrow_mut();
        if let Some(id) = records.free {
            let entry = &mut records.entries[id.get() - 1];
            let next = entry.next_free.take();
            debug_assert!(entry.state.is_none());
            entry.state = Some(state);
            records.free = next;
            return UncommonId(id);
        }
        assert!(records.entries.len() < records.entries.capacity());
        records.entries.push(Entry {
            state: Some(state),
            next_free: None,
        });
        UncommonId(NonZeroUsize::new(records.entries.len()).expect("inserted uncommon frame"))
    }

    pub(super) fn get(&self, id: &UncommonId) -> Ref<'_, FrameUncommon> {
        Ref::map(self.records.borrow(), |records| {
            records.entries[id.0.get() - 1]
                .state
                .as_ref()
                .expect("frame owns uncommon control")
        })
    }

    pub(super) fn take(&self, id: UncommonId) -> FrameUncommon {
        let mut records = self.records.borrow_mut();
        let next = records.free;
        let entry = &mut records.entries[id.0.get() - 1];
        let state = entry.state.take().expect("frame owns uncommon control");
        entry.next_free = next;
        records.free = Some(id.0);
        state
    }

    #[cfg(feature = "execution-diagnostics")]
    pub(crate) fn visit_usage(&self, visit: &mut impl FnMut(BufferUsage)) {
        visit(BufferUsage::new(
            "uncommon frame control",
            &self.records.borrow().entries,
        ));
    }

    #[cfg(test)]
    pub(crate) fn live(&self) -> usize {
        self.records
            .borrow()
            .entries
            .iter()
            .filter(|entry| entry.state.is_some())
            .count()
    }

    #[cfg(test)]
    fn capacity(&self) -> usize {
        self.records.borrow().entries.capacity()
    }
}

impl Debug for UncommonFrames {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        let records = self.records.borrow();
        f.debug_struct("UncommonFrames")
            .field("slots", &records.entries.len())
            .field("capacity", &records.entries.capacity())
            .finish_non_exhaustive()
    }
}
