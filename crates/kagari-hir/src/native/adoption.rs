//! Import a provider batch once before attaching its records to source lowering.
use kagari_common::{
    cancellation::CancellationToken,
    identity::{map::DefinitionContext, metadata::DefinitionMetadata},
};
use kagari_types::declaration::module::{DeclarationError, ModuleDecl};
use std::{collections::HashSet, sync::Arc};

/// Return one retained prefix containing all provider references and implicit names.
/// Identity adoption does not grant declaration, binding or source correspondence evidence.
pub(crate) fn adopt(
    providers: &[Arc<DefinitionMetadata<ModuleDecl>>],
    context: &DefinitionContext,
    cancel: &CancellationToken,
) -> Result<Vec<Arc<DefinitionMetadata<ModuleDecl>>>, DeclarationError> {
    cancel
        .check()
        .map_err(|_| DeclarationError("native provider adoption cancelled".into()))?;
    let invalid = |error| DeclarationError(format!("native provider adoption: {error}"));
    let mut identities = HashSet::new();
    let mut records = Vec::with_capacity(providers.len());
    for provider in providers {
        if !identities.insert(&provider.records().identity) {
            return Err(DeclarationError("duplicate native provider module".into()));
        }
        let imported = provider.import_into(context, cancel).map_err(invalid)?;
        imported.records().collect_definitions(context)?;
        records.push(imported.into_records());
    }
    cancel
        .check()
        .map_err(|_| DeclarationError("native provider adoption cancelled".into()))?;
    let definitions = context.snapshot();
    records
        .into_iter()
        .map(|records| {
            DefinitionMetadata::checked(definitions.clone(), records, cancel)
                .map(Arc::new)
                .map_err(invalid)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::{paths::array_interfaces, render::declaration_source};
    use kagari_common::identity::{DefinitionKind, ModuleIdentity, scoped::DefinitionReferences};
    use kagari_stdlib::catalog;

    #[test]
    fn provider_batch_adoption_retains_one_analysis_scope_and_checks_failure() {
        let cancel = CancellationToken::default();
        // Alternate providers from independent writers to exercise cross-provider adoption.
        let left = catalog::shared().unwrap();
        let right = catalog::declarations().unwrap();
        let providers = left
            .iter()
            .zip(right)
            .enumerate()
            .map(|(index, (left, right))| {
                if index % 2 == 0 {
                    left.clone()
                } else {
                    Arc::new(right)
                }
            })
            .collect::<Vec<_>>();
        let context = DefinitionContext::new().unwrap();
        let adopted = adopt(&providers, &context, &cancel).unwrap();
        let retained = adopted[0].definitions().clone();
        for (original, adopted_module) in providers.iter().zip(&adopted) {
            assert_eq!(adopted_module.definitions().id(), retained.id());
            assert_eq!(adopted_module.definitions().len(), retained.len());
            assert_eq!(
                original.records().identity,
                adopted_module.records().identity
            );
            assert_eq!(
                declaration_source(original, &providers).unwrap().text,
                declaration_source(adopted_module, &adopted).unwrap().text,
            );
        }
        let interfaces =
            array_interfaces(adopted.iter().map(|module| module.records()), &retained).unwrap();
        assert_eq!(interfaces.len(), 2);
        let again = adopt(&providers, &context, &cancel).unwrap();
        assert_eq!(retained.len(), again[0].definitions().len());
        let references = |module: &DefinitionMetadata<ModuleDecl>| {
            let mut references = vec![];
            module
                .records()
                .visit_definition_ids(
                    &mut |id| {
                        references.push(*id);
                        Ok(())
                    },
                    &cancel,
                )
                .unwrap();
            references
        };
        for (first, second) in adopted.iter().zip(&again) {
            assert_eq!(references(first), references(second));
        }
        let root = context
            .intern_root(&ModuleIdentity::single_file("later"))
            .unwrap();
        let later = context
            .intern_child(root, DefinitionKind::Function, "new", 0)
            .unwrap();
        assert!(retained.resolve(later).is_err());
        assert!(
            adopt(
                &[providers[0].clone(), providers[0].clone()],
                &context,
                &cancel
            )
            .is_err()
        );
        cancel.cancel();
        assert!(adopt(&providers, &context, &cancel).is_err());
        assert!(adopt(&[], &context, &cancel).is_err());
        assert!(
            adopted
                .iter()
                .all(|module| module.definitions().len() == retained.len())
        );
    }
}
