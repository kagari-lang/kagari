//! Call-site adaptation for ordinary shared script and native entries.
use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_common::span::Span;
use kagari_contract::callable::shared::SharedCall;
use kagari_hir::{
    callable::AppliedCallSignature, declarations::DeclarationId,
    language::semantics::ProtocolSemantics, resolver::resolved::ResolvedName,
    typeck::table::CallTarget as CheckedCallTarget, types::TypeId,
};
use kagari_mir::{ids::InstanceId, instruction::CallTarget};
use kagari_types::{
    callable::{CallableImplementation, Signature},
    declaration::requirement::NativeCallableRequirement,
    language::{Protocol, adapter::adapter_contract},
};
use std::slice;

impl FunctionLowerer<'_, '_> {
    pub(super) fn shared_tuple_protocol(
        &mut self,
        function: InstanceId,
        protocol: Protocol,
        receiver: &TypeId,
        members: &[TypeId],
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
        let (_, signature) = adapter_contract(&required, &definitions)
            .ok_or(MirLoweringError::MissingBinding("tuple protocol signature"))?;
        let mut requirements = Vec::new();
        for ty in members {
            let operation = NativeCallableRequirement {
                receiver: self.planner.encode_type(ty),
                ..required.clone()
            };
            if !requirements.contains(&operation) {
                requirements.push(operation);
            }
        }
        let operations = self.planner.bind_operations(requirements, span)?;
        let instance = self.planner.instances[function.index()].key.lower(
            self.planner.catalog,
            self.planner.options,
            &definitions,
            span,
        )?;
        Ok(CallTarget::Shared(Box::new(SharedCall {
            instance,
            implementation: CallableImplementation::Script,
            arguments: members
                .iter()
                .map(|ty| self.planner.encode_type(ty))
                .collect(),
            signature,
            operations,
        })))
    }

    pub(super) fn shared_function_call(
        &mut self,
        target: &CheckedCallTarget,
        arguments: &[TypeId],
        signature: &AppliedCallSignature,
        span: Span,
    ) -> Result<CallTarget, MirLoweringError> {
        let invalid = || MirLoweringError::MissingBinding("shared call target");
        let declaration = match target {
            CheckedCallTarget::Function(id) => {
                let DeclarationId::Definition(declaration) = &self
                    .analyzed
                    .declarations
                    .target(ResolvedName::Function(*id))
                    .ok_or_else(invalid)?
                    .id
                else {
                    return Err(invalid());
                };
                *declaration
            }
            CheckedCallTarget::SourceFunction(id) => {
                self.analyzed
                    .imported_functions
                    .target(id)
                    .ok_or_else(invalid)?
                    .declaration
            }
            _ => return Err(invalid()),
        };
        let params =
            self.planner
                .arguments(&signature.params, &self.instance.substitution, span)?;
        let result = self
            .planner
            .arguments(
                slice::from_ref(&signature.return_type),
                &self.instance.substitution,
                span,
            )?
            .remove(0);
        let signature = Signature {
            params: params
                .iter()
                .map(|ty| self.planner.encode_type(ty))
                .collect(),
            result: self.planner.encode_type(&result),
        };
        Ok(CallTarget::Shared(Box::new(self.planner.shared_call(
            &declaration,
            arguments,
            signature,
            span,
        )?)))
    }
}
