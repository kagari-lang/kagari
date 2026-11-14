//! Static operands follow their admitted revision's actual binding container.
use crate::{
    Runtime,
    error::{RuntimeError, fault::EngineFault, trap::TrapReason},
    execution::roots::{DependencyId, ExecutionRoots, Slots},
    frame::types::compatibility::TypeView,
    host::{HostBorrowTable, HostRegistry},
    module::{
        ExecutableModule, ModuleInstance,
        managed::{
            cell::{StaticCellObject, StaticPhase},
            revision::RevisionState,
            source::RevisionSource,
        },
        state::{StaticRecipe, StaticState, initialization::InitializationAttempt},
    },
    resource::ResourceState,
    value::{
        carriers::{RootedValue, ScopedValue},
        properties::ValueProperties,
    },
    value_check::scoped::{self, CheckError},
};
use kagari_bytecode::instruction::ModuleSlot;
use kagari_gc::{access::Scope, error::GcError, object::Gc};
use std::sync::Arc;

/// A nonowning address in the caller's real source prefix. The caller/initializer
/// keeps that prefix until commit/failure settlement; facts alone grant no access.
#[derive(Debug, Clone)]
pub(crate) struct StaticAccess {
    pub(crate) owner: ExecutableModule,
    pub(crate) body: DependencyId<RevisionState>,
    pub(crate) slot: ModuleSlot,
}

pub(in crate::module) enum Observation {
    Ready,
    Uninitialized(StaticRecipe),
    Initializing(Arc<InitializationAttempt>),
    Failed(String),
}

impl StaticAccess {
    pub(crate) fn resolve<'gc>(
        &self,
        slots: &mut Slots<'_, 'gc>,
    ) -> Result<Gc<'gc, StaticCellObject>, GcError> {
        let body = slots.dependency(self.body)?;
        resolve(slots.scope(), body, &self.owner, self.slot)
    }

    pub(in crate::module) fn with<R>(
        &self,
        runtime: &mut Runtime,
        access: impl for<'gc> FnOnce(
            &mut Slots<'_, 'gc>,
            Gc<'gc, StaticCellObject>,
        ) -> Result<R, CheckError>,
    ) -> Result<R, RuntimeError> {
        runtime.with_static_origin(
            &self.owner,
            RevisionSource::Execution(self.body),
            self.slot,
            access,
        )
    }
}

fn resolve<'gc>(
    cx: &Scope<'gc>,
    body: Gc<'gc, RevisionState>,
    owner: &ExecutableModule,
    slot: ModuleSlot,
) -> Result<Gc<'gc, StaticCellObject>, GcError> {
    if cx.read(body, |body| body.code().program_identity()) != owner.program_identity() {
        return Err(GcError::Representation);
    }
    let cell = RevisionState::static_cell(cx, body, owner.slot(), slot)?;
    let matches = cx.read(cell, |cell| {
        cell.owner().registry_owner == owner.registry_owner
            && cell.owner().installation_id() == owner.installation_id()
            && cell.owner().static_contracts.get(cell.slot().index())
                == owner.static_contracts.get(slot.index())
    });
    if !matches {
        return Err(GcError::Representation);
    }
    Ok(cell)
}

impl Runtime {
    pub(in crate::module) fn validate_module_state_access(
        &mut self,
        module: &ExecutableModule,
    ) -> Result<(), RuntimeError> {
        self.resources.ensure_synchronous_thread()?;
        self.resources.ensure_execution_allowed()?;
        self.validate_executable_module(module)?;
        if self.is_candidate_initialization() || self.modules.is_staged(module) {
            return Err(RuntimeError::execution_phase_violation(
                "candidate cannot access static state",
            ));
        }
        self.flush_static_failures()
    }

