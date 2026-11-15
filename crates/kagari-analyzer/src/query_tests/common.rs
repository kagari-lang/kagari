use kagari_common::identity::{
    map::DefinitionContext, metadata::DefinitionMetadata, scoped::RemapDefinitions,
    table::DefinitionTable,
};
/// Import checked semantic facts into a test-owned comparison scope. Source-local
/// analysis/arena identities remain intact and must be checked separately.
pub(crate) fn import_facts<T>(
    facts: &T,
    definitions: &DefinitionTable,
    context: &DefinitionContext,
) -> DefinitionMetadata<T>
where
    T: Clone + RemapDefinitions,
{
    DefinitionMetadata::checked(definitions.clone(), facts.clone(), &Default::default())
        .unwrap()
        .import_into(context, &Default::default())
        .unwrap()
}
