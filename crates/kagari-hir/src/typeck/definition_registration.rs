//! Register implementation-owned method identities after resolving trait headers.

use crate::{
    declarations::Declarations, lower::LoweredModule, resolver::resolved::ResolvedName,
    typeck::ModuleSignatures,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{mapping::DefinitionMappingError, table::DefinitionTable},
};

/// Reserve the same member identity for explicit and omitted implementation methods.
///
/// Registration does not establish a default body or satisfy a required method.
/// Aggregate selection still checks the actual trait contract. Reserving every
/// declared member also works for imported declaration surfaces, before their
/// checked default bodies are available. Failure prevents signature publication.
pub(crate) fn implementation_members(
    lowered: &LoweredModule,
    declarations: &Declarations,
    signatures: &ModuleSignatures,
    cancel: &CancellationToken,
) -> Result<DefinitionTable, DefinitionMappingError> {
    let context = declarations.context();
    let definitions = context.snapshot();
    for (owner, interface, ..) in signatures.type_table().implementation_entries() {
        cancel
            .check()
            .map_err(|_| DefinitionMappingError::Cancelled)?;
        let local = declarations
            .definition_target(&interface.declaration)
            .and_then(|target| match target {
                ResolvedName::Trait(id) => lowered.module.traits.iter().find(|item| item.id == id),
                _ => None,
            });
        let local_methods = local
            .into_iter()
            .flat_map(|item| &item.methods)
            .filter_map(|method| {
                declarations
                    .definition(ResolvedName::Function(method.function))
                    .copied()
            });
        let imported_methods = declarations
            .imported_types()
            .by_declaration(&interface.declaration)
            .into_iter()
            .filter(|_| local.is_none())
            .flat_map(|item| &item.trait_methods)
            .map(|method| method.declaration);
        for method in local_methods.chain(imported_methods) {
            cancel
                .check()
                .map_err(|_| DefinitionMappingError::Cancelled)?;
            let member = definitions
                .resolve(method)?
                .last()
                .expect("trait method identity");
            context.intern_child(*owner, member.kind, member.name, member.occurrence)?;
        }
    }
    cancel
        .check()
        .map_err(|_| DefinitionMappingError::Cancelled)?;
    Ok(context.snapshot())
}
