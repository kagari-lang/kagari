//! Checked implementation sources share pattern matching without invented tables.
use crate::{
    callable::CallableImplementation,
    native_api::NativeImplementation,
    types::{
        AbiType, AssociatedTypeFamilyAbi, FunctionAbi, GenericBoundAbi, GenericParameterAbi,
        InterfaceTableAbi, NominalAbiType, TraitAbi,
        matching::ImplementationPattern,
        substitution::{TypeSubstitution, TypeTransformError, resolve_associated_outputs},
    },
};
use kagari_common::{cancellation::CancellationToken, identity::DefinitionId};

/// Source-independent facts from a verified interface or validated registration.
/// The caller owns declaration/method validation; proof checks applicability.
pub enum Implementation<'a> {
    Interface(&'a InterfaceTableAbi),
    Native {
        declaration: &'a DefinitionId,
        implementation: &'a NativeImplementation,
    },
}

impl<'a> From<&'a InterfaceTableAbi> for Implementation<'a> {
    fn from(table: &'a InterfaceTableAbi) -> Self {
        Self::Interface(table)
    }
}

impl<'a> Implementation<'a> {
    pub(super) fn declaration(&self) -> &DefinitionId {
        match self {
            Self::Interface(table) => &table.declaration,
            Self::Native { declaration, .. } => declaration,
        }
    }
    pub(super) fn parameters(&self) -> &'a [GenericParameterAbi] {
        match self {
            Self::Interface(table) => &table.generic_params,
            Self::Native { implementation, .. } => &implementation.generic_params,
        }
    }
    pub(super) fn bounds(&self) -> &'a [GenericBoundAbi] {
        match self {
            Self::Interface(table) => &table.bounds,
            Self::Native { implementation, .. } => &implementation.bounds,
        }
    }
    pub(super) fn receiver(&self) -> &'a AbiType {
        match self {
            Self::Interface(table) => &table.for_type,
            Self::Native { implementation, .. } => &implementation.for_type,
        }
    }
    pub(super) fn interface(&self) -> Option<&'a NominalAbiType> {
        match self {
            Self::Interface(table) => match &table.trait_type {
                AbiType::Trait(interface) => Some(interface),
                _ => None,
            },
            Self::Native { implementation, .. } => implementation.trait_type.as_ref(),
        }
    }
    pub(super) fn pattern(&self) -> Option<ImplementationPattern<'a>> {
        Some(ImplementationPattern {
            parameters: self.parameters(),
            receiver: self.receiver(),
            interface: self.interface()?,
        })
    }
    pub(super) fn families(&self) -> &'a [AssociatedTypeFamilyAbi] {
        match self {
            Self::Interface(table) => &table.associated_type_families,
            Self::Native { .. } => &[],
        }
    }
    pub(super) fn is_bridge(&self) -> bool {
        matches!(self, Self::Interface(table) if table.host_bridge)
    }

    pub(super) fn method(
        &self,
        name: &str,
        arguments: &[AbiType],
        contract: Option<&TraitAbi>,
        cancel: &CancellationToken,
    ) -> Result<Option<FunctionAbi>, TypeTransformError> {
        if let Self::Interface(table) = self {
            return Ok(table
                .instantiate(arguments)
                .and_then(|table| table.methods.into_iter().find(|method| method.name == name)));
        }
        let Self::Native { implementation, .. } = self else {
            unreachable!()
        };
        if arguments.len() != self.parameters().len() || !arguments.iter().all(AbiType::is_concrete)
        {
            return Err(TypeTransformError::InvalidContract);
        }
        let mut bindings = TypeSubstitution::default();
        for (parameter, argument) in self.parameters().iter().zip(arguments) {
            bindings.bind(&parameter.owner, parameter.position, argument);
        }
        let applied = bindings.apply_nominal(
            self.interface()
                .ok_or(TypeTransformError::InvalidContract)?,
            cancel,
        )?;
        let receiver = bindings.apply(self.receiver(), cancel)?;
        let mut trait_bindings =
            TypeSubstitution::for_owner(&applied.declaration, &applied.arguments);
        trait_bindings.bind_receiver(&applied.declaration, &receiver);
        let provided = implementation
            .methods
            .iter()
            .find(|method| method.name == name);
        let inherited = provided.is_none();
        let mut method = match provided {
            Some(method) => method.clone(),
            None => {
                let Some(method) = contract.and_then(|contract| {
                    contract.methods.iter().find(|method| method.name == name)
                }) else {
                    return Ok(None);
                };
                if !matches!(
                    method.implementation,
                    CallableImplementation::NativeDefault(_)
                ) {
                    return Ok(None);
                }
                method.clone()
            }
        };
        let bindings = if inherited {
            &trait_bindings
        } else {
            &bindings
        };
        let apply = |ty: &AbiType| {
            let ty = bindings.apply(ty, cancel)?;
            if inherited {
                resolve_associated_outputs(&ty, &applied, cancel)
            } else {
                Ok(ty)
            }
        };
        method
            .generic_params
            .retain(|parameter| !self.parameters().contains(parameter));
        for param in &mut method.params {
            param.ty = apply(&param.ty)?;
        }
        method.return_type = apply(&method.return_type)?;
        method.bounds = bindings.apply_bounds(&method.bounds, cancel)?;
        method.implementation = method.implementation.apply(bindings, cancel)?;
        if let CallableImplementation::NativeDefault(application) = &mut method.implementation {
            application.arguments = application
                .arguments
                .iter()
                .map(apply)
                .collect::<Result<_, _>>()?;
        }
        Ok(Some(method))
    }
}
