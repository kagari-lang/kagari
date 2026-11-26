//! Plan selected Native methods and defaults using their checked declaration contracts.
use crate::source::lower::{
    MirLoweringError,
    instances::{
        InstancePlanner,
        methods::{MethodOperation, MethodPlan},
    },
};
use kagari_common::{identity::table::DefinitionId, span::Span};
use kagari_contract::{native_import::NativeImport, types::ConcreteFunctionIdentity};
use kagari_hir::{
    aggregates::traits::MethodDefault,
    callable::AppliedCallSignature,
    types::{NominalType, TypeId, TypeSubstitution as HirSubstitution, semantic::raise_type},
};
use kagari_mir::instruction::CallTarget;
use kagari_types::callable::Signature;

impl InstancePlanner<'_> {
    fn native_method_call(
        &mut self,
        mut import: NativeImport,
        result: &TypeId,
        span: Span,
    ) -> Result<MethodPlan, MirLoweringError> {
        let target = if import.instance.arguments.iter().any(|ty| !ty.is_concrete()) {
            CallTarget::Shared(Box::new(
                self.shared_call(
                    &import.instance.declaration,
                    &import
                        .instance
                        .arguments
                        .iter()
                        .map(raise_type)
                        .collect::<Vec<_>>(),
                    import.signature,
                    span,
                )?,
            ))
        } else {
            import.callables = self.native_callables(&mut import, span)?;
            if !import.structurally_valid(&self.definitions()) {
                return Err(MirLoweringError::MissingBinding(
                    "concrete native entry application",
                ));
            }
            CallTarget::Native(Box::new(import))
        };
        Ok(MethodPlan {
            result: result.clone(),
            operation: MethodOperation::Call(target),
        })
    }

    pub(super) fn native_implementation_plan(
        &mut self,
        declaration: &DefinitionId,
        arguments: &[TypeId],
        receiver: &TypeId,
        signature: &AppliedCallSignature,
        span: Span,
    ) -> Result<MethodPlan, MirLoweringError> {
        let import = self.native_target_import(declaration, arguments, span)?;
        let has_receiver = self
            .native_declaration(declaration)
            .and_then(|declaration| declaration.function.params.first())
            .is_some_and(|parameter| parameter.name == "self");
        if import.signature.result != self.encode_type(&signature.return_type)
            || import.signature.params
                != signature
                    .params
                    .iter()
                    .map(|parameter| self.encode_type(parameter))
                    .collect::<Vec<_>>()
            || (has_receiver
                && import
                    .signature
                    .params
                    .first()
                    .is_none_or(|param| *param != self.encode_type(receiver)))
        {
            return Err(MirLoweringError::MissingBinding(
                "applied Native method signature",
            ));
        }
        self.native_method_call(import, &signature.return_type, span)
    }

    pub(super) fn native_default_plan(
        &mut self,
        receiver: &TypeId,
        interface: &NominalType,
        method: &DefinitionId,
        arguments: &[TypeId],
        span: Span,
    ) -> Result<MethodPlan, MirLoweringError> {
        let signature = self
            .catalog
            .trait_method(method)
            .ok_or(MirLoweringError::MissingBinding(
                "selected native entry default",
            ))?
            .clone();
        let Some(MethodDefault::Native(_)) = &signature.default else {
            return Err(MirLoweringError::MissingBinding(
                "native entry default implementation",
            ));
        };
        let arguments = interface
            .arguments
            .iter()
            .chain(arguments)
            .cloned()
            .collect::<Vec<_>>();
        let mut substitution: HirSubstitution = signature
            .generic_params
            .iter()
            .cloned()
            .zip(arguments.iter().cloned())
            .collect();
        substitution.insert_receiver(signature.owner, receiver.clone());
        let instantiate = |ty: &TypeId| {
            self.catalog.normalize_type(
                &self.definitions(),
                &ty.instantiate(&substitution)
                    .with_associated_types(interface, receiver),
            )
        };
        let params = signature
            .params
            .iter()
            .map(|p| instantiate(&p.ty))
            .collect::<Vec<_>>();
        let result = instantiate(&signature.return_type);
        self.native_method_call(
            NativeImport {
                receiver_access: None,
                result_adapter: None,
                generic: None,
                callables: vec![],
                instance: ConcreteFunctionIdentity {
                    declaration: *method,
                    arguments: arguments.iter().map(|ty| self.encode_type(ty)).collect(),
                },
                binding: *method,
                signature: Signature {
                    params: params.iter().map(|ty| self.encode_type(ty)).collect(),
                    result: self.encode_type(&result),
                },
                requirements: vec![],
            },
            &result,
            span,
        )
    }
}
