//! Match required registration authority against the verified executable closure.
use crate::{error::RuntimeError, module::VerifiedProgram, native::catalog::DeclarationCatalog};
use kagari_bytecode::module::BytecodeModule;
use kagari_common::identity::{DefinitionKind, table::DefinitionId};
use kagari_contract::types::{InterfaceTable, PublicItem, inherent::InherentTable, trait_contract};
use kagari_types::{
    declaration::{TypeDefKind, module::ImplDecl},
    language,
    language::Protocol,
    ty::Ty,
};

/// Registered value/external schemas remain exact even without callable imports.
pub(super) fn validate_installed_values(
    installed: &DeclarationCatalog,
    module: &BytecodeModule,
) -> Result<(), RuntimeError> {
    let definitions = installed.definitions();
    for (id, expected) in installed.types.entries().filter(|(_, ty)| {
        matches!(
            ty.kind,
            TypeDefKind::Struct | TypeDefKind::Enum | TypeDefKind::Host(_)
        )
    }) {
        let owner = definitions
            .resolve(id)
            .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
        if owner.module() == &module.identity
            && !module
                .public_items
                .iter()
                .any(|item| matches!(item, PublicItem::Type(actual) if actual == expected))
        {
            return Err(RuntimeError::module_validation(
                "registered value schema differs from its installed contract",
            ));
        }
    }
    for required in &module.external_types {
        if installed.types.get_id(required.declaration) != Some(&required.definition) {
            return Err(RuntimeError::module_validation(
                "external type dependency lacks its exact installed contract",
            ));
        }
    }
    Ok(())
}

/// Installed trait identities retain their exact contracts even without calls.
/// Ordinary uninstalled traits carry no native authority. Conversion adapters
/// and reserved language contracts still require exact installed declarations.
pub(super) fn validate_installed_traits(
    installed: &DeclarationCatalog,
    module: &BytecodeModule,
) -> Result<(), RuntimeError> {
    let definitions = installed.definitions();
    for item in &module.public_items {
        let PublicItem::Trait(contract) = item else {
            continue;
        };
        let reserved = Protocol::ALL.into_iter().any(|protocol| {
            protocol.module() == module.identity && protocol.name() == contract.name
        });
        let id = definitions.lookup_root(&module.identity).and_then(|root| {
            definitions.lookup_child(root, DefinitionKind::Trait, &contract.name, 0)
        });
        let registered = id.and_then(|id| installed.traits.get_id(id));
        if (registered.is_some() || reserved || contract.conversion_adapter.is_some())
            && registered != Some(contract)
        {
            return Err(RuntimeError::module_validation(
                "reserved or native trait differs from its installed contract",
            ));
        }
    }
    for contract in &module.trait_contracts {
        definitions
            .resolve(contract.declaration)
            .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
        let registered = installed.traits.get_id(contract.declaration);
        if (registered.is_some()
            || Protocol::from_definition(&contract.declaration, &definitions).is_some()
            || contract.abi.conversion_adapter.is_some())
            && registered != Some(&contract.abi)
        {
            return Err(RuntimeError::module_validation(
                "private reserved or native trait lacks an installed contract",
            ));
        }
    }
    if language::is_language_module(&module.identity) {
        for protocol in Protocol::ALL
            .into_iter()
            .filter(|protocol| protocol.module() == module.identity)
        {
            let expected = protocol
                .definition(&definitions)
                .and_then(|id| installed.traits.get_id(id))
                .ok_or_else(|| {
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
                && trait_contract(
                    &module.identity,
                    &module.public_items,
                    &module.trait_contracts,
                    &id,
                    program.definitions(),
                ) == Some(expected)
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
        if !program
            .modules()
            .iter()
            .filter(|module| &module.identity == owner.module())
            .flat_map(|module| &module.public_items)
            .any(|item| match item {
                PublicItem::InterfaceTable(actual) => implementation_matches(&id, expected, actual),
                PublicItem::InherentTable(actual) => inherent_matches(id, expected, actual),
                _ => false,
            })
        {
            return Err(RuntimeError::module_validation(
                "native dependency differs from its registered implementation contract",
            ));
        }
    }
    Ok(())
}

fn implementation_matches(id: &DefinitionId, expected: &ImplDecl, actual: &InterfaceTable) -> bool {
    // The compiler materializes omitted defaults on its real executable table.
    // Match the registered header and every explicit native method; defaults are
    // independently checked against the exact carried trait/template contracts.
    actual.declaration == *id
        && actual.bridge.is_none()
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

fn inherent_matches(id: DefinitionId, expected: &ImplDecl, actual: &InherentTable) -> bool {
    expected.trait_type.is_none()
        && actual.declaration == id
        && actual.generic_params == expected.generic_params
        && actual.bounds == expected.bounds
        && actual.for_type == expected.for_type
        && expected.methods.iter().all(|method| {
            let mut method = method.clone();
            method
                .generic_params
                .retain(|parameter| !expected.generic_params.contains(parameter));
            actual.methods.contains(&method)
        })
}

#[cfg(test)]
mod tests {
    use crate::{Runtime, RuntimeConfig};
    use kagari_bytecode::{
        module::BytecodeModule,
        program::{BytecodeProgram, ModuleRef},
    };
    use kagari_common::identity::{
        DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext,
        metadata::DefinitionMetadata,
    };
    use kagari_contract::types::PublicItem;
    use kagari_types::declaration::{
        TypeDeclaration, TypeDef, TypeDefKind,
        host::{HostReflectionPolicy, HostTypeOwnership, HostTypePolicy, PathAccess},
        module::ModuleDecl,
    };

    #[test]
    fn uninstalled_external_schema_cannot_publish_a_source_free_module() {
        let context = DefinitionContext::new().unwrap();
        let mut declaration = ModuleDecl::new(ModuleIdentity {
            package: PackageId("example".into()),
            path: vec!["roots".into()],
        });
        let definition = TypeDef {
            name: "Player".into(),
            kind: TypeDefKind::Host(HostTypePolicy {
                ownership: HostTypeOwnership::HostRoot,
                reflection: HostReflectionPolicy::Metadata,
                path_access: PathAccess::ReadWrite,
            }),
            generic_params: vec![],
            bounds: vec![],
            fields: vec![],
            variants: vec![],
        };
        declaration.types.push(definition.clone());
        declaration.collect_definitions(&context).unwrap();
        let id = declaration
            .definition(DefinitionKind::Struct, "Player", &context.snapshot())
            .unwrap();
        let program = BytecodeProgram {
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule {
                identity: declaration.identity,
                public_items: vec![PublicItem::Type(definition.clone())],
                external_types: vec![TypeDeclaration {
                    declaration: id,
                    definition,
                }],
                ..Default::default()
            }],
            exports: Default::default(),
        };
        let input =
            DefinitionMetadata::checked(context.snapshot(), program, &Default::default()).unwrap();
        let mut runtime = Runtime::new(RuntimeConfig::default());
        let error = runtime.load_program("external", input).unwrap_err();
        assert!(
            error
                .message()
                .contains("external type dependency lacks its exact installed contract"),
            "{error}"
        );
        assert_eq!(runtime.modules().loaded_count(), 0);
    }
}
