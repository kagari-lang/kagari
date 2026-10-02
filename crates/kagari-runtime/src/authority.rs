use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    host::HostFunction,
    session::ExecutionPhase,
    value::Value,
};
use kagari_common::host_interface::HostPassingStyle;

impl Runtime {
    pub(super) fn is_candidate_initialization(&self) -> bool {
        self.resources
            .active_session()
            .is_some_and(|session| session.options.phase == ExecutionPhase::CandidateInitialization)
    }

    pub(super) fn reject_candidate_external_access(&self) -> Result<(), RuntimeError> {
        if self.is_candidate_initialization() {
            return Err(RuntimeError::capability_denied(
                "external state access during candidate initialization",
            ));
        }
        Ok(())
    }

    pub fn validate_host_function_boundary(&self, symbol: &str) -> Result<(), RuntimeError> {
        self.resources.poll_execution()?;
        let function = self.host.function(symbol);
        self.validate_bound_host_boundary(symbol, function)
    }

    pub(super) fn validate_bound_host_boundary(
        &self,
        symbol: &str,
        function: Option<&HostFunction>,
    ) -> Result<(), RuntimeError> {
        self.resources.poll_execution()?;
        let Some(function) = function else {
            return Err(RuntimeError::host_call_failure(format!(
                "unregistered host function `{symbol}`"
            )));
        };
        let metadata = function.declaration();
        if self.is_candidate_initialization()
            && (metadata.effects.may_call_host_services
                || metadata.effects.may_mutate_host_state
                || metadata.effects.may_suspend
                || metadata
                    .params
                    .iter()
                    .any(|parameter| parameter.passing != HostPassingStyle::Owned))
        {
            return Err(RuntimeError::capability_denied(
                "external host effects during candidate initialization",
            ));
        }
        Ok(())
    }

    pub fn validate_debug_value(&self, value: &Value) -> Result<(), RuntimeError> {
        self.resources.poll_execution()?;
        if !self.gc.validate_value(value) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid heap reference in debug value",
            ));
        }
        Ok(())
    }
}