    pub(in crate::module) fn with_static_origin<R>(
        &mut self,
        owner: &ExecutableModule,
        origin: RevisionSource<'_>,
        slot: ModuleSlot,
        access: impl for<'gc> FnOnce(
            &mut Slots<'_, 'gc>,
            Gc<'gc, StaticCellObject>,
        ) -> Result<R, CheckError>,
    ) -> Result<R, RuntimeError> {
        if slot.index() >= owner.bytecode.module_slots.len() {
            return Err(RuntimeError::module_validation(
                "static binding index out of bounds",
            ));
        }
        let Self {
            resources,
            memory,
            modules,
            ..
        } = self;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let revision = origin.resolve(modules, slots, owner)?;
                let cell = resolve(slots.scope(), revision, owner, slot)?;
                Ok(access(slots, cell))
            })
        });
        result
            .map_err(|error| self.managed_memory_error(error))?
            .map_err(|error| {
                self.resources
                    .admit_execution_error(error.into_runtime(self))
            })
    }

    fn static_origin(
        &mut self,
        owner: &ExecutableModule,
    ) -> Result<(ExecutableModule, RevisionSource<'static>), RuntimeError> {
        if let Some(body) = self.admitted_origin(owner)? {
            let matches = self
                .memory
                .scope(&self.resources, |cx, roots| {
                    ExecutionRoots::with(roots, cx, |slots| {
                        let revision = slots.dependency(body)?;
                        Ok(slots
                            .scope()
                            .read(revision, |revision| revision.code().program_identity())
                            == owner.program_identity())
                    })
                })
                .map_err(|error| self.managed_memory_error(error))?;
            if matches {
                return Ok((owner.clone(), RevisionSource::Execution(body)));
            }
        }
        // Host descriptors grant no retained-body access. Reach the installation
        // through its publication, then map the requested immutable contract to
        // that revision's bindings. Reload may reorder members and static slots.
        let current = self.modules.current_for(owner)?;
        let current = current
            .members()
            .find(|member| member.id == owner.id)
            .ok_or_else(|| {
                RuntimeError::module_validation("static member is no longer installed")
            })?;
        Ok((current, RevisionSource::Current))
    }

    pub(in crate::module) fn observe_static(
        &mut self,
        access: &StaticAccess,
    ) -> Result<Observation, RuntimeError> {
        access.with(self, |slots, cell| observe(slots.scope(), cell))
    }

    /// Inspect actual state without forcing initialization. Ready values acquire
    /// genuine Host roots; editing/dropping the detached snapshot cannot edit cells.
    pub fn module_instance_snapshot(
        &mut self,
        module: &ExecutableModule,
    ) -> Option<ModuleInstance> {
        self.validate_module_state_access(module).ok()?;
        let (target, origin) = self.static_origin(module).ok()?;
        let mut states = Vec::new();
        states
            .try_reserve_exact(module.bytecode.module_slots.len())
            .ok()?;
        for index in 0..module.bytecode.module_slots.len() {
            let slot = current_slot(module, ModuleSlot::new(index), &target).ok()?;
            let state = self
                .with_static_origin(&target, origin, slot, |slots, cell| {
                    Ok(match observe(slots.scope(), cell)? {
                        Observation::Ready => StaticState::Ready(
                            StaticCellObject::read(slots.scope(), cell)?.retain(slots.scope())?,
                        ),
                        Observation::Uninitialized(recipe) => StaticState::Uninitialized(recipe),
                        Observation::Initializing(_) => {
                            StaticState::Initializing(recipe(slots.scope(), cell)?)
                        }
                        Observation::Failed(_) => StaticState::Failed(
                            slots
                                .scope()
                                .read(cell, |cell| cell.failure().expect("failed static").clone()),
                        ),
                    })
                })
                .ok()?;
            states.push(state);
        }
        Some(ModuleInstance {
            id: module.id,
            name: module.name.clone(),
            epoch: module.epoch,
            module_slots: states,
        })
    }

    /// Read an already ready static without executing its initializer. A managed
    /// result owns a genuine Host root; interpreter loads publish into frame slots.
    pub fn read_module_slot(
        &mut self,
        module: &ExecutableModule,
        slot: ModuleSlot,
    ) -> Result<RootedValue, RuntimeError> {
        self.validate_module_state_access(module)?;
        let (target, origin) = self.static_origin(module)?;
        let target_slot = current_slot(module, slot, &target)?;
        self.require_static_ready(&target, origin, target_slot)?;
        let Self {
            host,
            host_borrows,
            resources,
            memory,
            modules,
            ..
        } = self;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let revision = origin.resolve(modules, slots, &target)?;
                let cell = resolve(slots.scope(), revision, &target, target_slot)?;
                Ok((|| {
                    let value = StaticCellObject::read(slots.scope(), cell)?;
                    check_ready(
                        slots.scope(),
                        value,
                        module,
                        slot,
                        host,
                        host_borrows,
                        resources,
                    )?;
                    value.retain(slots.scope()).map_err(CheckError::from)
                })())
            })
        });
        result
            .map_err(|error| self.managed_memory_error(error))?
            .map_err(|error| {
                self.resources
                    .admit_execution_error(error.into_runtime(self))
            })
    }

    /// Replace a ready mutable static from genuine Host ownership. This never
    /// initializes a cold cell and preserves readiness-before-input validation.
    pub fn write_module_slot(
        &mut self,
        module: &ExecutableModule,
        slot: ModuleSlot,
        value: &RootedValue,
    ) -> Result<(), RuntimeError> {
        self.validate_module_state_access(module)?;
        let (target, origin) = self.static_origin(module)?;
        mutable(module, slot)?;
        let target_slot = current_slot(module, slot, &target)?;
        self.require_static_ready(&target, origin, target_slot)?;
        let Self {
            memory,
            modules,
            host,
            host_borrows,
            resources,
            ..
        } = self;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let revision = origin.resolve(modules, slots, &target)?;
                let cell = resolve(slots.scope(), revision, &target, target_slot)?;
                Ok((|| {
                    let previous = StaticCellObject::read(slots.scope(), cell)?;
                    check_ready(
                        slots.scope(),
                        previous,
                        module,
                        slot,
                        host,
                        host_borrows,
                        resources,
                    )?;
                    let value = value
                        .get(slots.scope())
                        .map_err(|_| RuntimeError::from(TrapReason::InvalidOrForeignHeapPayload))?;
                    check_input(
                        slots.scope(),
                        value,
                        module,
                        slot,
                        host,
                        host_borrows,
                        resources,
                    )?;
                    resources.poll_closed()?;
                    StaticCellObject::replace(slots.scope(), cell, value).map_err(CheckError::from)
                })())
            })
        });
        result
            .map_err(|error| self.managed_memory_error(error))?
            .map_err(|error| {
                self.resources
                    .admit_execution_error(error.into_runtime(self))
            })
    }

    pub(crate) fn require_frame_static_ready(
        &mut self,
        access: &StaticAccess,
    ) -> Result<(), RuntimeError> {
        self.validate_module_state_access(&access.owner)?;
        self.require_static_ready(
            &access.owner,
            RevisionSource::Execution(access.body),
            access.slot,
        )
    }

    fn require_static_ready(
        &mut self,
        owner: &ExecutableModule,
        origin: RevisionSource<'_>,
        slot: ModuleSlot,
    ) -> Result<(), RuntimeError> {
        match self.with_static_origin(owner, origin, slot, |slots, cell| {
            observe(slots.scope(), cell)
        })? {
            Observation::Ready => Ok(()),
            Observation::Uninitialized(_) => Err(RuntimeError::module_validation(
                "static initialization requires an execution transition",
            )),
            Observation::Failed(cause) => Err(RuntimeError::module_validation(format!(
                "static initialization failed: {cause}"
            ))),
            Observation::Initializing(attempt) => {
                attempt.fail("recursive static initialization", None);
                self.flush_static_failures()?;
                Err(RuntimeError::module_validation(
                    "recursive static initialization",
                ))
            }
        }
    }
}

