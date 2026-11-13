//! Reserve both local branch owners before committing their one-shot claims.
use crate::{
    Runtime,
    error::RuntimeError,
    execution::roots::{ExecutionRoots, LocalMark, SessionId},
    native::{binding::NativeResult, composition::future_output, future::storage::ColdObject},
    session::group::{GroupRecord, GroupState, RunningGroup, invalid},
    value::view::ValueView,
};
use kagari_gc::{error::GcError, root::Root};
use std::task::Waker;

impl Runtime {
    pub(super) fn admit_group(
        &mut self,
        parent: SessionId,
        record: GroupRecord,
    ) -> NativeResult<()> {
        let GroupState::Admitting(cold, inputs) = record.state else {
            return Err(invalid());
        };
        let mut group = Box::new(RunningGroup {
            contract: cold,
            inputs,
            branches: [None, None],
            outputs: [None, None],
            next: 0,
        });
        let admitted = self.admit_group_branches(parent, &mut group);
        let record = GroupRecord {
            wake: record.wake,
            state: GroupState::Running(group),
        };
        match admitted {
            Ok(()) => {
                self.groups.records.insert(parent, record);
                Ok(())
            }
            Err(error) => self.finish_group_record(parent, record, Err(error)),
        }
    }

    fn admit_group_branches(
        &mut self,
        parent: SessionId,
        group: &mut RunningGroup,
    ) -> NativeResult<()> {
        self.poll_group_parent(parent)?;
        let (mark, values) = self.group_input_views(parent, &group.inputs)?;
        // Reservation does not run branches. This local prefix ends before any
        // child starts, including on a failed admission; it never crosses a frame.
        let admitted = (|| {
            let contracts = self.future_pair_contracts(&values)?;
            for (index, contract) in contracts.iter().enumerate() {
                self.validate_executable_module(&contract.owner)?;
                let output = future_output(self, &contract.owner, &contract.applied)?;
                if !output
                    .view(&contract.owner)
                    .compatible(group.contract.inputs[index].view(&group.contract.owner))
                {
                    return Err(RuntimeError::module_validation(
                        "composition branch output type",
                    ));
                }
                if let Some(closure) = &contract.script {
                    self.validate_future_resume(closure, &contract.owner, &output)?;
                }
                let execution = self.prepare_owned_branch(parent, &contract.owner, output)?;
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
            let claims = self.claim_future_pair(&values)?;
            // Claims own actual computation roots. Everything after the atomic
            // detach is an infallible move into pre-reserved sessions.
            for (branch, claim) in group.branches.iter().zip(claims) {
                self.install_branch_claim(branch.as_ref().expect("reserved branch"), claim);
            }
            self.memory
                .scope(&self.resources, |cx, _| {
                    let cold = cx.get(&group.inputs)?;
                    ColdObject::release_captures(cx, cold)
                })
                .map_err(|error| self.managed_memory_error(error))?;
            Ok(())
        })();
        let retired = self.memory.scope(&self.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| slots.retire_locals(mark))
        });
        retired.map_err(|error| self.managed_memory_error(error))?;
        admitted
    }

    fn group_input_views<'input>(
        &mut self,
        parent: SessionId,
        source: &'input Root<ColdObject>,
    ) -> NativeResult<(LocalMark, [ValueView<'input>; 2])> {
        self.memory
            .scope(&self.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let cold = slots.scope().get(source)?;
                    if slots.scope().read(cold, ColdObject::len) != 2 {
                        return Err(GcError::Representation);
                    }
                    let values = [
                        ColdObject::capture(slots.scope(), cold, 0)?,
                        ColdObject::capture(slots.scope(), cold, 1)?,
                    ];
                    slots.reserve_locals(parent, 2)?;
                    let mark = slots.mark_locals(parent)?;
                    let publish = (|| {
                        let left = slots.push_local(parent, values[0])?;
                        let right = slots.push_local(parent, values[1])?;
                        Ok([
                            ValueView::protected(values[0], Some(left))?,
                            ValueView::protected(values[1], Some(right))?,
                        ])
                    })();
                    match publish {
                        Ok(values) => Ok((mark, values)),
                        Err(error) => {
                            slots.retire_locals(mark)?;
                            Err(error)
                        }
                    }
                })
            })
            .map_err(|error| self.managed_memory_error(error))
    }
}
