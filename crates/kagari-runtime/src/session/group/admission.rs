//! Reserve every local branch owner before committing the pair's one-shot claims.
use crate::{
    Runtime,
    error::RuntimeError,
    native::{binding::NativeResult, composition::future_output, storage::value::StoredValue},
    session::{
        group::{GroupRecord, GroupState, RunningGroup, invalid},
        store::SessionId,
    },
    value::Value,
};
use std::{slice, task::Waker};

impl Runtime {
    pub(super) fn admit_group(&self, parent: SessionId, record: GroupRecord) -> NativeResult<()> {
        let GroupState::Admitting(cold, inputs) = record.state else {
            return Err(invalid());
        };
        let mut group = Box::new(RunningGroup {
            contract: cold,
            _inputs: inputs,
            branches: [None, None],
            outputs: [None, None],
            next: 0,
        });
        let admitted = (|| {
            self.poll_group_parent(parent)?;
            let ids = group.contract.values.map(|value| match value.value() {
                Value::GcHandle(id) => Ok(id),
                _ => Err(invalid()),
            });
            let [left, right] = ids;
            let ids = [left?, right?];
            // State validation is left-to-right, and roots precede every callback
            // or allocation that could collect the heap after ownership transfers.
            self.gc()
                .validate_async_values(StoredValue::values(&group.contract.values))?;
            let captures = self.gc().prepare_future_pair(ids)?;
            for (index, id) in ids.iter().enumerate() {
                let (owner, ty, scope) = self.gc().future_contract(*id)?;
                self.validate_executable_module(&owner)?;
                let applied = match scope {
                    Some(scope) => scope,
                    None => self
                        .resolve_type_arguments(&owner, slice::from_ref(&ty))?
                        .pop()
                        .ok_or_else(invalid)?,
                };
                let output = future_output(self, &owner, &applied)?;
                if !output
                    .view(&owner)
                    .compatible(group.contract.inputs[index].view(&group.contract.owner))
                {
                    return Err(RuntimeError::module_validation(
                        "composition branch output type",
                    ));
                }
                if let Some(closure) = self.gc().future_script(*id)? {
                    self.checked_future_resume(closure, &owner, &output)?;
                }
                let execution = self.prepare_owned_branch(
                    parent,
                    &owner,
                    group.contract.values[index].value(),
                    output,
                )?;
                group.branches[index] = Some(execution);
                self.scheduler.hub.contain(|| {
                    group.branches[index]
                        .as_ref()
                        .expect("reserved branch")
                        .set_waker(&Waker::from(self.scheduler.hub.clone()))
                });
                self.check_scheduler_fault()?;
            }
            let children = group
                .branches
                .each_ref()
                .map(|branch| branch.as_ref().expect("reserved branch").id);
            self.dependencies.attach_group(parent, children)?;
            self.poll_group_parent(parent)?;
            let claims = self.gc().take_cold_future_pair(ids)?;
            // Everything below is an infallible move into pre-reserved sessions.
            for ((branch, cold), roots) in group.branches.iter().zip(claims).zip(captures) {
                self.install_branch_claim(branch.as_ref().expect("reserved branch"), cold, roots);
            }
            Ok(())
        })();
        let record = GroupRecord {
            wake: record.wake,
            state: GroupState::Running(group),
        };
        match admitted {
            Ok(()) => {
                self.groups.borrow_mut().records.insert(parent, record);
                Ok(())
            }
            Err(error) => self.finish_group_record(parent, record, Err(error)),
        }
    }
}
