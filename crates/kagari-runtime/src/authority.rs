use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    session::ExecutionPhase,
    value::Value,
};
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

    pub fn validate_debug_value(&self, value: &Value) -> Result<(), RuntimeError> {
        self.resources().poll_execution()?;
        if !self.gc.validate_value(value) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid heap reference in debug value",
            ));
        }
        Ok(())
    }
}
