//! Scoped static evidence absent from Native declaration syntax. No executable entries.

use crate::{
    DiagnosticBuffer,
    analysis::{error::AnalysisError, external::ExternalModule},
    lower::LoweredModule,
    native::NativeTypeKind,
    typeck::ModuleSignatures,
    types::semantic::lower_type,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        map::DefinitionContext,
        mapping::{DefinitionMapper, DefinitionMappingError},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};
use kagari_types::declaration::{
    TypeDeclaration, TypeDefKind, module::DeclarationError, path::HostPathDeclaration, verify,
};
use std::{collections::BTreeMap, sync::Arc};

/// A Host root and its complete shared schema/path closure, without Rust storage.
/// IDs belong to the enclosing `DefinitionMetadata` and are explicitly imported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalHost {
    pub root: TypeDeclaration,
    pub schemas: Vec<TypeDeclaration>,
    pub paths: Vec<HostPathDeclaration>,
}

impl DefinitionReferences for ExternalHost {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        self.root.visit_definition_ids(visit, cancel)?;
        self.schemas.visit_definition_ids(visit, cancel)?;
        self.paths.visit_definition_ids(visit, cancel)
    }
}

impl RemapDefinitions for ExternalHost {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        Ok(Self {
            root: self.root.remap_definitions(mapper)?,
            schemas: self.schemas.remap_definitions(mapper)?,
            paths: self.paths.remap_definitions(mapper)?,
        })
    }
}

impl ExternalModule {
    pub(super) fn host_kind(
        &self,
        name: &str,
        lowered: &LoweredModule,
        definitions: &DefinitionContext,
        cancel: &CancellationToken,
    ) -> Result<NativeTypeKind, AnalysisError> {
        let metadata = self.hosts.get(name).ok_or_else(|| {
            DeclarationError(format!(
                "external Host {name} requires schema/path metadata"
            ))
        })?;
        let imported = metadata.import_into(definitions, cancel)?;
        let host = imported.records();
        let table = imported.definitions();
        let root = &host.root;
        if root.definition.name != name
            || !matches!(root.definition.kind, TypeDefKind::Host(_))
            || !table
                .resolve(root.declaration)
                .is_ok_and(|view| view.module() == lowered.source.module_identity())
            || !verify::type_definition_valid(&root.definition, root.declaration, cancel, table)
        {
            return Err(DeclarationError(format!("invalid external Host root {name}")).into());
        }
        let mut schemas = BTreeMap::new();
        for schema in &host.schemas {
            cancel.check()?;
            if schemas.insert(schema.declaration, schema).is_some()
                || !verify::type_definition_valid(
                    &schema.definition,
                    schema.declaration,
                    cancel,
                    table,
                )
            {
                return Err(
                    DeclarationError("invalid or duplicate external Host schema".into()).into(),
                );
            }
        }
        if schemas.get(&root.declaration).copied() != Some(root) {
            return Err(DeclarationError(
                "external Host closure omits its exact root schema".into(),
            )
            .into());
        }
        for path in &host.paths {
            cancel.check()?;
            if path.root != root.declaration || path.contract(&host.schemas, table).is_err() {
                return Err(DeclarationError("invalid external Host path contract".into()).into());
            }
        }
        Ok(NativeTypeKind::Host {
            declaration: root.declaration,
            definition: Arc::new(root.definition.clone()),
            paths: host.paths.clone().into(),
            schemas: host.schemas.clone().into(),
        })
    }

    pub(super) fn attach_traits(
        &self,
        lowered: &mut LoweredModule,
        definitions: &DefinitionContext,
        cancel: &CancellationToken,
    ) -> Result<(), AnalysisError> {
        let root = definitions
            .intern_root(lowered.source.module_identity())
            .map_err(DefinitionMappingError::from)?;
        for (name, metadata) in &self.traits {
            cancel.check()?;
            let mut matching = lowered
                .module
                .traits
                .iter()
                .filter(|item| &item.name == name);
            let item = matching
                .next()
                .ok_or_else(|| DeclarationError(format!("missing external trait {name}")))?;
            if matching.next().is_some() {
                return Err(DeclarationError(format!("ambiguous external trait {name}")).into());
            }
            let scope = metadata.definitions();
            let owner = scope
                .lookup_root(lowered.source.module_identity())
                .and_then(|root| scope.lookup_child(root, DefinitionKind::Trait, name, 0));
            if !owner
                .is_some_and(|owner| verify::trait_valid(metadata.records(), owner, cancel, scope))
            {
                return Err(
                    DeclarationError(format!("invalid external trait evidence {name}")).into(),
                );
            }
            let imported = metadata.import_into(definitions, cancel)?;
            let contract = imported.records();
            let id = definitions
                .intern_child(root, DefinitionKind::Trait, name, 0)
                .map_err(DefinitionMappingError::from)?;
            if let Some(adapter) = &contract.conversion_adapter {
                lowered
                    .native_trait_adapters
                    .insert(item.id, adapter.clone());
            }
            lowered.external_traits.insert(id, imported.into_records());
        }
        lowered.language_foundation = self.language_foundation;
        Ok(())
    }
}

/// The source surface must describe the same Host fields as the static schema.
/// Paths may further restrict operations, but cannot replace authored field types.
pub(crate) fn validate_host_surfaces(
    lowered: &LoweredModule,
    signatures: &ModuleSignatures,
    diagnostics: &mut DiagnosticBuffer,
    cancel: &CancellationToken,
) {
    if !lowered.external_api {
        return;
    }
    for item in &lowered.module.native_types {
        if cancel.check().is_err() {
            return;
        }
        let Some(NativeTypeKind::Host { definition, .. }) = lowered.native_types.get(&item.id)
        else {
            continue;
        };
        let valid = item.fields.len() == definition.fields.len()
            && item
                .fields
                .iter()
                .zip(&definition.fields)
                .all(|(actual, expected)| {
                    actual.name == expected.name
                        && actual.visibility == expected.visibility
                        && actual.writeability.is_var() == expected.mutable
                        && signatures
                            .type_table()
                            .field_type(actual.id)
                            .is_some_and(|ty| !ty.is_unresolved() && lower_type(&ty) == expected.ty)
                });
        if !valid {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidHostPath {
                    reason: format!(
                        "external Host {} fields differ from its static schema",
                        item.name
                    ),
                })
                .with_span(lowered.source_map.native_type_span(item.id)),
            );
        }
    }
}

/// Shared schemas carried by different Host roots must agree in the mixed scope.
pub(in crate::analysis) fn validate_schema_closure<'a>(
    modules: impl IntoIterator<Item = &'a LoweredModule>,
    cancel: &CancellationToken,
) -> Result<(), AnalysisError> {
    let mut schemas = BTreeMap::new();
    for module in modules {
        for kind in module.native_types.values() {
            if let NativeTypeKind::Host {
                schemas: closure, ..
            } = kind
            {
                for schema in closure.iter() {
                    cancel.check()?;
                    if schemas
                        .insert(schema.declaration, &schema.definition)
                        .is_some_and(|old| old != &schema.definition)
                    {
                        return Err(
                            DeclarationError("conflicting external Host schemas".into()).into()
                        );
                    }
                }
            }
        }
    }
    Ok(())
}
