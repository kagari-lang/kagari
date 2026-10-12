//! Interface value protocols are explicit library implementations.
use crate::catalog::{
    identities,
    key::{self, RegistrationTrait},
};
use kagari_common::identity::{DefinitionKind, map::DefinitionContext};
use kagari_types::{
    declaration::module::{DeclarationError, ImplDecl, ModuleDecl},
    ty::{GenericParam, Ty},
};

pub(super) fn declare(
    ids: &DefinitionContext,
    module: &mut ModuleDecl,
) -> Result<(), DeclarationError> {
    for (interface, arity) in [
        (RegistrationTrait::List, 1),
        (RegistrationTrait::MutableList, 1),
        (RegistrationTrait::Set, 1),
        (RegistrationTrait::MutableSet, 1),
        (RegistrationTrait::Map, 2),
        (RegistrationTrait::MutableMap, 2),
    ] {
        for (protocol, member) in [
            (RegistrationTrait::PartialEq, "eq"),
            (RegistrationTrait::Debug, "debug"),
        ] {
            let binding = format!("$foundation_interface_{}_{member}", interface.name());
            let owner = identities::implementation(ids, module.implementations.len())?;
            let parameters: Vec<_> = (0..arity)
                .map(|position| GenericParam { owner, position })
                .collect();
            let receiver = Ty::Trait(key::applied(
                ids,
                interface,
                parameters.iter().map(GenericParam::as_type).collect(),
            )?);
            let contract = module
                .traits
                .iter()
                .find(|item| item.name == protocol.name())
                .expect("declared protocol")
                .clone();
            module.implement_trait(
                &contract,
                ImplDecl {
                    generic_params: parameters,
                    bounds: vec![],
                    trait_type: Some(key::applied(ids, protocol, vec![])?),
                    for_type: receiver,
                    methods: vec![],
                },
                &[(
                    member,
                    identities::definition(ids, DefinitionKind::Function, &binding)?,
                )],
                ids,
            )?;
        }
    }
    Ok(())
}
