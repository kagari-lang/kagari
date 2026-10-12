//! Select receiver comparison while the checked source catalog is available.
use crate::source::lower::{MirLoweringError, instances::InstancePlanner};
use kagari_common::{identity::DefinitionKind, span::Span};
use kagari_contract::callable::{
    receiver::{ReceiverComparison, object_receiver},
    witness::OperationWitness,
};
use kagari_hir::{
    aggregates::implementations::ImplementationSearchLimits,
    language::semantics::ProtocolSemantics, typeck::GenericBounds, types::TypeId,
};
use kagari_types::{declaration::requirement::NativeCallableRequirement, language::Protocol};

impl InstancePlanner<'_> {
    pub(super) fn receiver_comparison(
        &mut self,
        receiver: &TypeId,
        assumptions: &GenericBounds,
        span: Span,
    ) -> Result<Option<ReceiverComparison>, MirLoweringError> {
        let ty = self.encode_type(receiver);
        if !object_receiver(&ty) {
            return Ok(None);
        }
        let definitions = self.definitions();
        let interface = Protocol::PartialEq
            .nominal(&definitions)
            .ok_or(MirLoweringError::MissingBinding("object equality protocol"))?;
        if !ty.is_concrete()
            && self
                .catalog
                .concrete_interface_implementation(
                    &definitions,
                    &interface,
                    receiver,
                    assumptions,
                    ImplementationSearchLimits {
                        max_checks: 100_000,
                        max_depth: 64,
                    },
                    &self.options.cancel,
                )
                .map_err(|_| MirLoweringError::MissingBinding("receiver comparison bounds"))?
                .is_none()
        {
            return Ok(None);
        }
        let member = definitions
            .lookup_child(interface.declaration, DefinitionKind::Method, "eq", 0)
            .ok_or(MirLoweringError::MissingBinding("object equality member"))?;
        let required = NativeCallableRequirement {
            receiver: ty,
            interface: self.encode_nominal(&interface),
            member,
            arguments: vec![],
        };
        let operation = self.bind_operations(vec![required], span)?.pop().ok_or(
            MirLoweringError::MissingBinding("selected receiver comparison"),
        )?;
        // A proved implementation retains receiver binders and bound operations
        // in its checked shared entry. Unproved conditional/default selections still
        // need concrete demands; an unconstrained template cannot forward them.
        if matches!(operation, OperationWitness::Forward(_)) {
            return Ok(None);
        }
        Ok(Some(ReceiverComparison { operation }))
    }
}
