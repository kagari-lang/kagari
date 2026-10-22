//! Select one branch per shared scheduler activation; policy never drives code.
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    gc::roots::RootedValue,
    native::{
        binding::NativeResult,
        composition::{PairContext, PairDecision},
    },
    session::{
        group::{BranchActivation, GroupRecord, GroupState, GroupWork, invalid},
        owned::OwnedExecution,
        store::SessionId,
    },
};
use std::slice;

impl Runtime {
    pub(crate) fn acquire_group(&self, parent: SessionId) -> NativeResult<GroupWork<'_>> {
        self.require_idle_driver()?;
        let Some(mut record) = self.groups.borrow_mut().records.remove(&parent) else {
            return Ok(GroupWork::Stale);
        };
        if matches!(record.state, GroupState::Ready(_)) {
            self.groups.borrow_mut().records.insert(parent, record);
            return Ok(GroupWork::Stale);
        }
        if let Err(error) = self.poll_group_parent(parent) {
            self.finish_group_record(parent, record, Err(error))?;
            return Ok(GroupWork::Control);
        }
        match &mut record.state {
            GroupState::Admitting(..) => {
                self.admit_group(parent, record)?;
                Ok(GroupWork::Control)
            }
            GroupState::Running(group) => {
                let mut selected = None;
                for offset in 0..2 {
                    let index = (group.next + offset) % 2;
                    if group.branches[index]
                        .as_ref()
                        .is_some_and(OwnedExecution::is_ready)
                    {
                        selected =
                            Some((index, group.branches[index].take().expect("ready branch")));
                        group.next = (index + 1) % 2;
                        break;
                    }
                }
                self.groups.borrow_mut().records.insert(parent, record);
                Ok(match selected {
                    Some((index, execution)) => GroupWork::Branch(BranchActivation {
                        runtime: self,
                        parent,
                        index,
                        execution: Some(execution),
                    }),
                    None => GroupWork::Waiting,
                })
            }
            GroupState::Ready(_) => unreachable!(),
        }
    }

    fn accept_branch(
        &self,
        parent: SessionId,
        index: usize,
        mut record: GroupRecord,
        outcome: NativeResult<RootedValue>,
    ) -> NativeResult<()> {
        let decision = (|| {
            let output = outcome?;
            self.poll_group_parent(parent)?;
            let GroupState::Running(group) = &mut record.state else {
                return Err(invalid());
            };
            let value = output.value(self.gc()).ok_or_else(invalid)?;
            if !group.contract.inputs[index].matches(self, &value, &group.contract.owner) {
                return Err(invalid());
            }
            self.gc().validate_async_values(slice::from_ref(&value))?;
            group.outputs[index] = Some(output);
            let cx = PairContext {
                runtime: self,
                contract: &group.contract,
                outputs: &group.outputs,
            };
            let decision = self
                .scheduler
                .hub
                .contain(|| group.contract.policy.on_output(&cx, index));
            self.check_scheduler_fault()?;
            self.poll_group_parent(parent)?;
            match decision.expect("contained policy")? {
                PairDecision::Continue if group.outputs.iter().all(Option::is_some) => {
                    Err(invalid())
                }
                PairDecision::Finish(root) => {
                    let value = root.value(self.gc()).ok_or_else(invalid)?;
                    if !group
                        .contract
                        .output
                        .matches(self, &value, &group.contract.owner)
                    {
                        return Err(invalid());
                    }
                    self.gc().validate_async_values(slice::from_ref(&value))?;
                    Ok(PairDecision::Finish(root))
                }
                decision => Ok(decision),
            }
        })();
        match decision {
            Ok(PairDecision::Continue) => {
                self.groups.borrow_mut().records.insert(parent, record);
                Ok(())
            }
            Ok(PairDecision::Finish(output)) => {
                self.finish_group_record(parent, record, Ok(output))
            }
            Err(error) => self.finish_group_record(parent, record, Err(error)),
        }
    }
}

impl BranchActivation<'_> {
    pub fn execution(&self) -> &OwnedExecution {
        self.execution.as_ref().expect("active branch")
    }

    pub fn park(mut self) -> NativeResult<()> {
        self.runtime.require_idle_driver()?;
        let mut groups = self.runtime.groups.borrow_mut();
        let record = groups.records.get_mut(&self.parent).ok_or_else(invalid)?;
        let GroupState::Running(group) = &mut record.state else {
            return Err(invalid());
        };
        group.branches[self.index] = self.execution.take();
        Ok(())
    }

    /// The ordinary owned executor must retire the branch before reporting output.
    pub fn complete(mut self, outcome: NativeResult<RootedValue>) -> NativeResult<()> {
        self.runtime.require_idle_driver()?;
        if self
            .runtime
            .resources()
            .sessions
            .get(self.execution().id)
            .is_some()
        {
            return Err(self
                .runtime
                .quarantine_execution_invariant("branch publication before retirement"));
        }
        self.execution.take();
        let record = self
            .runtime
            .groups
            .borrow_mut()
            .records
            .remove(&self.parent)
            .ok_or_else(invalid)?;
        self.runtime
            .accept_branch(self.parent, self.index, record, outcome)
    }

    pub fn fail(mut self, error: RuntimeError) -> NativeResult<()> {
        self.runtime.require_idle_driver()?;
        let error = self
            .runtime
            .retire_scheduled_execution(self.execution(), error);
        self.close(error)
    }

    fn close(&mut self, error: RuntimeError) -> NativeResult<()> {
        let execution = self.execution.take().expect("active branch");
        let record = self
            .runtime
            .groups
            .borrow_mut()
            .records
            .remove(&self.parent);
        let Some(mut record) = record else {
            if self
                .runtime
                .resources()
                .sessions
                .get(execution.id)
                .is_some()
            {
                self.runtime.finish_owned_execution(&execution)?;
            }
            return Ok(());
        };
        let GroupState::Running(group) = &mut record.state else {
            return Err(invalid());
        };
        if self.runtime.require_idle_driver().is_err() {
            // A backend can abandon a selection before releasing its stack.
            // Preserve local ownership until an idle boundary can acknowledge it.
            self.runtime.scheduler.hub.contain(|| execution.cancel());
            group.branches[self.index] = Some(execution);
            self.runtime
                .groups
                .borrow_mut()
                .records
                .insert(self.parent, record);
            self.runtime.scheduler.hub.mark_ready();
            return Ok(());
        }
        group.branches[self.index] = Some(execution);
        self.runtime
            .finish_group_record(self.parent, record, Err(error))
    }
}

impl Drop for BranchActivation<'_> {
    fn drop(&mut self) {
        if self.execution.is_some() {
            let _ = self.close(RuntimeError::new(
                RuntimeErrorKind::Cancelled,
                "branch activation abandoned",
            ));
        }
    }
}
