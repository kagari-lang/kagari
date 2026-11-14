//! Executable facts require an explicit current or retained managed state origin.
use crate::{
    Runtime,
    error::{RuntimeError, fault::EngineFault},
    execution::roots::{DependencyId, ExecutionRoots, Slots, ValueLocation},
    execution_metadata::managed::applications::ApplicationObject,
    module::{
        ExecutableModule, ModuleStore,
        managed::{InstallationState, cell::StaticCellObject, revision::RevisionState},
    },
    native::future::{claims::FutureContract, storage::ColdObject},
    value::{carriers::RootedValue, closures::ClosureObject},
};
use kagari_bytecode::{instruction::ModuleSlot, program::ModuleRef};
use kagari_gc::{error::GcError, object::Gc, root::Root};

/// Current ordinary selections reopen the publication. Already selected bodies
/// use their protected source or genuine Host root, never current-state fallback.
/// An Execution caller keeps that coordinate's owning suffix through admission;
/// synchronous nested calls may borrow a same-execution prefix until return.
#[derive(Clone, Copy)]
pub(crate) enum RevisionSource<'root> {
    Current,
    Execution(DependencyId<RevisionState>),
    Host(&'root Root<RevisionState>),
    Application(DependencyId<ApplicationObject>),
    HostApplication(&'root Root<ApplicationObject>),
    RootedClosure(&'root RootedValue),
    RootedFuture(&'root RootedValue),
    Future(ValueLocation),
    Cold(&'root Root<ColdObject>),
    Closure(ValueLocation),
    StaticRecipe {
        body: DependencyId<RevisionState>,
        module: ModuleRef,
        slot: ModuleSlot,
    },
}

impl Runtime {
    /// Find a body only in the current admitted frame/root, never a global ID
    /// registry. The semantic owner keeps its source prefix alive through use.
    fn frame_origin(
        &self,
        owner: &ExecutableModule,
    ) -> Result<Option<DependencyId<RevisionState>>, RuntimeError> {
        self.resources.ensure_synchronous_thread()?;
        self.resources.ensure_execution_allowed()?;
        if let Some(session) = self.resources.active_session() {
            let frames = self.resources.sessions.frames(session.id).ok_or_else(|| {
                self.resources
                    .quarantine(EngineFault::FrameStackIsBorrowedAcrossExecution)
            })?;
            if let Some(frame) = frames.last()
                && frame.loaded().program_identity() == owner.program_identity()
            {
                return frame
                    .admitted_body()
                    .map(Some)
                    .map_err(|error| self.resources.quarantine(error));
            }
            if session.root.program_identity() == owner.program_identity() {
                return Ok(Some(session.body));
            }
        }
        Ok(None)
    }

    /// The top Native record is an additional actual source candidate. Its
    /// generic coordinate is not type/program authority: resolve validates both
    /// under the heap scope before retention or frame publication.
    pub(crate) fn admitted_origin(
        &self,
        owner: &ExecutableModule,
    ) -> Result<Option<DependencyId<RevisionState>>, RuntimeError> {
        if let Some(body) = self.frame_origin(owner)? {
            return Ok(Some(body));
        }
        let Some(session) = self.resources.active_session() else {
            return Ok(None);
        };
        self.memory
            .native_origin(session.id)
            .map_err(|error| self.managed_memory_error(error))
    }

    /// Origin for a freshly selected ordinary body. Candidate/same-body calls
    /// borrow their admitted prefix; other fresh selections reopen publication.
    /// Retained descriptors must supply their own origin instead of using this.
    /// The untyped top Native coordinate is not an ordinary selection source:
    /// it may belong to another program or an older body of this installation.
    pub(crate) fn selection_origin(
        &self,
        owner: &ExecutableModule,
    ) -> Result<RevisionSource<'static>, RuntimeError> {
        Ok(self
            .frame_origin(owner)?
            .map_or(RevisionSource::Current, RevisionSource::Execution))
    }

    /// Lend one authenticated actual revision to a closed internal operation.
    /// Source/payload loans end before the caller can execute or publish code.
    pub(crate) fn with_revision_origin<R>(
        &mut self,
        owner: &ExecutableModule,
        origin: RevisionSource<'_>,
        access: impl for<'gc> FnOnce(&mut Slots<'_, 'gc>, Gc<'gc, RevisionState>) -> Result<R, GcError>,
    ) -> Result<R, RuntimeError> {
        self.resources.ensure_synchronous_thread()?;
        self.resources.ensure_execution_allowed()?;
        self.validate_executable_module(owner)?;
        let modules = &self.modules;
        let result = self.memory.scope(&self.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let revision = origin.resolve(modules, slots, owner)?;
                access(slots, revision)
            })
        });
        result.map_err(|error| match error {
            GcError::ForeignHeap
            | GcError::InvalidRoot
            | GcError::Index
            | GcError::Representation => {
                RuntimeError::module_validation("foreign, expired or mismatched revision origin")
            }
            error => self.managed_memory_error(error),
        })
    }
}

