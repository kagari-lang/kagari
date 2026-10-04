//! Reuse immutable catalog imports within one validated installation batch.
use crate::{
    error::RuntimeError,
    native::catalog::{DeclarationCatalog, ownership::scope_values},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionPath,
        map::{DefinitionContext, DefinitionMap},
        mapping::DefinitionRecord,
        table::DefinitionId,
    },
};
use kagari_types::declaration::{NativeDeclaration, TraitDef, TypeDef, module::ImplDecl};
use std::{collections::HashMap, sync::Arc};

type SharedImports<T, U = T> = HashMap<usize, (Arc<DefinitionMap<T>>, Arc<DefinitionMap<U>>)>;

pub(crate) struct CatalogImports {
    context: DefinitionContext,
    types: SharedImports<TypeDef<DefinitionId>>,
    traits: SharedImports<TraitDef<DefinitionId>>,
    declarations: SharedImports<NativeDeclaration<DefinitionId>>,
    implementations: SharedImports<ImplDecl<DefinitionId>>,
}

impl CatalogImports {
    pub(crate) fn new(context: DefinitionContext) -> Self {
        Self {
            context,
            types: HashMap::new(),
            traits: HashMap::new(),
            declarations: HashMap::new(),
            implementations: HashMap::new(),
        }
    }

    pub(crate) fn import(
        &mut self,
        catalog: &DeclarationCatalog,
    ) -> Result<DeclarationCatalog, RuntimeError> {
        Ok(DeclarationCatalog {
            types: imported(&catalog.types, &self.context, &mut self.types)?,
            traits: imported(&catalog.traits, &self.context, &mut self.traits)?,
            declarations: imported(&catalog.declarations, &self.context, &mut self.declarations)?,
            implementations: imported(
                &catalog.implementations,
                &self.context,
                &mut self.implementations,
            )?,
        })
    }
}

fn imported<T: DefinitionRecord<DefinitionId, Rebind<DefinitionId> = T> + Clone>(
    source: &Arc<DefinitionMap<T>>,
    context: &DefinitionContext,
    imports: &mut SharedImports<T>,
) -> Result<Arc<DefinitionMap<T>>, RuntimeError> {
    shared(source, imports, |source| {
        source
            .import_records(context, &CancellationToken::default())
            .map_err(|cause| RuntimeError::metadata_conflict(cause.to_string()))
    })
}

fn shared<T, U>(
    source: &Arc<DefinitionMap<T>>,
    imports: &mut SharedImports<T, U>,
    convert: impl FnOnce(&DefinitionMap<T>) -> Result<DefinitionMap<U>, RuntimeError>,
) -> Result<Arc<DefinitionMap<U>>, RuntimeError> {
    let key = Arc::as_ptr(source) as usize;
    if let Some((_, imported)) = imports.get(&key) {
        return Ok(imported.clone());
    }
    let imported = Arc::new(convert(source)?);
    // Keep the source Arc alive so an address cannot be reused during the batch.
    // A caller changing entries must use Arc::make_mut and receives a different key.
    imports.insert(key, (source.clone(), imported.clone()));
    Ok(imported)
}

/// Preserve a closed seed's sharing when authoring binding requirements become scoped.
pub(crate) struct CatalogScopes {
    context: DefinitionContext,
    types: SharedImports<TypeDef, TypeDef<DefinitionId>>,
    traits: SharedImports<TraitDef, TraitDef<DefinitionId>>,
    declarations: SharedImports<NativeDeclaration, NativeDeclaration<DefinitionId>>,
    implementations: SharedImports<ImplDecl, ImplDecl<DefinitionId>>,
}

impl CatalogScopes {
    pub(crate) fn new(context: DefinitionContext) -> Self {
        Self {
            context,
            types: HashMap::new(),
            traits: HashMap::new(),
            declarations: HashMap::new(),
            implementations: HashMap::new(),
        }
    }

    pub(crate) fn scope(
        &mut self,
        catalog: &DeclarationCatalog<DefinitionPath>,
    ) -> Result<DeclarationCatalog, RuntimeError> {
        Ok(DeclarationCatalog {
            types: shared(&catalog.types, &mut self.types, |source| {
                scope_values(source, &self.context)
            })?,
            traits: shared(&catalog.traits, &mut self.traits, |source| {
                scope_values(source, &self.context)
            })?,
            declarations: shared(&catalog.declarations, &mut self.declarations, |source| {
                scope_values(source, &self.context)
            })?,
            implementations: shared(
                &catalog.implementations,
                &mut self.implementations,
                |source| scope_values(source, &self.context),
            )?,
        })
    }
}
