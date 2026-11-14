//! Internal scalar leaf transitions keep the existing frame/storage protocol.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};
use crate::{
    error::{RuntimeError, fault::EngineFault},
    execution::{roots::Slots, space},
    frame::{
        ExecutionFrame, ReturnDestination,
        arguments::FrameArguments,
        cursor::kernel::RegionError,
        transfer::ReturnValue,
        values::{FrameAllocationError, FrameMetadata, FrameSlots},
    },
    module::{
        ModuleStore, execution::calls::PreparedCallTarget, linked_execution::calls::LinkedCall,
    },
    resource::ResourceState,
    session::{ExecutionPhase, SessionState},
};
use kagari_bytecode::module::CallableTarget;

/// All coordinates are reopened after growth. Nothing in this interval can
/// publish code, collect, invoke Host code or observe a partially entered frame.
pub(super) struct ClosedFrames<'control, 'source, 'gc> {
    pub(super) frames: &'control mut Vec<ExecutionFrame>,
    pub(super) slots: &'control mut Slots<'source, 'gc>,
    pub(super) session: &'control SessionState,
    pub(super) resources: &'control ResourceState,
    pub(super) modules: &'control ModuleStore,
    pub(super) base: usize,
}

impl ClosedFrames<'_, '_, '_> {
    fn allowed(&self) -> bool {
        self.session.options.phase == ExecutionPhase::Ordinary
            && !self.session.observer_attached.get()
            && !self.resources.native_stack_active()
    }

    /// False leaves ordinary call admission entirely unchanged. Eligibility
    /// uses the refreshed binding, never the original bytecode target alone.
    pub(super) fn try_call(&mut self, index: usize) -> Result<bool, RegionError> {
        if !self.allowed() {
            return Ok(false);
        }
        let caller = self.frames.last().expect("admitted script frame");
        if caller.native_origin.is_some() {
            return Ok(false);
        }
        let CallableTarget::Script(caller_function) = caller.target else {
            return Ok(false);
        };
        let owner = &caller.loaded;
        let caller_slots = caller.slots();
        let function = &owner.execution().functions[caller_function.index()];
        let call = &function.calls[index];
        let PreparedCallTarget::Static {
            module,
            target: CallableTarget::Script(target),
        } = call.target
        else {
            return Ok(false);
        };
        if module != owner.slot()
            || !caller
                .links
                .as_ref()
                .and_then(|links| links.call_contract(index))
                .and_then(LinkedCall::direct)
                .is_some_and(|binding| {
                    binding
                        .selected
                        .selects(owner.key(), CallableTarget::Script(target))
                })
            || owner.is_static_initializer(target)
        {
            return Ok(false);
        }
        if !self
            .modules
            .contains_module(owner)
            .map_err(|_| RuntimeError::from(EngineFault::ModuleStoreIsBorrowedAcrossExecution))?
        {
            return Ok(false);
        }
        let callee = &owner.execution().functions[target.index()];
        if callee.needs_runtime_links()
            || callee.frame.generic
            || callee.frame.may_suspend
            || callee.frame.slot_layout.managed_count != 0
        {
            return Ok(false);
        }
        // Pin only the accepted callee across possible frame-Vec relocation.
        // Native/semantic fallback must not acquire another Program reference.
        let owner = owner.clone();
        let call = &owner.execution().functions[caller_function.index()].calls[index];
        let callee = &owner.execution().functions[target.index()];
        // Control growth precedes depth entry, just as in ordinary admission.
        // No reference into the frame Vec survives this operation.
        space::reserve(self.frames, 1, &self.session.space).map_err(RegionError::storage)?;
        self.resources.enter_call_closed()?;
        #[cfg(feature = "execution-diagnostics")]
        {
            diagnostics::record(Event::CallSelection);
            diagnostics::record(Event::CallAdmission);
        }
        let storage = (|| {
            let body = self.slots.dependency(caller_slots.body)?;
            FrameSlots::allocate(
                self.slots,
                self.session.id,
                &callee.frame.slot_layout,
                &callee.frame.parameter_slots,
                FrameArguments::frame(caller_slots, &call.arguments),
                FrameMetadata {
                    body,
                    inherited_body: Some(caller_slots.body),
                    dependencies: &[],
                },
                self.resources,
            )
        })();
        let storage = match storage {
            Ok(storage) => storage,
            Err(error) => {
                self.resources.leave_call();
                return Err(match error {
                    FrameAllocationError::Storage(error) => RegionError::storage(error),
                    FrameAllocationError::Execution(error) => error.into(),
                });
            }
        };
        let destination = ReturnDestination::Prepared(call.destination);
        self.frames.push(ExecutionFrame::from_storage(
            owner,
            CallableTarget::Script(target),
            storage,
            destination,
        ));
        Ok(true)
    }

    /// Publish through the shared return transaction before retiring any callee
    /// storage. The caller's Program pins the leaf's code/layout during Drop.
    pub(super) fn try_return(&mut self, packet: &ReturnValue) -> Result<bool, RegionError> {
        if self.frames.len() <= self.base + 1 || !packet.is_scalar() || !self.allowed() {
            return Ok(false);
        }
        let index = self.frames.len() - 1;
        let callee = &self.frames[index];
        let caller = &self.frames[index - 1];
        let ReturnDestination::Prepared(destination) = callee.return_to else {
            return Ok(false);
        };
        if callee.environment.is_some()
            || callee.uncommon.is_some()
            || callee.links.is_some()
            || callee.native_origin.is_some()
            || callee.slots().banks.managed_count != 0
            || !matches!(callee.target, CallableTarget::Script(_))
            || !matches!(caller.target, CallableTarget::Script(_))
            || caller.native_origin.is_some()
            || callee.loaded.program_identity() != caller.loaded.program_identity()
        {
            return Ok(false);
        }
        // Ordinary finish_return polls at this exact point, while the callee's
        // completed Return remains observable on cancellation.
        self.resources.poll_closed()?;
        let storage = callee.storage.as_ref().expect("live return frame");
        packet
            .publish_to_caller(
                self.slots,
                storage.mark(),
                caller.slots().banks,
                destination,
            )
            .map_err(RegionError::storage)?;
        self.frames[index]
            .storage
            .take()
            .expect("checked return storage")
            .retire(self.slots)
            .map_err(RegionError::storage)?;
        let retired = self.frames.pop().expect("checked return frame");
        debug_assert!(retired.uncommon.is_none());
        drop(retired);
        self.resources.leave_call();
        if destination.is_some() {
            let caller = self.frames.last_mut().expect("retained caller");
            caller.completed = caller.executing;
        }
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::CallReturn);
        Ok(true)
    }
}
