//! Apply checked metadata binding plans to actual protected installation state.
use crate::{
    error::RuntimeError,
    execution::memory::ManagedMemory,
    module::{
        ExecutableModule, ModuleStore,
        managed::{
            InstallationState, cell::StaticCellObject, revision::RevisionState,
            statics::StaticState,
        },
        staging::{ManagedProgramError, PreparedProgramMetadata, StagedProgram},
        state::cells::CellId,
    },
    resource::ResourceState,
};
use kagari_bytecode::{instruction::ModuleSlot, program::ModuleRef};
use kagari_gc::{error::GcError, root::Root};
use std::collections::HashMap;

impl PreparedProgramMetadata {
    /// Metadata owns an abandonment lease before managed allocation. A new graph
    /// is admitted only by that registered initial installation; reload derives
    /// its installation from the existing publication root, never a module key.
    pub(crate) fn into_managed(
        self,
        store: &ModuleStore,
        memory: &mut ManagedMemory,
        resources: &ResourceState,
    ) -> Result<StagedProgram, GcError> {
        let current = store
            .current
            .try_borrow()
            .map_err(|_| GcError::Storage("current publication is borrowed during staging"))?;
        let published = match self.expected_base {
            Some(expected) => {
                let publication = current.get(&self.installation.id).ok_or(GcError::Index)?;
                if publication.program.modules[publication.program.root.index()].epoch
                    != expected.epoch
                    || publication.program.modules[publication.program.root.index()].id
                        != expected.id
                {
                    return Err(GcError::Storage(
                        "staging baseline changed before managed admission",
                    ));
                }
                Some(&publication.root)
            }
            None => None,
        };
        let retirement = store.retirements.reserve(self.module.program_key())?;
        store
            .inner
            .try_borrow_mut()
            .map_err(|_| GcError::Storage("records borrowed during revision admission"))?
            .resolve_mut(&self.module)
            .ok_or(GcError::Index)?
            .managed_owner = true;
        let revision = memory.scope(resources, |cx, _| {
            let installation = match published {
                Some(root) => cx.get(root)?,
                None => InstallationState::alloc(cx, &self.installation)?,
            };
            let revision = RevisionState::alloc(cx, installation, self.module.clone(), retirement)?;
            cx.retain(revision)
        })?;
        Ok(StagedProgram {
            installation: self.installation,
            module: self.module,
            lease: self.lease,
            expected_base: self.expected_base,
            revision,
        })
    }
}

#[derive(Clone, Copy)]
struct CellLocation {
    module: ModuleRef,
    slot: ModuleSlot,
}

enum BindingSource {
    Existing(CellLocation),
    New(usize),
}

struct BindingPlan {
    new: Vec<CellLocation>,
    bindings: Vec<BindingSource>,
}

impl StagedProgram {
    pub(crate) fn is_initial(&self) -> bool {
        self.expected_base.is_none()
    }

    pub(crate) fn revision(&self) -> &Root<RevisionState> {
        &self.revision
    }

    /// Source IDs are logical static identities from the already checked binding
    /// plan. Existing cells are reopened through the protected baseline revision;
    /// no old values, numeric heap identities or independent cell roots are copied.
    pub(crate) fn finish_bindings(
        &self,
        store: &ModuleStore,
        memory: &mut ManagedMemory,
        resources: &ResourceState,
    ) -> Result<(), ManagedProgramError> {
        let plan = self.binding_plan(store)?;
        let current = store.current.try_borrow().map_err(|_| {
            RuntimeError::module_validation(
                "current publications borrowed during static preparation",
            )
        })?;
        let baseline = self
            .expected_base
            .map(|_| {
                current.get(&self.installation.id).ok_or_else(|| {
                    RuntimeError::module_validation("static baseline installation is closed")
                })
            })
            .transpose()?;
        memory.scope(resources, |cx, _| {
            let revision = cx.get(&self.revision)?;
            let installation = RevisionState::installation(cx, revision);
            let baseline = baseline
                .map(|publication| {
                    let published = cx.get(&publication.root)?;
                    if !published.ptr_eq(installation) {
                        return Err(GcError::Representation);
                    }
                    let baseline =
                        InstallationState::current(cx, published).ok_or(GcError::Index)?;
                    if cx.read(baseline, |baseline| baseline.code().key())
                        != self.expected_base.expect("reload baseline")
                    {
                        return Err(GcError::Storage(
                            "static baseline changed before graph preparation",
                        ));
                    }
                    Ok(baseline)
                })
                .transpose()?;
            let mut new = Vec::new();
            new.try_reserve_exact(plan.new.len())?;
            for location in &plan.new {
                new.push(StaticCellObject::alloc(
                    cx,
                    revision,
                    location.module,
                    location.slot,
                )?);
            }
            let mut bindings = Vec::new();
            bindings.try_reserve_exact(plan.bindings.len())?;
            for binding in &plan.bindings {
                bindings.push(match binding {
                    BindingSource::New(index) => new[*index],
                    BindingSource::Existing(location) => RevisionState::static_cell(
                        cx,
                        baseline.ok_or(GcError::Index)?,
                        location.module,
                        location.slot,
                    )?,
                });
            }
            let statics = StaticState::alloc(cx, installation, &bindings)?;
            RevisionState::link(cx, revision, statics)
        })?;
        Ok(())
    }

