//! Checked implementation sources share pattern matching without invented tables.
use kagari_common::{
    cancellation::CancellationToken,
    identity::table::{DefinitionId, DefinitionTable},
};

use {
    crate::types::InterfaceTable,
    kagari_types::{
        callable::CallableImplementation,
        declaration::{
            AssociatedTypeFamily, FnDecl, TraitDef, module::ImplDecl, verify::types_in_scope,
        },
        language::Protocol,
        ty::{
            GenericBound, GenericParam, NominalTy, Ty,
            matching::ImplementationPattern,
            substitution::{TypeSubstitution, TypeTransformError, resolve_associated_outputs},
        },
    },
};

/// Source-independent facts from a verified interface or validated registration.
/// The caller owns declaration/method validation; proof checks applicability.
pub enum Implementation<'a> {
    Interface(&'a InterfaceTable<DefinitionId>),
    Native {
        declaration: &'a DefinitionId,
        implementation: &'a ImplDecl<DefinitionId>,
    },
}

impl<'a> From<&'a InterfaceTable<DefinitionId>> for Implementation<'a> {
    fn from(table: &'a InterfaceTable<DefinitionId>) -> Self {
        Self::Interface(table)
    }
}

impl<'a> Implementation<'a> {
    pub(super) fn methods(&self) -> &[FnDecl<DefinitionId>] {
        match self {
            Self::Interface(table) => &table.methods,
            Self::Native { implementation, .. } => &implementation.methods,
        }
    }

    pub(super) fn declaration(&self) -> &DefinitionId {
        match self {
            Self::Interface(table) => &table.declaration,
            Self::Native { declaration, .. } => declaration,
        }
    }

    pub(super) fn parameters(&self) -> &'a [GenericParam<DefinitionId>] {
        match self {
            Self::Interface(table) => &table.generic_params,
            Self::Native { implementation, .. } => &implementation.generic_params,
        }
    }

    pub(super) fn bounds(&self) -> &'a [GenericBound<DefinitionId>] {
        match self {
            Self::Interface(table) => &table.bounds,
            Self::Native { implementation, .. } => &implementation.bounds,
        }
    }

    pub(super) fn receiver(&self) -> &'a Ty<DefinitionId> {
        match self {
            Self::Interface(table) => &table.for_type,
            Self::Native { implementation, .. } => &implementation.for_type,
        }
    }

    pub(super) fn interface(&self) -> Option<&'a NominalTy<DefinitionId>> {
        match self {
            Self::Interface(table) => match &table.trait_type {
                Ty::Trait(interface) => Some(interface),
                _ => None,
            },
            Self::Native { implementation, .. } => implementation.trait_type.as_ref(),
        }
    }

    pub(super) fn pattern(
        &self,
        contract: Option<&TraitDef<DefinitionId>>,
        definitions: &DefinitionTable,
    ) -> Option<ImplementationPattern<'a>> {
        Some(ImplementationPattern {
            protocol: Protocol::from_reference(&self.interface()?.declaration, Some(definitions)),
            storage_access: contract.and_then(|contract| contract.storage_access),
            parameters: self.parameters(),
            receiver: self.receiver(),
            interface: self.interface()?,
        })
    }

    pub(super) fn families(&self) -> &'a [AssociatedTypeFamily<DefinitionId>] {
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
        arguments: &[Ty<DefinitionId>],
        contract: Option<&TraitDef<DefinitionId>>,
        definitions: &DefinitionTable,
        cancel: &CancellationToken,
    ) -> Result<Option<FnDecl<DefinitionId>>, TypeTransformError> {
        self.method_in(name, arguments, contract, &[], definitions, cancel)
    }

    pub(super) fn method_in(
        &self,
        name: &str,
        arguments: &[Ty<DefinitionId>],
        contract: Option<&TraitDef<DefinitionId>>,
        parameters: &[GenericParam<DefinitionId>],
        definitions: &DefinitionTable,
        cancel: &CancellationToken,
    ) -> Result<Option<FnDecl<DefinitionId>>, TypeTransformError> {
        if let Self::Interface(table) = self {
            return Ok(table
                .instantiate(arguments, parameters, definitions)
                .and_then(|table| table.methods.into_iter().find(|method| method.name == name)));
        }
        let Self::Native { implementation, .. } = self else {
            unreachable!()
        };
        if arguments.len() != self.parameters().len()
            || !types_in_scope(arguments, parameters, cancel, definitions)
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
        let apply = |ty: &Ty<DefinitionId>| {
            let ty = bindings.apply(ty, cancel)?;
            if inherited {
                resolve_associated_outputs(&ty, &applied, &receiver, cancel)
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
        method.bounds = bindings.apply_bounds(&method.bounds, definitions, cancel)?;
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
