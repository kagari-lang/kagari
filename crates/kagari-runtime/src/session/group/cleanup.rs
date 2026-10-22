//! Descendants retire leaf-first, without recursive group teardown or script execution.
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    gc::roots::RootedValue,
    native::binding::NativeResult,
    scheduler::WorkKey,
    session::{
        ExecutionId,
        group::{GroupRecord, GroupState},
        store::SessionId,
    },
};

impl Runtime {
    /// Quarantine prevents the parent from resuming to consume its group result.
    /// Retirement retrieves the selected fatal outcome before disposing descendants.
    pub(crate) fn group_fatal_error(&self, root: SessionId) -> Option<RuntimeError> {
        self.groups
            .borrow()
            .records
            .iter()
            .find_map(|(parent, record)| {
                let GroupState::Ready(Err(error)) = &record.state else {
                    return None;
                };
                (error.kind() == RuntimeErrorKind::EngineFault
                    && self.resources().sessions.get(*parent)?.logical_root.get() == root)
                    .then(|| error.clone())
            })
    }

    pub(crate) fn close_owned_groups(&self, parent: SessionId) -> NativeResult<()> {
        let mut failure = None;
        loop {
            // Ownership ancestry is bounded at admission. Choose the deepest
            // descendant so retiring its branches cannot recurse into another group.
            let selected = self
                .groups
                .borrow()
                .records
                .keys()
                .filter_map(|id| {
                    let mut cursor = *id;
                    for distance in 0..=self.async_limits.max_group_depth.get() {
                        if cursor == parent {
                            return Some((*id, distance));
                        }
                        cursor = self.resources().sessions.get(cursor)?.group_parent.get()?;
                    }
                    None
                })
                .max_by_key(|(_, distance)| *distance);
            let Some((id, _)) = selected else { break };
            let record = self
                .groups
                .borrow_mut()
                .records
                .remove(&id)
                .expect("selected group");
            self.scheduler.forget(WorkKey::Group(ExecutionId(id)));
            if let Err(error) = self.dispose_group(id, record) {
                failure.get_or_insert(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }

    fn dispose_group(&self, parent: SessionId, mut record: GroupRecord) -> NativeResult<()> {
        self.dependencies.detach_group(parent);
        let mut failure = None;
        if let GroupState::Running(group) = &mut record.state {
            for branch in &mut group.branches {
                if let Some(execution) = branch.take() {
                    if self.resources().sessions.get(execution.id).is_some()
                        && let Err(error) = self.finish_owned_execution(&execution)
                    {
                        failure.get_or_insert(error);
                    }
                    if let Err(error) = self.retirement_cleanup(|| {
                        drop(execution);
                        Ok(())
                    }) {
                        failure.get_or_insert(error);
                    }
                }
            }
        }
        if let Err(error) = self.retirement_cleanup(|| {
            drop(record);
            Ok(())
        }) {
            failure.get_or_insert(error);
        }
        failure.map_or(Ok(()), Err)
    }

    pub(super) fn finish_group_record(
        &self,
        parent: SessionId,
        record: GroupRecord,
        mut outcome: NativeResult<RootedValue>,
    ) -> NativeResult<()> {
        // Detach the record before callbacks. Retain only the selected rooted
        // result and parent wake through Closing; everything else is local cleanup.
        let selected_fatal = outcome
            .as_ref()
            .is_err_and(|error| error.kind() == RuntimeErrorKind::EngineFault);
        let trace = outcome
            .as_ref()
            .err()
            .and_then(|error| error.trace().cloned());
        let retain_trace = |error: RuntimeError| match &trace {
            Some(trace) => error.with_trace(trace.clone()),
            None => error,
        };
        let callback_fault = self.scheduler.hub.check().is_err();
        if selected_fatal {
            self.quarantine_execution_invariant("structured branch failed");
        } else if let Err(error) = self.poll_group_parent(parent) {
            outcome = Err(retain_trace(error));
        }
        let wake = record.wake.clone();
        let descendants = self.close_owned_groups(parent);
        let cleanup = self.dispose_group(parent, record);
        self.scheduler.forget(WorkKey::Group(ExecutionId(parent)));
        // Actual cleanup failures take precedence. Health checks merely observing
        // the selected fatal's quarantine must not replace that failure or origin.
        if let Err(error) = descendants.and(cleanup) {
            outcome = Err(retain_trace(error));
        } else if !selected_fatal
            && let Err(error) = self
                .check_scheduler_fault()
                .and_then(|()| self.poll_group_parent(parent))
        {
            outcome = Err(retain_trace(error));
        }
        let outcome = outcome.map_err(|error| self.group_error(parent, error));
        let fatal = outcome
            .as_ref()
            .err()
            .filter(|error| error.kind() == RuntimeErrorKind::EngineFault)
            .cloned();
        self.groups.borrow_mut().records.insert(
            parent,
            GroupRecord {
                wake: wake.clone(),
                state: GroupState::Ready(outcome),
            },
        );
        self.scheduler.hub.contain(|| wake.wake());
        if !callback_fault && self.scheduler.hub.check().is_err() {
            let error = retain_trace(
                self.check_scheduler_fault()
                    .expect_err("new scheduler callback failure"),
            );
            self.groups
                .borrow_mut()
                .records
                .get_mut(&parent)
                .expect("ready group")
                .state = GroupState::Ready(Err(error.clone()));
            return Err(error);
        }
        fatal.map_or_else(|| self.check_scheduler_fault(), Err)
    }
}
