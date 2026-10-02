//! Apply registered default templates using the already checked trait selection.
use crate::source::{
    lower::{MirLoweringError, instances::InstancePlanner},
    types::raise_type,
};
use kagari_abi::native_import::NativeImport;
use kagari_common::{identity::DefinitionId, span::Span};
use kagari_hir::{
    aggregates::traits::MethodDefault,
    native::NativeBinding,
    types::{GenericParameterType, NominalType, TypeId, TypeSubstitution, abi::lower_type},
};

impl InstancePlanner<'_> {
    pub(crate) fn native_default_import(
        &self,
        receiver: &TypeId,
        interface: &NominalType,
        method: &DefinitionId,
        arguments: &[TypeId],
        span: Span,
    ) -> Result<NativeImport, MirLoweringError> {
        let invalid = || MirLoweringError::MissingBinding("checked native default application");
        let signature = self.catalog.trait_method(method).ok_or_else(invalid)?;
        let Some(MethodDefault::Native(NativeBinding::Default(application))) = &signature.default
        else {
            return Err(invalid());
        };
        let parameters = interface.arguments.iter().chain(arguments).cloned();
        if signature.generic_params.len() != interface.arguments.len() + arguments.len() {
            return Err(invalid());
        }
        let mut substitution: TypeSubstitution = signature
            .generic_params
            .iter()
            .cloned()
            .zip(parameters)
            .collect();
        substitution.insert_receiver(signature.owner.clone(), receiver.clone());
        let normalize = |ty: &TypeId| {
            self.catalog.normalize_type(
                &ty.instantiate(&substitution)
                    .with_associated_types(interface),
            )
        };
        let template_arguments = application
            .arguments
            .iter()
            .map(|ty| normalize(&raise_type(ty)))
            .collect::<Vec<_>>();
        let import =
            self.native_target_import(&application.declaration, &template_arguments, span)?;
        if import.signature.params
            != signature
                .params
                .iter()
                .map(|p| lower_type(&normalize(&p.ty)))
                .collect::<Vec<_>>()
            || import.signature.result != lower_type(&normalize(&signature.return_type))
        {
            return Err(invalid());
        }
        Ok(import)
    }

    pub(super) fn prepare_default_target(
        &mut self,
        declaration: &DefinitionId,
        arguments: &[TypeId],
        span: Span,
    ) -> Result<bool, MirLoweringError> {
        let Some((implementation, method)) = self.catalog.default_method(declaration) else {
            return Ok(false);
        };
        if !matches!(
            method.default,
            Some(MethodDefault::Native(NativeBinding::Default(_)))
        ) {
            return Ok(false);
        }
        let count = implementation.generic_params.len();
        let contract = self
            .catalog
            .trait_(&method.owner)
            .ok_or(MirLoweringError::MissingBinding("native default trait"))?;
        if arguments.len() != count + method.generic_params.len() - contract.generic_params.len() {
            return Ok(false);
        }
        if arguments.len() < count {
            return Err(MirLoweringError::MissingBinding(
                "default implementation arguments",
            ));
        }
        let substitution = implementation
            .generic_params
            .iter()
            .cloned()
            .zip(arguments[..count].iter().cloned())
            .collect();
        let receiver = implementation.for_type.instantiate(&substitution);
        let interface = implementation.trait_type.instantiate(&substitution);
        let import = self.native_default_import(
            &receiver,
            &interface,
            &method.id,
            &arguments[count..],
            span,
        )?;
        let owner = implementation.id.clone();
        self.record_interface(&owner, &arguments[..count], span)?;
        let shared = import.instance.arguments.iter().any(|ty| !ty.is_concrete());
        let arguments = import
            .instance
            .arguments
            .iter()
            .enumerate()
            .map(|(position, ty)| {
                if shared {
                    TypeId::Generic(GenericParameterType {
                        owner: import.instance.declaration.clone(),
                        position,
                        name: format!("T{position}"),
                    })
                } else {
                    raise_type(ty)
                }
            })
            .collect::<Vec<_>>();
        self.prepare_native_target(&import.instance.declaration, &arguments, span)
    }
}