impl RevisionSource<'_> {
    /// Resolve under the paired source/heap interval and admit the exact supplying
    /// program identity. The caller publishes an edge before ending that scope.
    /// Module/candidate/execution admission remains the semantic caller's duty;
    /// keeping a revoked candidate's storage alive grants no execution authority.
    pub(crate) fn resolve<'gc>(
        self,
        modules: &ModuleStore,
        slots: &mut Slots<'_, 'gc>,
        owner: &ExecutableModule,
    ) -> Result<Gc<'gc, RevisionState>, GcError> {
        let revision = match self {
            Self::Application(source) => {
                let application = slots.dependency(source)?;
                ApplicationObject::body(slots.scope(), application)?
            }
            Self::HostApplication(root) => {
                let application = slots.scope().get(root)?;
                ApplicationObject::body(slots.scope(), application)?
            }
            Self::Current => {
                let current = modules.current.try_borrow().map_err(|_| {
                    GcError::Storage("current publication borrowed during state admission")
                })?;
                let publication = current
                    .get(&owner.installation_id())
                    .ok_or(GcError::Index)?;
                let installation = slots.scope().get(&publication.root)?;
                InstallationState::current(slots.scope(), installation).ok_or(GcError::Index)?
            }
            Self::StaticRecipe { body, module, slot } => {
                let caller = slots.dependency(body)?;
                let cell = RevisionState::static_cell(slots.scope(), caller, module, slot)?;
                StaticCellObject::recipe(slots.scope(), cell).ok_or(GcError::Index)?
            }
            Self::Closure(location) => {
                let value = slots.get(location)?;
                let object = slots
                    .scope()
                    .downcast::<ClosureObject>(value.object().ok_or(GcError::Representation)?)?;
                ClosureObject::body(slots.scope(), object).ok_or(GcError::Index)?
            }
            Self::RootedClosure(root) => {
                let value = root.get(slots.scope())?;
                let object = slots
                    .scope()
                    .downcast::<ClosureObject>(value.object().ok_or(GcError::Representation)?)?;
                ClosureObject::body(slots.scope(), object).ok_or(GcError::Index)?
            }
            Self::RootedFuture(root) => {
                let value = root.get(slots.scope())?;
                FutureContract::body(slots.scope(), value)?.ok_or(GcError::Index)?
            }
            Self::Future(location) => {
                let value = slots.get(location)?;
                FutureContract::body(slots.scope(), value)?.ok_or(GcError::Index)?
            }
            Self::Cold(root) => {
                let cold = slots.scope().get(root)?;
                ColdObject::body(slots.scope(), cold)
            }
            Self::Execution(dependency) => slots.dependency(dependency)?,
            Self::Host(root) => slots.scope().get(root)?,
        };
        if slots
            .scope()
            .read(revision, |revision| revision.code().program_identity())
            != owner.program_identity()
        {
            return Err(GcError::Representation);
        }
        Ok(revision)
    }
}
