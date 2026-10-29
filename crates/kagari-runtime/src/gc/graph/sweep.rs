//! Closed-phase authority for one exclusive, noncallback physical retirement batch.
use crate::gc::graph::{
    GraphMarker, MarkPhase,
    marks::{Color, PageMarks},
};

/// Borrows the graph lock exclusively; no phase change or admission can intervene.
/// Original storage owners still validate full identities, locations and charges.
pub(in crate::gc) struct SweepGraph<'batch, 'graph> {
    marker: &'batch mut GraphMarker<'graph>,
    // The uninterrupted marker borrow keeps this snapshot authoritative.
    epoch: u64,
}

/// Aggregate credit subtraction prepared before irreversible storage detachment.
/// Dropping an unconsumed proof changes nothing. Only the physical owner may commit
/// it, after retiring exactly the prepared identities and before any destructor.
#[must_use]
pub(in crate::gc) struct PreparedRetirement<'batch, 'graph> {
    marker: &'batch mut GraphMarker<'graph>,
    credits: usize,
    count: usize,
}

impl<'graph> GraphMarker<'graph> {
    /// Reject an unclosed graph; the physical transaction owns error reporting.
    pub(in crate::gc) fn sweep_batch(&mut self) -> Option<SweepGraph<'_, 'graph>> {
        if self.phase() != MarkPhase::Sweep {
            return None;
        }
        Some(SweepGraph {
            epoch: self.epoch,
            marker: self,
        })
    }
}

impl<'batch, 'graph> SweepGraph<'batch, 'graph> {
    /// Gray is invalid in this closed phase; never treat unfinished work as dead.
    pub(in crate::gc) fn is_survivor(&self, marks: &PageMarks, slot: usize) -> Option<bool> {
        match marks.color(slot, self.epoch) {
            Color::White => Some(false),
            Color::Black => Some(true),
            Color::Gray => None,
        }
    }

    /// This transaction is for grouped physical identities, not singleton owners
    /// whose registration must also be removed from the work-credit ledger.
    pub(in crate::gc) fn prepare_retirement(
        self,
        count: usize,
    ) -> Option<PreparedRetirement<'batch, 'graph>> {
        if count > 32 {
            return None;
        }
        let credits = self.marker.credits.checked_sub(count)?;
        self.marker.check_credits();
        Some(PreparedRetirement {
            marker: self.marker,
            credits,
            count,
        })
    }
}

impl PreparedRetirement<'_, '_> {
    pub(in crate::gc) fn commit(self) {
        self.marker.credits = self.credits;
        #[cfg(feature = "execution-diagnostics")]
        {
            self.marker.retirement.max_detached =
                self.marker.retirement.max_detached.max(self.count);
        }
        #[cfg(not(feature = "execution-diagnostics"))]
        let _ = self.count;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_metadata::scan_records::{NativeApplicationScanId, ScanRecordId};
    use crate::gc::graph::{GraphIdentity, GraphState};

    #[test]
    fn batch_authority_rejects_unclosed_faulted_and_overdrawn_retirement() {
        let state = GraphState::default();
        let mut graph = state.lock();
        assert!(graph.sweep_batch().is_none());
        graph.begin_cycle().unwrap();
        assert!(graph.sweep_batch().is_none());
        graph.finish_roots().unwrap();
        assert!(graph.sweep_batch().is_none());
        graph.close_mark().unwrap();
        assert!(graph.sweep_batch().unwrap().prepare_retirement(1).is_none());
        assert!(
            graph
                .sweep_batch()
                .unwrap()
                .prepare_retirement(33)
                .is_none()
        );
        graph
            .sweep_batch()
            .unwrap()
            .prepare_retirement(0)
            .unwrap()
            .commit();
        assert_eq!(graph.credits, 0);
        graph.fault();
        assert!(graph.sweep_batch().is_none());
    }

    #[test]
    fn prepared_batch_commits_exact_credits_without_changing_group_capacity() {
        let state = GraphState::default();
        let mut graph = state.lock();
        let marks = PageMarks::default();
        for slot in 0..33 {
            graph.prepare_identity().unwrap();
            graph.admit_prepared(
                GraphIdentity::NativeApplication(NativeApplicationScanId::from_parts(1, slot, 0)),
                marks.slot(slot),
            );
        }
        graph.begin_cycle().unwrap();
        graph.finish_roots().unwrap();
        graph.close_mark().unwrap();
        let capacity = graph.pending.capacity();
        let work_credits = graph.work_credits;
        {
            let batch = graph.sweep_batch().unwrap();
            assert!(!batch.is_survivor(&marks, 0).unwrap());
            let _uncommitted = batch.prepare_retirement(32).unwrap();
        }
        assert_eq!(graph.credits, 33);
        graph
            .sweep_batch()
            .unwrap()
            .prepare_retirement(32)
            .unwrap()
            .commit();
        assert_eq!(graph.credits, 1);
        assert_eq!(graph.work_credits, work_credits);
        assert_eq!(graph.pending.capacity(), capacity);
        assert!(graph.sweep_batch().unwrap().prepare_retirement(2).is_none());
        let mark = marks.slot(32);
        mark.set(graph.epoch, Color::Black);
        assert!(
            graph
                .sweep_batch()
                .unwrap()
                .is_survivor(&marks, 32)
                .unwrap()
        );
        mark.set(graph.epoch, Color::Gray);
        assert!(
            graph
                .sweep_batch()
                .unwrap()
                .is_survivor(&marks, 32)
                .is_none()
        );
        mark.set(graph.epoch, Color::White);
        graph
            .sweep_batch()
            .unwrap()
            .prepare_retirement(1)
            .unwrap()
            .commit();
        assert_eq!(graph.credits, 0);
    }
}
