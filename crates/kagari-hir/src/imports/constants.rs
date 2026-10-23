//! Canonical imported constant facts, never initializer syntax or alias-based identities.

use std::{collections::HashMap, sync::Arc};

use crate::{
    PreparedAnalysis,
    declarations::Declaration,
    identity_mapping::map_hash_entries,
    imports::{SourceDeclRef, SourceItem},
    resolver::resolved::{DeclarationNames, ResolvedName},
    typeck::{constant_facts::CheckedConstants, scalar::ScalarValue},
};
use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};
use kagari_source::{diagnostic::Severity, identity::FileId};

/// A checked scalar and its original declaration, shared by every import alias.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedConstant {
    /// Navigation and canonical declaration identity, not the importing alias.
    pub site: Declaration,
    /// Validated by the declaring module's ordinary constant checker/evaluator.
    pub value: ScalarValue,
}

/// Imported constants keyed by source file, revision, arena and declaration slot.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImportedConstants {
    constants: HashMap<SourceDeclRef, ImportedConstant>,
}

impl ImportedConstants {
    /// Finds the checked owner fact without interpreting source spelling.
    pub fn get(&self, source: &SourceDeclRef) -> Option<&ImportedConstant> {
        self.constants.get(source)
    }

    pub(crate) fn bindings(
        modules: &HashMap<FileId, &PreparedAnalysis>,
        checked: &HashMap<FileId, Arc<CheckedConstants>>,
        names: &DeclarationNames,
        cancel: &CancellationToken,
    ) -> Result<Self, Cancelled> {
        let mut result = Self::default();
        for source in names.catalog.reachable_sources(&names.items, cancel)? {
            cancel.check()?;
            let SourceItem::Const(id) = source.item else {
                continue;
            };
            let Some(module) = modules.get(&source.unit.file) else {
                continue;
            };
            if !source
                .unit
                .matches(&module.lowered, &module.names.facts.catalog.definitions)
            {
                continue;
            }
            let Some(facts) = checked.get(&source.unit.file) else {
                continue;
            };
            if facts
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Error)
            {
                continue;
            }
            let Some(value) = facts.values.get(&id) else {
                continue;
            };
            // Evaluation can retain recovery facts. Only matching, const-safe
            // declared/inferred types may cross the import boundary.
            if facts.types.get(&id) != Some(&value.ty()) {
                continue;
            }
            let Some(site) = module.declarations.target(ResolvedName::Const(id)) else {
                continue;
            };
            result.constants.insert(
                source,
                ImportedConstant {
                    site: site.clone(),
                    value: value.clone(),
                },
            );
        }
        Ok(result)
    }
}

impl RemapDefinitions for ImportedConstants {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(Self {
            constants: map_hash_entries(
                self.constants.len(),
                self.constants.iter().map(|(key, value)| {
                    Ok((
                        key.remap_definitions(mapper)?,
                        ImportedConstant {
                            site: value.site.remap_definitions(mapper)?,
                            value: value.value.clone(),
                        },
                    ))
                }),
            )?,
        })
    }
}

impl DefinitionReferences for ImportedConstants {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        for (key, value) in &self.constants {
            check_cancel(cancel)?;
            key.visit_definition_ids(visit, cancel)?;
            value.site.visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}