fn current_slot(
    owner: &ExecutableModule,
    slot: ModuleSlot,
    target: &ExecutableModule,
) -> Result<ModuleSlot, RuntimeError> {
    let contract = owner
        .static_contracts
        .get(slot.index())
        .ok_or_else(|| RuntimeError::module_validation("static binding index out of bounds"))?;
    if owner.program_identity() == target.program_identity() {
        return Ok(slot);
    }
    target
        .static_contracts
        .iter()
        .position(|current| current == contract)
        .map(ModuleSlot::new)
        .ok_or_else(|| RuntimeError::module_validation("static contract is no longer installed"))
}

pub(crate) fn mutable(owner: &ExecutableModule, slot: ModuleSlot) -> Result<(), RuntimeError> {
    let descriptor = owner
        .bytecode
        .module_slots
        .get(slot.index())
        .ok_or_else(|| RuntimeError::module_validation("static binding index out of bounds"))?;
    if !descriptor.contract.mutable {
        return Err(RuntimeError::module_validation("immutable static binding"));
    }
    Ok(())
}

pub(crate) fn check_ready<'gc>(
    cx: &Scope<'gc>,
    value: ScopedValue<'gc>,
    owner: &ExecutableModule,
    slot: ModuleSlot,
    host: &HostRegistry,
    borrows: &HostBorrowTable,
    resources: &ResourceState,
) -> Result<(), CheckError> {
    if !ValueProperties::of(cx, value)?.storable
        || !scoped::matches(
            cx,
            value,
            TypeView::new(
                &owner.bytecode.module_slots[slot.index()].contract.ty,
                owner,
                None,
            ),
            host,
            borrows,
            resources,
        )?
    {
        return Err(resources
            .quarantine(EngineFault::InvalidSemanticValueInStaticCell)
            .into());
    }
    Ok(())
}

