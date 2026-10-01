//! Materialize native method contracts while the checked source catalog is available.
use crate::source::{
    lower::{
        MirLoweringError,
        instances::{InstanceKey, InstancePlanner},
    },
    types::raise_type,
};
use kagari_abi::{
    callable::CallableImplementation,
    native_import::{NativeImport, NativeSignature},
    types::substitution::TypeSubstitution,
};
use kagari_common::{identity::DefinitionId, span::Span};
use kagari_hir::types::{TypeId, abi::lower_type};

impl InstancePlanner<'_> {
    pub(super) fn prepare_native_interface(
        &mut self,
        declaration: &DefinitionId,
        arguments: &[TypeId],
        span: Span,
    ) -> Result<(), MirLoweringError> {
        let Some(contract) = self.catalog.implementation_signature(declaration) else {
            // Engine/host bridges retain their already checked script adapters.
            return Ok(());
        };
        let methods = self.catalog.implementation_methods(contract);
        for method in methods {
            self.prepare_native_target(&method, arguments, span)?;
        }
        Ok(())
    }

    pub(crate) fn prepare_native_target(
        &mut self,
        declaration: &DefinitionId,
        arguments: &[TypeId],
        span: Span,
    ) -> Result<bool, MirLoweringError> {
        let Some(declared) = self.registered_native_declaration(declaration).cloned() else {
            return Ok(false);
        };
        let invalid = || MirLoweringError::MissingBinding("checked native method instance");
        if declaration.module != *self.module.lowered.source.module_identity() {
            return Err(invalid());
        }
        // Method-local generics require their own concrete call application.
        if declared.function.generic_params.len() != arguments.len() {
            return Ok(false);
        }
        let instance = InstanceKey {
            declaration: declaration.clone(),
            arguments: arguments.to_vec(),
        }
        .lower(self.options, span)?;
        let mut parent = declaration.clone();
        parent.path.pop();
        if let Some(implementation) = self.catalog.implementation_signature(&parent) {
            let count = implementation.generic_params.len();
            if arguments.len() < count {
                return Err(invalid());
            }
            self.record_interface(&parent, &arguments[..count], span)?;
        }
        if self
            .native_targets
            .iter()
            .any(|target| target.instance == instance)
        {
            return Ok(true);
        }
        if !arguments.is_empty() {
            self.charge_layout_instance(span)?;
        }
        let CallableImplementation::Native(binding) = &declared.function.implementation else {
            return Err(invalid());
        };
        let mut substitution = TypeSubstitution::default();
        for (param, argument) in declared
            .function
            .generic_params
            .iter()
            .zip(&instance.arguments)
        {
            substitution.bind(&param.owner, param.position, argument);
        }
        let normalize = |ty| {
            let ty = substitution
                .apply(ty, &self.options.cancel)
                .map_err(|_| invalid())?;
            Ok(lower_type(&self.catalog.normalize_type(&raise_type(&ty))))
        };
        let mut import = NativeImport {
            signature: NativeSignature {
                params: declared
                    .function
                    .params
                    .iter()
                    .map(|param| normalize(&param.ty))
                    .collect::<Result<_, MirLoweringError>>()?,
                result: normalize(&declared.function.return_type)?,
            },
            requirements: substitution
                .apply_bounds(&declared.function.bounds, &self.options.cancel)
                .map_err(|_| invalid())?,
            instance,
            binding: binding.clone(),
            host: None,
            callables: vec![],
        };
        import.callables = self.native_callables(&import, span)?;
        if !import.structurally_valid() {
            return Err(invalid());
        }
        self.native_targets.push(import);
        Ok(true)
    }
}
