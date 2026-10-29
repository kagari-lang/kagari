//! Closed-phase authority for one exclusive, noncallback physical retirement batch.
use crate::{
    error::RuntimeError,
    gc::graph::{
        GraphMarker, MarkPhase,
        marks::{Color, MarkRef},
    },
};

/// Borrows the graph lock exclusively; no phase change or admission can intervene.
/// Original storage owners still validate full identities, locations and charges.
pub(in crate::gc) struct SweepGraph<'batch, 'graph> {
    marker: &'batch mut GraphMarker<'graph>,
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
    pub(in crate::gc) fn sweep_batch(&mut self) -> Result<SweepGraph<'_, 'graph>, RuntimeError> {
        if self.phase() != MarkPhase::Sweep {
            return Err(Self::invalid("marks are not authoritative"));
        }
        Ok(SweepGraph { marker: self })
    }
}

impl<'batch, 'graph> SweepGraph<'batch, 'graph> {
    pub(in crate::gc) fn is_survivor(&self, mark: MarkRef<'_>) -> Result<bool, RuntimeError> {
        match mark.color(self.marker.epoch) {
            Color::White => Ok(false),
            Color::Black => Ok(true),
            Color::Gray => Err(GraphMarker::invalid("unfinished closed graph owner")),
        }
    }

    /// This transaction is for grouped physical identities, not singleton owners
    /// whose registration must also be removed from the work-credit ledger.
    pub(in crate::gc) fn prepare_retirement(
        self,
        count: usize,
    ) -> Result<PreparedRetirement<'batch, 'graph>, RuntimeError> {
        if count > 32 {
            return Err(GraphMarker::invalid(
                "physical retirement batch exceeds bound",
            ));
        }
        let credits = self.marker.credits.checked_sub(count).ok_or_else(|| {
            GraphMarker::invalid("physical retirement exceeds admitted graph credits")
        })?;
        self.marker.check_credits();
        Ok(PreparedRetirement {
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
    use crate::gc::graph::{
        GraphIdentity, GraphState,
        marks::{Domain, MarkBlocks},
    };

    #[test]
    fn batch_authority_rejects_unclosed_faulted_and_overdrawn_retirement() {
        let state = GraphState::default();
        let mut graph = state.lock();
        assert!(graph.sweep_batch().is_err());
        graph.begin_cycle().unwrap();
        assert!(graph.sweep_batch().is_err());
        graph.finish_roots().unwrap();
        assert!(graph.sweep_batch().is_err());
        graph.close_mark().unwrap();
        assert!(graph.sweep_batch().unwrap().prepare_retirement(1).is_err());
        assert!(graph.sweep_batch().unwrap().prepare_retirement(33).is_err());
        graph
            .sweep_batch()
            .unwrap()
            .prepare_retirement(0)
            .unwrap()
            .commit();
        assert_eq!(graph.credits, 0);
        graph.fault();
        assert!(graph.sweep_batch().is_err());
    }

    #[test]
    fn prepared_batch_commits_exact_credits_without_changing_group_capacity() {
        let state = GraphState::default();
        let mut graph = state.lock();
        let mut marks = MarkBlocks::default();
        marks.prepare(32).unwrap();
        for slot in 0..33 {
            graph.prepare_identity().unwrap();
            graph.admit_prepared(
                GraphIdentity::NativeApplication(NativeApplicationScanId::from_parts(1, slot, 0)),
                marks.slot(Domain::NativeApplication, slot),
            );
        }
        graph.begin_cycle().unwrap();
        graph.finish_roots().unwrap();
        graph.close_mark().unwrap();
        let capacity = graph.pending.capacity();
        let work_credits = graph.work_credits;
        {
            let batch = graph.sweep_batch().unwrap();
            assert!(
                !batch
                    .is_survivor(marks.slot(Domain::NativeApplication, 0))
                    .unwrap()
            );
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
        assert!(graph.sweep_batch().unwrap().prepare_retirement(2).is_err());
        let mark = marks.slot(Domain::NativeApplication, 32);
        mark.set(graph.epoch, Color::Black);
        assert!(graph.sweep_batch().unwrap().is_survivor(mark).unwrap());
        mark.set(graph.epoch, Color::Gray);
        assert!(graph.sweep_batch().unwrap().is_survivor(mark).is_err());
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