pub(crate) fn check_input<'gc>(
    cx: &Scope<'gc>,
    value: ScopedValue<'gc>,
    owner: &ExecutableModule,
    slot: ModuleSlot,
    host: &HostRegistry,
    borrows: &HostBorrowTable,
    resources: &ResourceState,
) -> Result<(), CheckError> {
    if !ValueProperties::of(cx, value)?.storable {
        return Err(RuntimeError::from(TrapReason::InvalidOrForeignHeapPayload).into());
    }
    if !scoped::matches(
        cx,
        value,
        TypeView::new(
            &owner.bytecode.module_slots[slot.index()].contract.ty,
            owner,
            None,
        ),
        host,
        borrows,
        resources,
    )? {
        return Err(RuntimeError::module_validation("static semantic type mismatch").into());
    }
    Ok(())
}

fn recipe<'gc>(
    cx: &Scope<'gc>,
    cell: Gc<'gc, StaticCellObject>,
) -> Result<StaticRecipe, CheckError> {
    let body = StaticCellObject::recipe(cx, cell).ok_or(GcError::Representation)?;
    let recipe = cx.read(cell, |cell| StaticRecipe {
        module: cell.owner().clone(),
        function: cell.owner().bytecode.module_slots[cell.slot().index()].initializer,
    });
    if cx.read(body, |body| body.code().program_identity()) != recipe.module.program_identity() {
        return Err(GcError::Representation.into());
    }
    Ok(recipe)
}

fn observe<'gc>(
    cx: &Scope<'gc>,
    cell: Gc<'gc, StaticCellObject>,
) -> Result<Observation, CheckError> {
    Ok(match cx.read(cell, StaticCellObject::phase) {
        StaticPhase::Ready => Observation::Ready,
        StaticPhase::Uninitialized => Observation::Uninitialized(recipe(cx, cell)?),
        StaticPhase::Initializing => Observation::Initializing(
            cx.read(cell, |cell| cell.attempt().cloned())
                .ok_or(GcError::Representation)?,
        ),
        StaticPhase::Failed => Observation::Failed(
            cx.read(cell, |cell| cell.failure().map(ToString::to_string))
                .ok_or(GcError::Representation)?,
        ),
    })
}
