//! Select receiver comparison while the checked source catalog is available.
use crate::source::lower::{MirLoweringError, instances::InstancePlanner};
use kagari_common::{identity::DefinitionKind, span::Span};
use kagari_contract::callable::{
    receiver::{ReceiverComparison, object_receiver},
    witness::OperationWitness,
};
use kagari_hir::{language::semantics::ProtocolSemantics, types::TypeId};
use kagari_types::{declaration::requirement::NativeCallableRequirement, language::Protocol};

impl InstancePlanner<'_> {
    pub(super) fn receiver_comparison(
        &mut self,
        receiver: &TypeId,
        span: Span,
    ) -> Result<Option<ReceiverComparison>, MirLoweringError> {
        let ty = self.encode_type(receiver);
        // Shared templates are prepared from concrete demands before execution.
        if !object_receiver(&ty) || !ty.is_concrete() {
            return Ok(None);
        }
        let definitions = self.definitions();
        let interface = Protocol::PartialEq
            .nominal(&definitions)
            .ok_or(MirLoweringError::MissingBinding("object equality protocol"))?;
        let member = definitions
            .lookup_child(interface.declaration, DefinitionKind::Method, "eq", 0)
            .ok_or(MirLoweringError::MissingBinding("object equality member"))?;
        let required = NativeCallableRequirement {
            receiver: ty,
            interface: self.encode_nominal(&interface),
            member,
            arguments: vec![],
        };
        let selected = self.select_callables(vec![required], span)?.pop().ok_or(
            MirLoweringError::MissingBinding("selected receiver comparison"),
        )?;
        Ok(Some(ReceiverComparison {
            operation: OperationWitness::Selected(Box::new(selected)),
        }))
    }
}
