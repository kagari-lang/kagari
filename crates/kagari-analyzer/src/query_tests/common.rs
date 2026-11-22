use kagari_common::identity::{
    map::DefinitionContext, metadata::DefinitionMetadata, scoped::RemapDefinitions,
    table::DefinitionTable,
};
use kagari_hir::{analysis::AnalysisSnapshot, declarations::Declaration};

#[track_caller]
pub(super) fn assert_source_spelling(
    snapshot: &AnalysisSnapshot,
    declaration: &Declaration,
    uri: &str,
    spelling: &str,
) {
    let location = declaration.source_location().expect("authored declaration");
    let source = snapshot.source(location.file).expect("declaration source");
    assert_eq!(source.name(), uri);
    let range = source.local_range(location).expect("current source range");
    assert_eq!(&source.text()[range.start..range.end], spelling);
}

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
