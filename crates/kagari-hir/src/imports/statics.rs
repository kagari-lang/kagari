//! Runtime static contracts projected from checked declaration signatures.

use crate::{
    PreparedAnalysis,
    declarations::Declaration,
    hir::writeability::Writeability,
    identity_mapping::map_hash_entries,
    imports::{SourceDeclRef, SourceItem},
    resolver::resolved::{DeclarationNames, ResolvedName},
    types::TypeId,
};
use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};
use kagari_source::identity::FileId;
use std::collections::HashMap;

/// A canonical foreign runtime cell contract; importing never evaluates its initializer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedStatic {
    /// Semantic declaration identity shared by all aliases.
    pub declaration: DefinitionId,
    /// Original declaration/navigation site.
    pub site: Declaration,
    /// Required declared semantic type, not an inferred initializer value.
    pub ty: TypeId,
    /// Permission to rebind the cell through ordinary assignment.
    pub writeability: Writeability,
}

/// Foreign runtime cell facts keyed by complete source-unit declaration references.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportedStatics {
    statics: HashMap<SourceDeclRef, ImportedStatic>,
}

impl ImportedStatics {
    /// Finds a canonical imported static, independently of local alias spelling.
    pub fn get(&self, source: &SourceDeclRef) -> Option<&ImportedStatic> {
        self.statics.get(source)
    }

    pub(crate) fn bindings(
        modules: &HashMap<FileId, &PreparedAnalysis>,
        names: &DeclarationNames,
        cancel: &CancellationToken,
    ) -> Result<Self, Cancelled> {
        let mut result = Self::default();
        for source in names.catalog.reachable_sources(&names.items, cancel)? {
            cancel.check()?;
            let SourceItem::Static(id) = source.item else {
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
            let Some(ty) = module.signatures.facts.statics.get(&id) else {
                continue;
            };
            let target = ResolvedName::Static(id);
            let Some(declaration) = module.declarations.definition(target.clone()) else {
                continue;
            };
            result.statics.insert(
                source,
                ImportedStatic {
                    declaration: *declaration,
                    site: module
                        .declarations
                        .target(target)
                        .expect("static declaration")
                        .clone(),
                    ty: ty.clone(),
                    writeability: module.lowered.module.static_item(id).writeability,
                },
            );
        }
        Ok(result)
    }
}

impl RemapDefinitions for ImportedStatics {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(Self {
            statics: map_hash_entries(
                self.statics.len(),
                self.statics.iter().map(|(key, value)| {
                    Ok((
                        key.remap_definitions(mapper)?,
                        ImportedStatic {
                            declaration: mapper.reference(&value.declaration)?,
                            site: value.site.remap_definitions(mapper)?,
                            ty: value.ty.remap_definitions(mapper)?,
                            writeability: value.writeability,
                        },
                    ))
                }),
            )?,
        })
    }
}

impl DefinitionReferences for ImportedStatics {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        for (key, value) in &self.statics {
            check_cancel(cancel)?;
            key.visit_definition_ids(visit, cancel)?;
            visit(&value.declaration)?;
            value.site.visit_definition_ids(visit, cancel)?;
            value.ty.visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}
