//! Materialize native method contracts while the checked source catalog is available.
use crate::source::{
    lower::{MirLoweringError, instances::InstancePlanner},
    types::raise_type,
};
use kagari_abi::{
    callable::{CallableImplementation, generic::GenericBody},
    native_import::{NativeImport, NativeSignature},
    types::{ConcreteFunctionIdentity, GenericParameterAbi, substitution::TypeSubstitution},
};
use kagari_common::{identity::DefinitionId, span::Span};
use kagari_hir::types::{TypeId, abi::lower_type};

impl InstancePlanner<'_> {
    pub(crate) fn prepare_native_target(
        &mut self,
        declaration: &DefinitionId,
        arguments: &[TypeId],
        span: Span,
    ) -> Result<bool, MirLoweringError> {
        let Some(declared) = self.registered_native_declaration(declaration).cloned() else {
            return self.prepare_default_target(declaration, arguments, span);
        };
        let invalid = || MirLoweringError::MissingBinding("checked native method instance");
        // All declared parameters must be supplied, either concretely or by a
        // shared entry's explicitly scoped method binders.
        if declared.function.generic_params.len() != arguments.len() {
            return Ok(false);
        }
        let mut import = self.native_target_import(declaration, arguments, span)?;
        let instance = import.instance.clone();
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
        import.callables = self.native_callables(&mut import, span)?;
        if !import.structurally_valid() {
            return Err(invalid());
        }
        self.native_targets.push(import);
        Ok(true)
    }

    pub(super) fn native_target_import(
        &self,
        declaration: &DefinitionId,
        arguments: &[TypeId],
        span: Span,
    ) -> Result<NativeImport, MirLoweringError> {
        let declared = self.registered_native_declaration(declaration).ok_or(
            MirLoweringError::MissingBinding("registered native template"),
        )?;
        if declared.function.generic_params.len() != arguments.len() {
            return Err(MirLoweringError::MissingBinding(
                "native template arguments",
            ));
        }
        let invalid = || MirLoweringError::MissingBinding("checked native template application");
        let CallableImplementation::Native(binding) = &declared.function.implementation else {
            return Err(invalid());
        };
        let arguments = self.arguments(arguments, &Default::default(), span)?;
        let instance = ConcreteFunctionIdentity {
            declaration: declaration.clone(),
            arguments: arguments.iter().map(lower_type).collect(),
        };
        let parameters: Vec<_> = arguments
            .iter()
            .filter_map(|argument| {
                let TypeId::Generic(parameter) = argument else {
                    return None;
                };
                Some(GenericParameterAbi {
                    owner: parameter.owner.clone(),
                    position: parameter.position,
                })
            })
            .collect();
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
        let requirements = substitution
            .apply_bounds(&declared.function.bounds, &self.options.cancel)
            .map_err(|_| invalid())?;
        let import = NativeImport {
            result_adapter: None,
            generic: (!parameters.is_empty()).then(|| GenericBody {
                parameters,
                bounds: requirements.clone(),
            }),
            signature: NativeSignature {
                params: declared
                    .function
                    .params
                    .iter()
                    .map(|param| normalize(&param.ty))
                    .collect::<Result<_, MirLoweringError>>()?,
                result: normalize(&declared.function.return_type)?,
            },
            requirements,
            instance,
            binding: binding.clone(),
            host: None,
            callables: vec![],
        };
        Ok(import)
    }
}
