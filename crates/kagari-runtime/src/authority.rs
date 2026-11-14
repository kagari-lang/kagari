use crate::{
    Runtime,
    error::{RuntimeError, trap::TrapReason},
    session::ExecutionPhase,
    value::carriers::RootedValue,
    value_check,
};

use kagari_gc::error::GcError;
use kagari_types::declaration::host::{HostCallPolicy, HostFunctionEffects, HostPassingStyle};

impl Runtime {
    pub(super) fn is_candidate_initialization(&self) -> bool {
        self.resources()
            .active_session()
            .is_some_and(|session| session.options.phase == ExecutionPhase::CandidateInitialization)
    }

    pub(super) fn reject_candidate_external_access(&self) -> Result<(), RuntimeError> {
        if self.is_candidate_initialization() {
            return Err(RuntimeError::execution_phase_violation(
                "external state access during candidate initialization",
            ));
        }
        Ok(())
    }

    pub(crate) fn validate_native_host_policy(
        &self,
        policy: &HostCallPolicy,
    ) -> Result<(), RuntimeError> {
        self.resources().poll_execution()?;
        self.validate_external_call_effects(&policy.effects, policy.passing.iter().copied())
    }

    fn validate_external_call_effects(
        &self,
        effects: &HostFunctionEffects,
        passing: impl Iterator<Item = HostPassingStyle>,
    ) -> Result<(), RuntimeError> {
        if effects.may_suspend {
            self.reject_static_initialization("suspending native call")?;
        }
        if self.is_candidate_initialization()
            && (effects.may_call_host_services
                || effects.may_mutate_host_state
                || effects.may_suspend
                || passing
                    .into_iter()
                    .any(|style| style != HostPassingStyle::Owned))
        {
            return Err(RuntimeError::execution_phase_violation(
                "external host effects during candidate initialization",
            ));
        }
        Ok(())
    }

    /// Authenticate a retained diagnostic value in this Runtime. Invalid/foreign
    /// handles trap; shutdown, termination and candidate restrictions still apply.
    pub fn validate_debug_value(&mut self, value: &RootedValue) -> Result<(), RuntimeError> {
        self.resources.ensure_synchronous_thread()?;
        self.resources.poll_execution()?;
        self.memory
            .scope(&self.resources, |cx, _| value.get(cx).map(|_| ()))
            .map_err(|error| match error {
                GcError::ForeignHeap
                | GcError::InvalidRoot
                | GcError::Index
                | GcError::Representation => {
                    RuntimeError::from(TrapReason::InvalidHeapReferenceInDebugValue)
                }
                error => self.managed_memory_error(error),
            })?;
        value_check::candidate_root(self, value)
    }
}
