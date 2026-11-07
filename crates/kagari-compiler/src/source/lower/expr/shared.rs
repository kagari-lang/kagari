//! Call-site adaptation for ordinary shared script and native entries.
use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_hir::{language::semantics::ProtocolSemantics, types::TypeId};
use kagari_mir::{ids::InstanceId, instruction::CallTarget};
use kagari_types::{declaration::requirement::NativeCallableRequirement, language::Protocol};

impl FunctionLowerer<'_, '_> {
    pub(super) fn shared_tuple_protocol(
        &mut self,
        function: InstanceId,
        protocol: Protocol,
        receiver: &TypeId,
    ) -> Result<CallTarget, MirLoweringError> {
        let span = self.function.debug.source_span;
        let definitions = self.planner.definitions();
        let interface = protocol
            .nominal(&definitions)
            .ok_or(MirLoweringError::MissingBinding(
                "tuple protocol declaration",
            ))?;
        let member = self.protocol_method(protocol, 0)?;
        let required = NativeCallableRequirement {
            receiver: self.planner.encode_type(receiver),
            interface: self.planner.encode_nominal(&interface),
            member,
            arguments: vec![],
        };
        Ok(CallTarget::Shared(Box::new(
            self.planner.tuple_protocol_call(function, required, span)?,
        )))
    }
}
