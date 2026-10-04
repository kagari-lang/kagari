//! Plan ordinary calls into canonical shared entries when caller types are symbolic.
use crate::source::lower::{MirLoweringError, instances::InstancePlanner};
use kagari_common::{identity::DefinitionPath, span::Span};
use kagari_contract::{
    callable::shared::SharedCall, native_import::NativeSignature, types::ConcreteFunctionIdentity,
};
use kagari_hir::{
    resolver::resolved::ResolvedName,
    types::{GenericParameterType, TypeId, TypeSubstitution, abi::lower_type},
};
use kagari_types::callable::CallableImplementation;

impl InstancePlanner<'_> {
    pub(crate) fn shared_call(
        &mut self,
        declaration: &DefinitionPath,
        arguments: &[TypeId],
        signature: NativeSignature,
        span: Span,
    ) -> Result<SharedCall, MirLoweringError> {
        let invalid = || MirLoweringError::MissingBinding("shared function declaration");
        let module = self.modules.get(&declaration.module).ok_or_else(invalid)?;
        let Some(ResolvedName::Function(id)) = module.declarations.definition_target(declaration)
        else {
            return Err(invalid());
        };
        let function = module
            .typed
            .functions
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
                    owner: declaration.clone(),
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
        let operations = self.bound_operations(&function.bounds, &substitution, span)?;
        let (instance, implementation) =
            if self.prepare_native_target(declaration, &canonical, span)? {
                let import = self.native_target_import(declaration, &canonical, span)?;
                (
                    import.instance,
                    CallableImplementation::Native(import.binding),
                )
            } else {
                if declaration.module == *self.module.lowered.source.module_identity() {
                    self.enqueue_declaration(declaration, canonical, span)?;
                }
                (
                    ConcreteFunctionIdentity {
                        declaration: declaration.clone(),
                        arguments: vec![],
                    },
                    CallableImplementation::Script,
                )
            };
        Ok(SharedCall {
            instance,
            implementation,
            signature,
            operations,
            arguments: arguments.iter().map(lower_type).collect(),
        })
    }
}
