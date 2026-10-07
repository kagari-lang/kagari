//! Explicit adoption and authoring projections of cached analysis records.
use crate::{
    AnalysisPolicy, AnalysisResult, AnalyzedModule, aggregates::AggregateCatalog,
    analysis::error::AnalysisError, analyze_parsed, declare_analysis, host::HostDeclarations,
    lower::lower_module_controlled,
};
use kagari_syntax::parser::parse_with_limits;
use std::sync::Arc;
use {
    kagari_common::{
        cancellation::CancellationToken,
        identity::{
            DefinitionPath,
            map::DefinitionContext,
            mapping::{DefinitionMapper, DefinitionMappingError, DefinitionRecord},
            metadata::DefinitionMetadata,
            reference::DefinitionReference,
            table::{DefinitionId, DefinitionTable},
        },
    },
    kagari_source::source::SourceFile,
};

/// Interns portable paths in the database context and validates records with its resulting table.
pub(super) fn scope<T: DefinitionRecord<DefinitionPath>>(
    record: &T,
    context: &DefinitionContext,
    cancel: &CancellationToken,
) -> Result<DefinitionMetadata<T::Rebind<DefinitionId>>, DefinitionMappingError>
where
    T::Rebind<DefinitionId>: DefinitionRecord<DefinitionId>,
{
    let records = record.map_identities(&mut DefinitionMapper::new(
        &mut |path| context.intern(path).map_err(Into::into),
        cancel,
    ))?;
    DefinitionMetadata::checked(context.snapshot(), records, cancel)
}

/// Copies records while resolving every scoped ID through the supplied definition table.
pub(crate) fn paths<T: DefinitionRecord<DefinitionId>>(
    record: &T,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<T::Rebind<DefinitionPath>, DefinitionMappingError> {
    record.map_identities(&mut DefinitionMapper::new(
        &mut |id| Ok(definitions.resolve(*id)?.to_path()),
        cancel,
    ))
}

/// Maps portable or scoped references into an existing table; invalid/absent identities return None.
pub(super) fn locate<I: DefinitionReference, T: DefinitionRecord<I>>(
    record: &T,
    definitions: &DefinitionTable,
) -> Option<T::Rebind<DefinitionId>> {
    record
        .map_identities(&mut DefinitionMapper::new(
            &mut |id| Ok(id.resolve(definitions)?),
            &CancellationToken::default(),
        ))
        .ok()
}

/// Produces recoverable analysis with diagnostics for an over-limit source module identity.
pub(crate) fn recover_invalid_identity(
    source: &SourceFile,
) -> Result<AnalysisResult<AnalyzedModule>, AnalysisError> {
    let cancel = CancellationToken::default();
    let parsed =
        parse_with_limits(source, Default::default(), &cancel).expect("uncancelled recovery parse");
    let lowered = Arc::new(lower_module_controlled(
        Arc::new(source.clone()),
        &parsed.syntax(),
        &cancel,
    ));
    let definitions = DefinitionContext::new().expect("analysis definition context exhausted");
    let declared = declare_analysis(
        lowered,
        HostDeclarations::empty(),
        Default::default(),
        Default::default(),
        &definitions,
        &cancel,
    )?;
    let prepared = declared.check_signatures(Default::default(), None, &cancel);
    let mut aggregates = AggregateCatalog::default();
    aggregates
        .add_module(
            &prepared.lowered,
            &prepared.declarations,
            prepared.signatures.facts(),
            &cancel,
        )
        .expect("uncancelled recovery signatures");
    Ok(analyze_parsed(
        prepared,
        &parsed,
        AnalysisPolicy {
            const_limits: Default::default(),
            max_semantic_diagnostics: super::DEFAULT_MAX_SEMANTIC_DIAGNOSTICS,
        },
        Default::default(),
        aggregates,
        None,
        &cancel,
    ))
}
