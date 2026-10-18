//! Library-authored scoped declarations for the language foundation.
//! Registration and tooling consume the same records; no Rust bodies live here.
mod collections;
mod comparison;
mod construction;
mod construction_defaults;
mod contracts;
mod defaults;
pub(crate) mod documentation;
mod future;
mod identities;
mod interface_protocols;
mod iterable;
mod iterable_defaults;
mod key;
mod list_methods;
mod partition;
mod propagation;
mod roles;
mod tasks;
use crate::{assembly, namespaces};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{ModuleIdentity, map::DefinitionContext, metadata::DefinitionMetadata},
};
use kagari_types::declaration::module::{DeclarationError, ModuleDecl};
use std::sync::Arc;

// Assembly is transient authoring state. It is never installed, exported or
// validated as a module; partition assigns the final owners before publication.
fn assembly_identity() -> ModuleIdentity {
    namespaces::module("std", "__foundation")
}

/// Construct only the remaining SR03/SR04 contributors in a transient identity context.
/// Identity exhaustion, invalid contracts or missing publication identities return an error.
pub(crate) fn remaining() -> Result<Vec<DefinitionMetadata<ModuleDecl>>, DeclarationError> {
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
        "interface_protocols::declare",
        interface_protocols::declare(&ids, &mut module),
    )?;
    construction_step(
        "comparison::declare",
        comparison::declare(&ids, &mut module),
    )?;
    construction_step(
        "construction_defaults::declare",
        construction_defaults::declare(&ids, &mut module),
    )?;
    construction_step(
        "list_methods::configure_overrides",
        list_methods::configure_overrides(&ids, &mut module),
    )?;
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

/// Immutable production declarations from the same bundle that owns native preparation.
pub fn declarations() -> Result<Vec<DefinitionMetadata<ModuleDecl>>, DeclarationError> {
    assembly::semantic()
        .map(|semantic| semantic.snapshot().modules().to_vec())
        .map_err(|error| DeclarationError(error.to_string()))
}

pub fn shared() -> Result<Vec<Arc<DefinitionMetadata<ModuleDecl>>>, DeclarationError> {
    declarations().map(|modules| modules.into_iter().map(Arc::new).collect())
}

fn construction_step<T>(
    name: &str,
    outcome: Result<T, DeclarationError>,
) -> Result<T, DeclarationError> {
    outcome.map_err(|error| DeclarationError(format!("foundation {name}: {error}")))
}
