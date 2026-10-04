//! Match required registration authority against the verified executable closure.
use crate::{error::RuntimeError, module::VerifiedProgram, native::catalog::DeclarationCatalog};
use kagari_bytecode::module::BytecodeModule;
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, table::DefinitionId,
};
use kagari_contract::types::{InterfaceTable, PublicItem};
use kagari_types::{declaration::module::ImplDecl, language, language::Protocol, ty::Ty};

/// Native storage/conversion capabilities are installation facts even without calls.
/// A portable script cannot grant itself identity/storage semantics by forging a
/// trait record or changing an installed readonly interface into a mutable one.
pub(super) fn validate_installed_traits(
    installed: &DeclarationCatalog,
    module: &BytecodeModule<DefinitionId>,
) -> Result<(), RuntimeError> {
    for item in &module.public_items {
        let PublicItem::Trait(contract) = item else {
            continue;
        };
        let id = DefinitionPath {
            module: module.identity.clone(),
            path: vec![DefinitionPathSegment {
                kind: DefinitionKind::Trait,
                name: contract.name.clone(),
                occurrence: 0,
            }],
        };
        if (Protocol::from_id(&id).is_some()
            || contract.storage_access.is_some()
            || contract.conversion_adapter.is_some())
            && installed.traits.get(&id) != Some(contract)
        {
            return Err(RuntimeError::module_validation(
                "reserved or native trait differs from its installed contract",
            ));
        }
    }
    for contract in &module.trait_contracts {
        let path = installed
            .definitions()
            .resolve(contract.declaration)
            .map_err(|error| RuntimeError::module_validation(error.to_string()))?
            .to_path();
        if (Protocol::from_id(&path).is_some()
            || contract.abi.storage_access.is_some()
            || contract.abi.conversion_adapter.is_some())
            && installed.traits.get_id(contract.declaration) != Some(&contract.abi)
        {
            return Err(RuntimeError::module_validation(
                "private reserved or native trait lacks an installed contract",
            ));
        }
    }
    if language::is_language_module(&module.identity) {
        for protocol in Protocol::ALL
            .into_iter()
            .filter(|protocol| language::identity(*protocol).module == module.identity)
        {
            let id = language::identity(protocol);
            let expected = installed.traits.get(&id).ok_or_else(|| {
                RuntimeError::module_validation("language foundation is not installed")
            })?;
            if !module
                .public_items
                .iter()
                .any(|item| matches!(item, PublicItem::Trait(actual) if actual == expected))
            {
                return Err(RuntimeError::module_validation(
                    "language foundation is missing a required role",
                ));
            }
        }
    }
    Ok(())
}

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
        if !program.modules().iter().any(|module| {
            &module.identity == owner.module()
                && module.public_items.iter().any(
                    |item| matches!(item, PublicItem::Type(declaration) if declaration == expected),
                )
        }) {
            return Err(RuntimeError::module_validation(format!(
                "native type {} differs from its registered contract",
                expected.name
            )));
        }
    }
    for (id, expected) in required.traits.entries() {
        let owner = program
            .definitions()
            .resolve(id)
            .map_err(|cause| RuntimeError::module_validation(cause.to_string()))?;
        if !program.modules().iter().any(|module| {
            &module.identity == owner.module()
                && module
                    .public_items
                    .iter()
                    .any(|item| matches!(item, PublicItem::Trait(contract) if contract == expected))
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
                matches!(item, PublicItem::InterfaceTable(actual) if implementation_matches(&id, expected, actual))
            }) {
            return Err(RuntimeError::module_validation("native dependency differs from its registered implementation contract"));
        }
    }
    Ok(())
}

fn implementation_matches(
    id: &DefinitionId,
    expected: &ImplDecl<DefinitionId>,
    actual: &InterfaceTable<DefinitionId>,
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
        && matches!(&actual.trait_type, Ty::Trait(interface) if Some(interface) == expected.trait_type.as_ref())
        && expected.methods.iter().all(|method| {
            let mut method = method.clone();
            method
                .generic_params
                .retain(|parameter| !expected.generic_params.contains(parameter));
            actual.methods.contains(&method)
        })
}
