use crate::{
    AnalysisResult, DiagnosticBuffer,
    aggregates::AggregateCatalog,
    declarations::Declarations,
    hir::ids::BodySelection,
    lower::{LoweredModule, lower_module},
    resolver::resolved::{DeclarationNames, ResolvedNames},
    typeck::{
        BodyInputs, TypedModule,
        applications::validate_signatures,
        check::{check_bodies_controlled, check_signatures},
        definition_registration,
        reuse::BodyReuse,
    },
};
use kagari_common::identity::{
    DefinitionKind,
    map::DefinitionContext,
    metadata::DefinitionMetadata,
    scoped::RemapDefinitions,
    table::{DefinitionId, DefinitionTable},
};
use kagari_source::source::SourceFile;

use kagari_syntax::parser::parse_module;

pub fn lower_ok(text: &str) -> LoweredModule {
    let source = SourceFile::new("test.kg", text);
    parse_module(&source).expect("source should parse");
    lower_module(&source)
}

pub fn definition(
    definitions: &DefinitionTable,
    lowered: &LoweredModule,
    kind: DefinitionKind,
    name: &str,
) -> DefinitionId {
    let root = definitions
        .lookup_root(lowered.source.module_identity())
        .unwrap();
    definitions.lookup_child(root, kind, name, 0).unwrap()
}

/// Check a source fixture in its caller-owned identity scope. The caller retains
/// the context for subsequent nominal queries and independent fixture comparison.
pub fn check_module(
    context: &DefinitionContext,
    lowered: &LoweredModule,
    names: &ResolvedNames,
    reuse: Option<&BodyReuse<'_>>,
) -> AnalysisResult<TypedModule> {
    let names = names.clone();
    let mut declarations = Declarations::collect_named(
        &lowered.source,
        lowered,
        &DeclarationNames {
            catalog: names.catalog.clone(),
            items: names.items.clone(),
            imports: names.imports.clone(),
        },
        context,
        &Default::default(),
    )
    .unwrap()
    .with_bindings(lowered, &names, &Default::default());
    let mut signatures = check_signatures(lowered, &declarations, &Default::default());
    let definitions = definition_registration::implementation_members(
        lowered,
        &declarations,
        signatures.facts(),
        &Default::default(),
    )
    .unwrap();
    declarations.publish_definitions(definitions);
    let mut aggregates = AggregateCatalog::default();
    aggregates
        .add_module(
            lowered,
            &declarations,
            signatures.facts(),
            &Default::default(),
        )
        .unwrap();
    let mut diagnostics = DiagnosticBuffer::new();
    validate_signatures(
        lowered,
        &declarations,
        signatures.facts(),
        &aggregates,
        &mut diagnostics,
        &Default::default(),
    );
    signatures.diagnostics.extend(diagnostics);
    check_bodies_controlled(
        lowered,
        &names,
        &declarations,
        BodyInputs {
            const_limits: Default::default(),
            selection: BodySelection::All,
            signatures: &signatures,
            imported_functions: &Default::default(),
            imported_statics: &Default::default(),
            imported_constants: &Default::default(),
            constants: None,
            aggregates: &aggregates,
        },
        reuse,
        &Default::default(),
    )
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