    fn binding_plan(&self, store: &ModuleStore) -> Result<BindingPlan, RuntimeError> {
        let records = store.inner.try_borrow().map_err(|_| {
            RuntimeError::module_validation("static records borrowed during graph preparation")
        })?;
        let current = store.current.try_borrow().map_err(|_| {
            RuntimeError::module_validation(
                "current publications borrowed during graph preparation",
            )
        })?;
        let mut existing = HashMap::<CellId, CellLocation>::new();
        if let Some(expected) = self.expected_base {
            let published = current.get(&self.installation.id).ok_or_else(|| {
                RuntimeError::module_validation("static baseline installation is closed")
            })?;
            let baseline = ExecutableModule {
                program: published.program.clone(),
                slot: published.program.root,
            };
            if baseline.key() != expected {
                return Err(RuntimeError::module_validation("static baseline changed"));
            }
            for member in baseline.members() {
                let record = records.resolve(&member).ok_or_else(|| {
                    RuntimeError::module_validation("static baseline record retired")
                })?;
                for (slot, id) in record.bindings.iter().enumerate() {
                    records.bound_cell(record, slot).ok_or_else(|| {
                        RuntimeError::module_validation("invalid static baseline binding")
                    })?;
                    existing.entry(*id).or_insert(CellLocation {
                        module: member.slot(),
                        slot: ModuleSlot::new(slot),
                    });
                }
            }
        }
        let mut new = Vec::new();
        let mut created = HashMap::new();
        for member in self.module.members() {
            let record = records.resolve(&member).ok_or_else(|| {
                RuntimeError::module_validation("static candidate record retired")
            })?;
            for (slot, descriptor) in member.bytecode.module_slots.iter().enumerate() {
                if descriptor.contract.supplier != member.bytecode.identity {
                    continue;
                }
                records.bound_cell(record, slot).ok_or_else(|| {
                    RuntimeError::module_validation("invalid static candidate binding")
                })?;
                let id = record.bindings[slot];
                if existing.contains_key(&id) {
                    continue;
                }
                new.try_reserve(1)
                    .map_err(|_| RuntimeError::resource_limit("static graph plan"))?;
                let index = new.len();
                new.push(CellLocation {
                    module: member.slot(),
                    slot: ModuleSlot::new(slot),
                });
                if created.insert(id, index).is_some() {
                    return Err(RuntimeError::module_validation(
                        "duplicate static supplier binding",
                    ));
                }
            }
        }
        let mut bindings = Vec::new();
        for member in self.module.members() {
            let record = records.resolve(&member).ok_or_else(|| {
                RuntimeError::module_validation("static candidate record retired")
            })?;
            for (slot, id) in record.bindings.iter().enumerate() {
                records.bound_cell(record, slot).ok_or_else(|| {
                    RuntimeError::module_validation("invalid static candidate binding")
                })?;
                bindings
                    .try_reserve(1)
                    .map_err(|_| RuntimeError::resource_limit("static graph bindings"))?;
                bindings.push(match existing.get(id) {
                    Some(location) => BindingSource::Existing(*location),
                    None => BindingSource::New(*created.get(id).ok_or_else(|| {
                        RuntimeError::module_validation("missing static supplier binding")
                    })?),
                });
            }
        }
        Ok(BindingPlan { new, bindings })
    }
}
