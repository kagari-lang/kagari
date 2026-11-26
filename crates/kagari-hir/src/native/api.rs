//! Lower validated Rust registrations directly, retaining their executable contracts.
pub(crate) mod preparation;

use crate::{
    hir::{
        item::{
            Item,
            function::{FunctionImplementation, NativeContract},
        },
        ty::TypeKind,
    },
    lower::LoweredModule,
    native::{
        paths::LibraryPaths,
        structured::{self, RegistrationMap},
    },
};

use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, ModuleIdentity,
        metadata::DefinitionMetadata,
        scoped::DefinitionReferences,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_source::source::SourceFile;
use kagari_types::{
    callable::CallableImplementation,
    declaration::{
        FnDecl, TypeDefKind,
        module::{DeclarationError, ModuleDecl},
    },
};
use std::{collections::BTreeSet, sync::Arc};

/// Attaches trusted registration facts to a fresh lowering, never to a copied ID map.
fn attach(
    metadata: &DefinitionMetadata<ModuleDecl>,
    paths: &LibraryPaths,
    source: Arc<SourceFile>,
    cancel: &CancellationToken,
) -> Result<LoweredModule, DeclarationError> {
    let definition = metadata.records();
    let definitions = metadata.definitions();
    let (mut lowered, allocated) = structured::lower(metadata, paths, source, cancel)?;
    attach_host_types(definition, definitions, &allocated, &mut lowered)?;
    attach_functions(definition, definitions, &allocated, &mut lowered)?;

    Ok(lowered)
}

fn attach_host_types(
    definition: &ModuleDecl,
    definitions: &DefinitionTable,
    allocated: &RegistrationMap,
    lowered: &mut LoweredModule,
) -> Result<(), DeclarationError> {
    for ty in &definition.types {
        if !matches!(ty.kind, TypeDefKind::Host(_)) {
            continue;
        }
        let declaration =
            definition.definition(ty.kind.definition_kind(), &ty.name, definitions)?;
        let Item::NativeType(id) = allocated.item(declaration)? else {
            return Err(DeclarationError(
                "registered native type kind differs".into(),
            ));
        };
        lowered.module.native_types[id.index()].is_host = true;
    }
    Ok(())
}

fn attach_functions(
    definition: &ModuleDecl,
    definitions: &DefinitionTable,
    allocated: &RegistrationMap,
    lowered: &mut LoweredModule,
) -> Result<(), DeclarationError> {
    let mut attach =
        |declaration: DefinitionId, function: &FnDecl| -> Result<(), DeclarationError> {
            if !matches!(function.implementation, CallableImplementation::Native(_)) {
                return Ok(());
            }
            let Item::Function(id) = allocated.item(declaration)? else {
                return Err(DeclarationError("registered function kind differs".into()));
            };
            lowered.module.functions[id.index()].implementation =
                FunctionImplementation::Native(NativeContract {
                    host_policy: function.host_policy.clone(),
                    callable_requirements: definition
                        .callable_requirements
                        .get(&declaration)
                        .cloned()
                        .unwrap_or_default(),
                    concrete_result: definition.concrete_results.get(&declaration).cloned(),
                });
            Ok(())
        };
    for function in &definition.functions {
        attach(
            definition.definition(DefinitionKind::Function, &function.name, definitions)?,
            function,
        )?;
    }
    for contract in &definition.traits {
        let owner = definition.definition(DefinitionKind::Trait, &contract.name, definitions)?;
        for method in &contract.methods {
            attach(
                ModuleDecl::method_id(owner, &method.name, definitions)?,
                method,
            )?;
        }
    }
    for (index, implementation) in definition.implementations.iter().enumerate() {
        let owner = definition.implementation_id(index, definitions)?;
        for method in &implementation.methods {
            attach(
                ModuleDecl::method_id(owner, &method.name, definitions)?,
                method,
            )?;
        }
    }
    Ok(())
}

pub(crate) fn dependencies(
    metadata: &DefinitionMetadata<ModuleDecl>,
    lowered: &LoweredModule,
    adopted: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<BTreeSet<ModuleIdentity>, DeclarationError> {
    let definition = metadata.records();
    let definitions = metadata.definitions();
    let mut imports = definition.dependencies.clone();
    // Structured type nodes already carry exact, adopted identities. Reading their
    // display names would create false dependency edges for same-spelled providers.
    for (_, ty) in lowered.module.nodes.types.iter() {
        cancel
            .check()
            .map_err(|_| DeclarationError("native dependency collection cancelled".into()))?;
        if let TypeKind::Definition { declaration, .. } = &ty.kind {
            let target = adopted
                .resolve(*declaration)
                .map_err(|e| DeclarationError(e.to_string()))?;
            if target.module() != &definition.identity {
                imports.insert(target.module().clone());
            }
        }
    }
    for id in definition.exports.values() {
        let target = definitions
            .resolve(*id)
            .map_err(|e| DeclarationError(e.to_string()))?;
        imports.insert(target.module().clone());
    }
    definition
        .visit_definition_ids(
            &mut |id: &DefinitionId| {
                let view = definitions.resolve(*id)?;
                if view.module() != &definition.identity
                    && view.segments().count() == 1
                    && view.last().is_some_and(|part| {
                        matches!(
                            part.kind,
                            DefinitionKind::Trait
                                | DefinitionKind::AssociatedType
                                | DefinitionKind::Enum
                                | DefinitionKind::Struct
                        )
                    })
                {
                    imports.insert(view.module().clone());
                }
                Ok(())
            },
            cancel,
        )
        .map_err(|error| DeclarationError(error.to_string()))?;
    Ok(imports)
}

#[cfg(test)]
mod tests;
