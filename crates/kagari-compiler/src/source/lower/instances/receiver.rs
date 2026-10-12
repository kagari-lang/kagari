//! Select receiver comparison while the checked source catalog is available.
use crate::source::lower::{MirLoweringError, abi::checked_bounds, instances::InstancePlanner};
use kagari_common::{identity::DefinitionKind, span::Span};
use kagari_contract::callable::{
    receiver::{ReceiverComparison, object_receiver},
    witness::OperationWitness,
};
use kagari_hir::{
    aggregates::implementations::ImplementationSearchLimits,
    language::semantics::ProtocolSemantics, typeck::GenericBounds, types::TypeId,
};
use kagari_types::{
    declaration::requirement::NativeCallableRequirement, language::Protocol,
    ty::substitution::TypeSubstitution,
};

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
        let conditional = !ty.is_concrete()
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
                .is_none();
        let member = definitions
            .lookup_child(interface.declaration, DefinitionKind::Method, "eq", 0)
            .ok_or(MirLoweringError::MissingBinding("object equality member"))?;
        let required = NativeCallableRequirement {
            receiver: ty,
            interface: self.encode_nominal(&interface),
            member,
            arguments: vec![],
        };
        let conditions = if conditional {
            let Some((declaration, arguments)) = self.operation_candidate(&required)? else {
                return Ok(None);
            };
            let implementation = self.catalog.implementation_signature(&declaration).ok_or(
                MirLoweringError::MissingBinding("conditional receiver implementation"),
            )?;
            let bounds = checked_bounds(self.catalog, &definitions, &implementation.bounds);
            let mut bindings = TypeSubstitution::default();
            // Match the exact implementation argument application. These are
            // guards, not assumptions introduced into the enclosing source body.
            let arguments = arguments
                .iter()
                .map(|argument| self.encode_type(argument))
                .collect::<Vec<_>>();
            for (parameter, argument) in implementation.generic_params.iter().zip(&arguments) {
                bindings.bind(&parameter.owner, parameter.position, argument);
            }
            bindings
                .apply_bounds(&bounds, &definitions, &self.options.cancel)
                .map_err(|_| MirLoweringError::MissingBinding("conditional receiver bounds"))?
        } else {
            vec![]
        };
        let operation = self.bind_operations(vec![required], span)?.pop().ok_or(
            MirLoweringError::MissingBinding("selected receiver comparison"),
        )?;
        // Conditional selections retain exact applicability guards. They cannot
        // execute until the actual argument eligibility is supplied separately.
        if matches!(operation, OperationWitness::Forward(_)) {
            return Ok(None);
        }
        Ok(Some(ReceiverComparison {
            operation,
            conditions,
        }))
    }
}
