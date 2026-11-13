//! Fixed closed-call dictionaries belong to their supplying managed revision.
use crate::{
    Runtime,
    error::{RuntimeError, fault::EngineFault},
    execution::roots::ExecutionRoots,
    execution_metadata::{
        call_contracts::ScopedInterfaceCall,
        managed::{
            Dependencies, ProtectedGraph,
            sources::{MetadataError, MetadataSource},
        },
    },
    module::{
        managed::{revision::RevisionState, source::RevisionSource},
        staging::StagedProgram,
    },
};
use std::sync::Arc;

impl Runtime {
    /// Build once under the candidate's actual root, then transfer the finite
    /// graph before retiring all temporary preparation coordinates.
    pub(crate) fn link_staged_execution(
        &mut self,
        staged: &StagedProgram,
    ) -> Result<(), RuntimeError> {
        let execution = self
            .memory
            .enter_execution(&self.resources)
            .map_err(|error| self.managed_memory_error(error))?;
        let mut protected = vec![MetadataSource::Revision {
            owner: staged.module(),
            source: RevisionSource::Host(staged.revision()),
        }];
        let result = (|| {
            let mut calls = Vec::new();
            for owner in staged.module().members() {
                self.resources.poll_execution()?;
                let prepared = self.link_execution(execution, &owner, &mut protected)?;
                calls
                    .try_reserve(prepared.len())
                    .map_err(|_| RuntimeError::resource_limit("fixed linked calls"))?;
                calls.extend(prepared);
            }
            self.publish_linked_call_metadata(staged, &calls, &protected)
        })();
        self.memory
            .retire_execution(&self.resources, execution)
            .map_err(|error| {
                self.resources
                    .quarantine(EngineFault::ExecutionStorageRetirementFailed(error))
            })?;
        result
    }

    fn publish_linked_call_metadata(
        &mut self,
        staged: &StagedProgram,
        calls: &[Arc<ScopedInterfaceCall>],
        protected: &[MetadataSource<'_>],
    ) -> Result<(), RuntimeError> {
        let Runtime {
            memory,
            modules,
            resources,
            ..
        } = self;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                Ok((|| -> Result<(), MetadataError> {
                    let revision = slots.scope().get(staged.revision())?;
                    let mut targets = Vec::new();
                    targets.try_reserve_exact(protected.len())?;
                    for (index, source) in protected.iter().enumerate() {
                        if index.is_multiple_of(1024) {
                            resources.poll_closed()?;
                        }
                        targets.push(source.resolve(modules, slots)?);
                    }
                    let edges = Dependencies::collect(resources, |visit| {
                        for (index, call) in calls.iter().enumerate() {
                            if index.is_multiple_of(1024) {
                                resources.poll_closed()?;
                            }
                            call.trace(visit)?;
                        }
                        Ok(())
                    })?;
                    let dependencies =
                        Dependencies::resolve(slots.scope(), edges, &targets, resources)?;
                    let programs = ProtectedGraph::new(slots.scope(), &dependencies, resources)?
                        .programs(slots.scope(), resources)?;
                    for program in &programs {
                        if program.program_identity() != staged.module().program_identity() {
                            return Err(RuntimeError::module_validation(
                                "foreign closed call dependency",
                            )
                            .into());
                        }
                    }
                    RevisionState::link_metadata(slots.scope(), revision, &dependencies, resources)
                })())
            })
        });
        result
            .map_err(|error| self.managed_memory_error(error))?
            .map_err(|error| match error {
                MetadataError::Memory(error) => self.managed_memory_error(error),
                MetadataError::Execution(error) => self.resources.admit_execution_error(error),
            })
    }
}
