//! Library-authored scoped declarations for the language foundation.
//! Registration and tooling consume the same records; no Rust bodies live here.
mod collections;
mod comparison;
mod construction;
mod construction_defaults;
mod contracts;
mod defaults;
mod documentation;
mod enums;
mod future;
mod identities;
mod iterable;
mod iterable_defaults;
mod key;
mod list_methods;
mod partition;
mod propagation;
mod roles;
mod strings;
mod tasks;
use crate::namespaces;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{ModuleIdentity, map::DefinitionContext, metadata::DefinitionMetadata},
};
use kagari_types::declaration::module::{DeclarationError, ModuleDecl};
use std::sync::{Arc, OnceLock};

// Assembly is transient authoring state. It is never installed, exported or
// validated as a module; partition assigns the final owners before publication.
fn assembly_identity() -> ModuleIdentity {
    namespaces::module("std", "__foundation")
}

/// Construct the complete foundation in an independent identity context.
/// Identity exhaustion, invalid contracts or missing publication identities return an error.
pub fn declarations() -> Result<Vec<DefinitionMetadata<ModuleDecl>>, DeclarationError> {
    let ids = DefinitionContext::new().map_err(identities::invalid)?;
    let mut module = ModuleDecl::new(assembly_identity());
    construction_step("contracts::declare", contracts::declare(&ids, &mut module))?;
    construction_step("future::declare", future::declare(&ids, &mut module))?;
    construction_step("tasks::declare", tasks::declare(&ids, &mut module))?;
    construction_step(
        "construction::declare",
        construction::declare(&ids, &mut module),
    )?;
    construction_step(
        "collections::declare",
        collections::declare(&ids, &mut module),
    )?;
    construction_step("iterable::declare", iterable::declare(&ids, &mut module))?;
    construction_step(
        "list_methods::declare",
        list_methods::declare(&ids, &mut module),
    )?;
    construction_step("defaults::declare", defaults::declare(&ids, &mut module))?;
    construction_step(
        "comparison::declare",
        comparison::declare(&ids, &mut module),
    )?;
    construction_step(
        "construction_defaults::declare",
        construction_defaults::declare(&ids, &mut module),
    )?;
    construction_step(
        "propagation::implementations",
        propagation::implementations(&ids, &mut module),
    )?;
    construction_step(
        "list_methods::configure_overrides",
        list_methods::configure_overrides(&ids, &mut module),
    )?;
    construction_step("strings::declare", strings::declare(&ids, &mut module))?;
    let source = module.collect_definitions(&ids)?;
    let (context, mut modules) =
        construction_step("partition", partition::finish(module, &source))?;
    let definitions = context.snapshot();
    for module in &mut modules {
        construction_step(
            "documentation",
            documentation::complete(&definitions, module),
        )?;
    }
    construction_step(
        "inherited documentation",
        documentation::inherit(&definitions, &mut modules),
    )?;
    modules
        .into_iter()
        .map(|module| {
            DefinitionMetadata::checked(definitions.clone(), module, &CancellationToken::default())
                .map_err(identities::invalid)
        })
        .collect()
}

/// Retain immutable scoped foundation inputs for ordinary registration and tooling.
/// Sharing records does not install runtime handlers or grant execution evidence.
pub fn shared() -> Result<Vec<Arc<DefinitionMetadata<ModuleDecl>>>, DeclarationError> {
    static CONTRACTS: OnceLock<Result<Vec<Arc<DefinitionMetadata<ModuleDecl>>>, DeclarationError>> =
        OnceLock::new();
    CONTRACTS
        .get_or_init(|| declarations().map(|modules| modules.into_iter().map(Arc::new).collect()))
        .clone()
}

fn construction_step<T>(
    name: &str,
    outcome: Result<T, DeclarationError>,
) -> Result<T, DeclarationError> {
    outcome.map_err(|error| DeclarationError(format!("foundation {name}: {error}")))
}
