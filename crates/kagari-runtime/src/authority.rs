use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    host::{self, HostFunction},
    session::ExecutionPhase,
    value, value_contains_host_owned_data,
};
use kagari_common::{capability::CapabilitySet, host_interface::HostPassingStyle};

impl Runtime {
    pub(super) fn validate_host_path_exposure(
        &self,
        descriptor_id: host::HostPathDescriptorId,
        operation: host::HostPathOperation,
    ) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        self.reject_candidate_external_access()?;
        let Some(descriptor) = self.host.path_descriptor(descriptor_id) else {
            return Err(RuntimeError::typed_path_validation(
                "path descriptor is not registered",
            ));
        };
        let Some(root_type) = self.host.host_type(descriptor.root_type) else {
            return Err(RuntimeError::typed_path_validation(
                "path descriptor root type is not registered",
            ));
        };
        if !self
            .host_exposure()
            .exposes_host_type(&root_type.declaration.symbol)
        {
            return Err(RuntimeError::capability_denied(format!(
                "host type `{}`",
                root_type.declaration.symbol
            )));
        }
        if operation.writes() {
            if self.host_exposure().exposes_host_path_mutation() {
                Ok(())
            } else {
                Err(RuntimeError::capability_denied("host path mutation"))
            }
        } else if self.host_exposure().exposes_host_path_read() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied("host path read"))
        }
    }

    pub(super) fn validate_host_path_capabilities(
        &self,
        descriptor_id: host::HostPathDescriptorId,
    ) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        let Some(descriptor) = self.host.path_descriptor(descriptor_id) else {
            return Err(RuntimeError::typed_path_validation(
                "path descriptor is not registered",
            ));
        };
        self.validate_capabilities(descriptor.capability_requirements)
    }

    pub(super) fn validate_capabilities(
        &self,
        required: CapabilitySet,
    ) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        let granted = self.security().capabilities;
        if required.fs_read && !granted.fs_read {
            return Err(RuntimeError::capability_denied("fs_read"));
        }
        if required.fs_write && !granted.fs_write {
            return Err(RuntimeError::capability_denied("fs_write"));
        }
        if required.net && !granted.net {
            return Err(RuntimeError::capability_denied("net"));
        }
        if required.clock && !granted.clock {
            return Err(RuntimeError::capability_denied("clock"));
        }
        if required.random && !granted.random {
            return Err(RuntimeError::capability_denied("random"));
        }
        if required.host_calls && !self.security().allows_host_calls() {
            return Err(RuntimeError::capability_denied("host_calls"));
        }
        if required.path_mutation && !self.security().allows_path_mutation() {
            return Err(RuntimeError::capability_denied("path_mutation"));
        }
        if required.reflection_metadata && !self.security().allows_reflection_metadata() {
            return Err(RuntimeError::capability_denied("reflection_metadata"));
        }
        if required.reflection_read && !self.security().allows_reflection_read() {
            return Err(RuntimeError::capability_denied("reflection_read"));
        }
        if required.reflection_write && !self.security().allows_reflection_write() {
            return Err(RuntimeError::capability_denied("reflection_write"));
        }
        if required.dynamic_invocation && !self.security().allows_dynamic_invocation() {
            return Err(RuntimeError::capability_denied("dynamic_invocation"));
        }
        if required.downcast && !self.security().allows_downcast() {
            return Err(RuntimeError::capability_denied("downcast"));
        }
        if required.module_loading && !self.security().allows_module_loading() {
            return Err(RuntimeError::capability_denied("module_loading"));
        }
        if required.jit && !self.security().allows_jit() {
            return Err(RuntimeError::capability_denied("jit"));
        }
        if required.debug_attach && !self.security().allows_debug_attach() {
            return Err(RuntimeError::capability_denied("debug_attach"));
        }
        if required.debug_breakpoints && !self.security().allows_debug_breakpoints() {
            return Err(RuntimeError::capability_denied("debug_breakpoints"));
        }
        if required.debug_pause && !self.security().allows_debug_pause() {
            return Err(RuntimeError::capability_denied("debug_pause"));
        }
        if required.debug_stack_inspection && !self.security().allows_debug_stack_inspection() {
            return Err(RuntimeError::capability_denied("debug_stack_inspection"));
        }
        if required.debug_value_inspection && !self.security().allows_debug_value_inspection() {
            return Err(RuntimeError::capability_denied("debug_value_inspection"));
        }
        if required.debug_host_value_inspection
            && !self.security().allows_debug_host_value_inspection()
        {
            return Err(RuntimeError::capability_denied(
                "debug_host_value_inspection",
            ));
        }
        if required.debug_watch_evaluation && !self.security().allows_debug_watch_evaluation() {
            return Err(RuntimeError::capability_denied("debug_watch_evaluation"));
        }
        if required.debug_side_effecting_evaluation
            && !self.security().allows_debug_side_effecting_evaluation()
        {
            return Err(RuntimeError::capability_denied(
                "debug_side_effecting_evaluation",
            ));
        }
        Ok(())
    }

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
        self.resources.ensure_execution_allowed()?;
        let function = self.host.function(symbol);
        self.validate_bound_host_boundary(symbol, function)
    }

    pub(super) fn validate_bound_host_boundary(
        &self,
        symbol: &str,
        function: Option<&HostFunction>,
    ) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if !self.host_exposure().exposes_host_function(symbol) {
            return Err(RuntimeError::capability_denied(format!(
                "host function `{symbol}`"
            )));
        }
        if !self.security().allows_host_calls() {
            return Err(RuntimeError::capability_denied("host_calls"));
        }
        let Some(function) = function else {
            return Ok(());
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
        self.validate_capabilities(metadata.capability_requirements)?;
        self.resources.consume_host_call()?;
        if let Some(cost) = metadata.resource_cost_hint {
            self.resources.consume_instruction_steps(cost)?;
        }
        Ok(())
    }

    pub fn validate_reflection_metadata_boundary(&self) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if self.security().allows_reflection_metadata() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied("reflection_metadata"))
        }
    }

    pub fn validate_reflection_read_boundary(&self) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if self.security().allows_reflection_read() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied("reflection_read"))
        }
    }

    pub fn validate_reflection_write_boundary(&self) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if self.security().allows_reflection_write() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied("reflection_write"))
        }
    }

    pub fn validate_dynamic_invocation_boundary(&self) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if self.security().allows_dynamic_invocation() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied("dynamic_invocation"))
        }
    }

    pub fn validate_downcast_boundary(&self) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if self.security().allows_downcast() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied("downcast"))
        }
    }

    pub fn validate_path_mutation_boundary(&self) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        self.reject_candidate_external_access()?;
        if self.security().allows_path_mutation() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied("path_mutation"))
        }
    }

    pub fn validate_module_loading_boundary(&self) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        self.reject_candidate_external_access()?;
        if self.security().allows_module_loading() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied("module_loading"))
        }
    }

    pub fn validate_jit_boundary(&self) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if self.security().allows_jit() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied("jit"))
        }
    }

    pub fn validate_debug_attach_boundary(&self) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if self.security().allows_debug_attach() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied("debug_attach"))
        }
    }

    pub fn validate_debug_breakpoint_boundary(&self) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if self.security().allows_debug_breakpoints() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied("debug_breakpoints"))
        }
    }

    pub fn validate_debug_pause_boundary(&self) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if self.security().allows_debug_pause() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied("debug_pause"))
        }
    }

    pub fn validate_debug_stack_inspection_boundary(&self) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if self.security().allows_debug_stack_inspection() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied("debug_stack_inspection"))
        }
    }

    pub fn validate_debug_value_inspection_boundary(&self) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if self.security().allows_debug_value_inspection() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied("debug_value_inspection"))
        }
    }

    pub fn validate_debug_host_value_inspection_boundary(&self) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if self.security().allows_debug_host_value_inspection()
            && self.debug_visibility.exposes_host_values()
        {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied(
                "debug_host_value_inspection",
            ))
        }
    }

    pub fn validate_debug_watch_evaluation_boundary(&self) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if self.security().allows_debug_watch_evaluation() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied("debug_watch_evaluation"))
        }
    }

    pub fn validate_debug_side_effecting_evaluation_boundary(&self) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if self.security().allows_debug_side_effecting_evaluation() {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied(
                "debug_side_effecting_evaluation",
            ))
        }
    }

    pub fn validate_debug_module_visible(&self, module_name: &str) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        if self.debug_visibility.exposes_module(module_name) {
            Ok(())
        } else {
            Err(RuntimeError::capability_denied(format!(
                "debug module `{module_name}`"
            )))
        }
    }

    pub fn validate_debug_value_visible(&self, value: &value::Value) -> Result<(), RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        self.validate_debug_value_inspection_boundary()?;
        if !self.gc.validate_value(value) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid heap reference in debug value",
            ));
        }
        if value_contains_host_owned_data(value) {
            self.validate_debug_host_value_inspection_boundary()?;
        }
        Ok(())
    }
}
