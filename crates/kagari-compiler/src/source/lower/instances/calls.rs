//! Plan ordinary calls into canonical shared entries when caller types are symbolic.
use crate::source::lower::{MirLoweringError, instances::InstancePlanner};
use kagari_common::{identity::table::DefinitionId, span::Span};
use kagari_contract::{callable::shared::SharedCall, types::ConcreteFunctionIdentity};
use kagari_hir::{
    aggregates::traits::MethodDefault,
    callable::AppliedCallSignature,
    resolver::resolved::ResolvedName,
    semantic::signatures::FunctionImplementation,
    types::{GenericParameterType, TypeId, TypeSubstitution},
};
use kagari_mir::instruction::{CallTarget, SourceFunctionContract};
use kagari_types::callable::{CallableImplementation, Signature};
use std::slice;

impl InstancePlanner<'_> {
    /// Plan a canonical direct call after substituting the caller's type arguments.
    /// Applied parameter/result facts remain authoritative for native projections.
    pub(crate) fn direct_call(
        &mut self,
        declaration: DefinitionId,
        arguments: &[TypeId],
        application: &AppliedCallSignature,
        substitution: &TypeSubstitution,
        span: Span,
    ) -> Result<CallTarget, MirLoweringError> {
        let arguments = self.arguments(arguments, substitution, span)?;
        let params = self.arguments(&application.params, substitution, span)?;
        let result = self
            .arguments(
                slice::from_ref(&application.return_type),
                substitution,
                span,
            )?
            .remove(0);
        if arguments.iter().any(|ty| !ty.is_concrete()) {
            let signature = Signature {
                params: params.iter().map(|ty| self.encode_type(ty)).collect(),
                result: self.encode_type(&result),
            };
            return Ok(CallTarget::Shared(Box::new(self.shared_call(
                &declaration,
                &arguments,
                signature,
                span,
            )?)));
        }
        if self.native_function(&declaration).is_some() {
            let applied = AppliedCallSignature {
                params,
                return_type: result,
            };
            return Ok(CallTarget::Native(Box::new(self.checked_native_import(
                declaration,
                &applied,
                &arguments,
                span,
            )?)));
        }
        let module = self
            .modules
            .get(&self.module_handle(declaration))
            .ok_or(MirLoweringError::MissingBinding("direct function module"))?;
        if let Some(ResolvedName::Function(id)) = module
            .environment()
            .declarations()
            .definition_target(&declaration)
        {
            let signature = module
                .environment()
                .signatures()
                .function(id)
                .ok_or(MirLoweringError::MissingTypedFunction(id))?;
            if signature.implementation != FunctionImplementation::Script {
                return Err(MirLoweringError::MissingBinding(
                    "unimplemented direct callable",
                ));
            }
        } else if !self
            .catalog
            .default_method(&self.definitions(), &declaration)
            .is_some_and(|(_, method)| method.default == Some(MethodDefault::Script))
        {
            return Err(MirLoweringError::MissingBinding(
                "direct function declaration",
            ));
        }
        if self.module_handle(declaration) == self.owner_handle() {
            Ok(CallTarget::Function(self.enqueue_declaration(
                &declaration,
                arguments,
                span,
            )?))
        } else {
            Ok(CallTarget::SourceFunction(Box::new(
                SourceFunctionContract {
                    declaration,
                    arguments: arguments.iter().map(|ty| self.encode_type(ty)).collect(),
                    params: params
                        .iter()
                        .map(|ty| self.value_type(ty, &Default::default(), span))
                        .collect::<Result<_, _>>()?,
                    return_type: self.value_type(&result, &Default::default(), span)?,
                },
            )))
        }
    }

    pub(crate) fn shared_call(
        &mut self,
        declaration: &DefinitionId,
        arguments: &[TypeId],
        signature: Signature,
        span: Span,
    ) -> Result<SharedCall, MirLoweringError> {
        let invalid = || MirLoweringError::MissingBinding("shared function declaration");
        let module = self
            .modules
            .get(&self.module_handle(*declaration))
            .ok_or_else(invalid)?;
        let Some(ResolvedName::Function(id)) = module
            .environment()
            .declarations()
            .definition_target(declaration)
        else {
            return Err(invalid());
        };
        let function = module
            .environment()
            .signatures()
            .functions()
            .iter()
            .find(|function| function.id == id)
            .ok_or_else(invalid)?
            .clone();
        if arguments.len() != function.generic_params.len() || arguments.is_empty() {
            return Err(invalid());
        }
        let canonical: Vec<_> = function
            .generic_params
            .iter()
            .enumerate()
            .map(|(position, parameter)| {
                TypeId::Generic(GenericParameterType {
                    owner: *declaration,
                    position,
                    name: parameter.name.clone(),
                })
            })
            .collect();
        let substitution: TypeSubstitution = function
            .generic_params
            .iter()
            .cloned()
            .zip(arguments.iter().cloned())
            .collect();
        let evidence = self.bound_evidence(&function.bounds, &substitution, span)?;
        let (instance, implementation, receiver_access) =
            if self.prepare_native_target(declaration, &canonical, span)? {
                let import = self.native_target_import(declaration, &canonical, span)?;
                (
                    import.instance,
                    CallableImplementation::Native(import.binding),
                    import.receiver_access,
                )
            } else {
                if self.module_handle(*declaration) == self.owner_handle() {
                    self.enqueue_declaration(declaration, canonical, span)?;
                }
                (
                    ConcreteFunctionIdentity {
                        declaration: *declaration,
                        arguments: vec![],
                    },
                    CallableImplementation::Script,
                    None,
                )
            };
        Ok(SharedCall {
            constructions: evidence.constructions,
            receiver_access,
            instance,
            implementation,
            signature,
            operations: evidence.operations,
            arguments: arguments.iter().map(|ty| self.encode_type(ty)).collect(),
        })
    }
}
