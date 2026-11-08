//! Implementation search policy over acquisition-owned provider/consumer facts.
use crate::{
    analysis::{AnalysisFailure, indexes::scope::IndexScope},
    workspace::{ContextCapture, WorkspaceCapture, relations::ProviderKey},
};
use kagari_common::{cancellation::CancellationToken, identity::table::DefinitionId};
use kagari_hir::{
    analysis::signatures::{SignatureSnapshot, relations::ImplementationTarget},
    declarations::DeclarationId,
    types::TypeId,
};
use std::collections::BTreeSet;

pub(super) fn select(
    capture: &WorkspaceCapture,
    selected: &ContextCapture,
    signatures: &SignatureSnapshot,
    target: &ImplementationTarget,
    cancel: &CancellationToken,
) -> Result<IndexScope, AnalysisFailure> {
    let mut providers = BTreeSet::new();
    for anchor in [
        target.declaration,
        target.receiver.as_ref().and_then(receiver_anchor),
    ]
    .into_iter()
    .flatten()
    {
        cancel.check()?;
        if let Some(provider) = provider(selected, signatures, anchor)? {
            providers.insert(provider);
        }
    }
    let standalone = selected.relations.providers.is_empty();
    let project_wide = providers.is_empty();
    // A native/structural anchor does not join every project that uses the same
    // builtins. Search the selected project and actual consumers of its root.
    if providers.is_empty()
        && let Some(provider) = selected.relations.providers.get(&selected.root.package)
    {
        providers.insert(provider.clone());
    }
    for error in &capture.errors {
        cancel.check()?;
        let relevant = !standalone
            && error.providers.as_ref().is_none_or(|candidates| {
                providers
                    .iter()
                    .any(|provider| candidates.contains(provider))
                    || error.project.as_ref() == Some(&selected.key.workspace)
            });
        if relevant {
            return Err(AnalysisFailure::Unavailable(format!(
                "implementation search cannot establish discovery coverage: {}",
                error.issue
            )));
        }
    }
    let mut contexts = Vec::new();
    for context in &capture.contexts {
        cancel.check()?;
        let relevant = context.key == selected.key
            || (!standalone
                && ((project_wide && context.key.workspace == selected.key.workspace)
                    || (!providers.is_empty()
                        && providers.iter().all(|provider| {
                            context.relations.providers.get(&provider.package) == Some(provider)
                        }))));
        if relevant {
            contexts.push(context.clone());
        }
    }
    contexts.sort_by(|left, right| left.key.cmp(&right.key));
    Ok(IndexScope { contexts })
}

fn provider(
    context: &ContextCapture,
    signatures: &SignatureSnapshot,
    anchor: DefinitionId,
) -> Result<Option<ProviderKey>, AnalysisFailure> {
    let site = signatures
        .declaration_snapshot()
        .declaration(&DeclarationId::Definition(anchor))
        .ok_or_else(|| {
            AnalysisFailure::Unavailable(
                "implementation anchor has no declaration provenance".into(),
            )
        })?;
    let Some(origin) = context.origins.get(&site.location.file) else {
        if signatures.source(site.location.file).is_some_and(|source| {
            context
                .relations
                .providers
                .contains_key(&source.module_identity().package)
        }) {
            return Err(AnalysisFailure::Unavailable(
                "implementation source anchor has no captured physical origin".into(),
            ));
        }
        // Installed declarations are owned by the retained native environment.
        return Ok(None);
    };
    Ok(context
        .relations
        .providers
        .get(&origin.module.package)
        .cloned())
}

fn receiver_anchor(receiver: &TypeId) -> Option<DefinitionId> {
    match receiver {
        TypeId::Struct(nominal)
        | TypeId::Enum(nominal)
        | TypeId::NativeObject(nominal)
        | TypeId::Trait(nominal) => Some(nominal.declaration),
        TypeId::Reference { target, .. } => receiver_anchor(target),
        _ => None,
    }
}
