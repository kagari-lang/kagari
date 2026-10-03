//! Match required registration authority against the verified executable closure.
use crate::{error::RuntimeError, module::VerifiedProgram, native::catalog::DeclarationCatalog};
use kagari_abi::{
    declaration::ImplDecl,
    types::{AbiType, InterfaceTableAbi, PublicAbiItem},
};
use kagari_common::identity::table::DefinitionId;

pub(super) fn validate(
    required: &DeclarationCatalog,
    program: &VerifiedProgram,
) -> Result<(), RuntimeError> {
    if required.definitions().id() != program.definitions().id() {
        return Err(RuntimeError::module_validation(
            "native requirements belong to a different identity context",
        ));
    }
    for (id, expected) in required.types.entries() {
        let owner = program
            .definitions()
            .resolve(id)
            .map_err(|cause| RuntimeError::module_validation(cause.to_string()))?;
        if !program.modules().iter().any(|module| &module.identity == owner.module() && module.public_items.iter().any(|item| matches!(item, PublicAbiItem::Type(declaration) if declaration == expected))) {
            return Err(RuntimeError::module_validation("native storage type differs from its registered contract"));
        }
    }
    for (id, expected) in required.traits.entries() {
        let owner = program
            .definitions()
            .resolve(id)
            .map_err(|cause| RuntimeError::module_validation(cause.to_string()))?;
        if !program.modules().iter().any(|module| {
            &module.identity == owner.module()
                && module.public_items.iter().any(
                    |item| matches!(item, PublicAbiItem::Trait(contract) if contract == expected),
                )
        }) {
            return Err(RuntimeError::module_validation(format!(
                "native dependency {} differs from its registered trait contract",
                expected.name
            )));
        }
    }
    for (id, expected) in required.declarations.entries() {
        let owner = program
            .definitions()
            .resolve(id)
            .map_err(|cause| RuntimeError::module_validation(cause.to_string()))?;
        if !program.modules().iter().any(|module| {
            &module.identity == owner.module()
                && module
                    .native_declarations
                    .iter()
                    .any(|declaration| declaration == expected)
        }) {
            return Err(RuntimeError::module_validation(
                "native dependency differs from its registered template contract",
            ));
        }
    }
    for (id, expected) in required.implementations.entries() {
        let owner = program
            .definitions()
            .resolve(id)
            .map_err(|cause| RuntimeError::module_validation(cause.to_string()))?;
        if !program.modules().iter().filter(|module| &module.identity == owner.module())
            .flat_map(|module| &module.public_items).any(|item| {
                matches!(item, PublicAbiItem::InterfaceTable(actual) if implementation_matches(&id, expected, actual))
            }) {
            return Err(RuntimeError::module_validation("native dependency differs from its registered implementation contract"));
        }
    }
    Ok(())
}

fn implementation_matches(
    id: &DefinitionId,
    expected: &ImplDecl<DefinitionId>,
    actual: &InterfaceTableAbi<DefinitionId>,
) -> bool {
    // The compiler materializes omitted defaults on its real executable table.
    // Match the registered header and every explicit native method; defaults are
    // independently checked against the exact carried trait/template contracts.
    actual.declaration == *id
        && !actual.host_bridge
        && actual.generic_params == expected.generic_params
        && actual.bounds == expected.bounds
        && actual.for_type == expected.for_type
        && actual.associated_type_families.is_empty()
        && actual.associated_consts.is_empty()
        && matches!(&actual.trait_type, AbiType::Trait(interface) if Some(interface) == expected.trait_type.as_ref())
        && expected.methods.iter().all(|method| {
            let mut method = method.clone();
            method
                .generic_params
                .retain(|parameter| !expected.generic_params.contains(parameter));
            actual.methods.contains(&method)
        })
}
