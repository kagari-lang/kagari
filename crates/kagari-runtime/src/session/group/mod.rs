//! Structured pairs own internal executions, never Task handles or host reports.
mod admission;
mod cleanup;
mod drive;

use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    error_trace::ErrorTrace,
    gc::roots::{RootSet, RootedValue},
    native::{binding::NativeResult, composition::ColdComposition, storage::value::StoredValue},
    scheduler::WorkKey,
    session::{ExecutionId, owned::OwnedExecution, store::SessionId},
};
use std::{
    collections::HashMap,
    task::{Poll, Waker},
};

#[derive(Debug, Default)]
pub(crate) struct GroupStore {
    records: HashMap<SessionId, GroupRecord>,
}

#[derive(Debug)]
struct GroupRecord {
    wake: Waker,
    state: GroupState,
}

#[derive(Debug)]
enum GroupState {
    Admitting(ColdComposition, RootSet),
    Running(Box<RunningGroup>),
    Ready(NativeResult<RootedValue>),
}

#[derive(Debug)]
struct RunningGroup {
    contract: ColdComposition,
    _inputs: RootSet,
    branches: [Option<OwnedExecution>; 2],
    outputs: [Option<RootedValue>; 2],
    next: usize,
}

pub(crate) enum GroupWork<'a> {
    Branch(BranchActivation<'a>),
    Control,
    Waiting,
    Stale,
}

/// A backend borrows one selected branch, using the ordinary owned executor.
/// Dropping a selection closes its group; it cannot detach a live branch.
pub struct BranchActivation<'a> {
    runtime: &'a Runtime,
    parent: SessionId,
    index: usize,
    execution: Option<OwnedExecution>,
}

impl Runtime {
    pub(crate) fn request_group(
        &self,
        parent: SessionId,
        cold: ColdComposition,
        wake: Waker,
    ) -> NativeResult<()> {
        self.poll_group_parent(parent)?;
        let nesting = self
            .resources()
            .sessions
            .get(parent)
            .ok_or_else(invalid)?
            .group_depth
            .get();
        if nesting >= self.async_limits.max_group_depth.get() {
            return Err(RuntimeError::resource_limit("structured group depth"));
        }
        {
            let mut groups = self.groups.borrow_mut();
            if groups.records.contains_key(&parent) {
                return Err(invalid());
            }
            if groups.records.len() >= self.async_limits.max_groups.get() {
                return Err(RuntimeError::resource_limit("structured groups"));
            }
            groups
                .records
                .try_reserve(1)
                .map_err(|_| RuntimeError::resource_limit("structured groups"))?;
        }
        self.scheduler.reserve_work()?;
        let inputs = self
            .gc()
            .root_execution_values(StoredValue::values(&cold.values).to_vec())
            .ok_or_else(|| RuntimeError::resource_limit("group input roots"))?;
        self.groups.borrow_mut().records.insert(
            parent,
            GroupRecord {
                wake,
                state: GroupState::Admitting(cold, inputs),
            },
        );
        self.scheduler.enqueue(WorkKey::Group(ExecutionId(parent)));
        Ok(())
    }

    pub(crate) fn poll_group(&self, parent: SessionId) -> NativeResult<Poll<RootedValue>> {
        self.poll_group_parent(parent)?;
        let ready = matches!(
            self.groups
                .borrow()
                .records
                .get(&parent)
                .ok_or_else(invalid)?
                .state,
            GroupState::Ready(_)
        );
        if !ready {
            return Ok(Poll::Pending);
        }
        let record = self
            .groups
            .borrow_mut()
            .records
            .remove(&parent)
            .expect("ready group");
        let GroupState::Ready(result) = record.state else {
            unreachable!()
        };
        self.retirement_cleanup(|| {
            drop(record.wake);
            Ok(())
        })?;
        result.map(Poll::Ready)
    }

    pub(crate) fn poll_group_parent(&self, parent: SessionId) -> NativeResult<()> {
        self.resources().ensure_execution_allowed()?;
        let state = self.resources().sessions.get(parent).ok_or_else(invalid)?;
        state.poll(self.resources())?;
        if state
            .owner
            .borrow()
            .as_ref()
            .is_none_or(|owner| owner.abandoned())
        {
            return Err(RuntimeError::new(
                RuntimeErrorKind::Cancelled,
                "group parent abandoned",
            ));
        }
        Ok(())
    }

    fn group_error(&self, parent: SessionId, error: RuntimeError) -> RuntimeError {
        let Some(state) = self.resources().sessions.get(parent) else {
            return error;
        };
        if error.kind() == RuntimeErrorKind::EngineFault {
            return error;
        }
        if let Err(termination) = state.poll(self.resources()) {
            return termination;
        }
        if matches!(
            error.kind(),
            RuntimeErrorKind::Cancelled | RuntimeErrorKind::ResourceLimitExceeded
        ) {
            return state.terminate(self.resources(), error);
        }
        error.with_trace(ErrorTrace::capture_session(self.resources(), parent))
    }

    pub(crate) fn groups_have_ready_work(&self) -> bool {
        self.groups
            .borrow()
            .records
            .iter()
            .any(|(parent, record)| match &record.state {
                GroupState::Admitting(..) => true,
                GroupState::Running(group) => {
                    self.poll_group_parent(*parent).is_err()
                        || group
                            .branches
                            .iter()
                            .flatten()
                            .any(OwnedExecution::is_ready)
                }
                GroupState::Ready(_) => false,
            })
    }
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("structured group state")
}
