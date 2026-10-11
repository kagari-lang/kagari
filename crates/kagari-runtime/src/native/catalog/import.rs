//! Reuse immutable catalog imports within one validated installation batch.
use crate::{error::RuntimeError, native::catalog::DeclarationCatalog};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        map::{DefinitionContext, DefinitionMap},
        scoped::RemapDefinitions,
    },
};
use kagari_types::declaration::{NativeDeclaration, TraitDef, TypeDef, module::ImplDecl};
use std::{collections::HashMap, sync::Arc};

type SharedImports<T> = HashMap<usize, (Arc<DefinitionMap<T>>, Arc<DefinitionMap<T>>)>;

pub(crate) struct CatalogImports {
    context: DefinitionContext,
    types: SharedImports<TypeDef>,
    traits: SharedImports<TraitDef>,
    declarations: SharedImports<NativeDeclaration>,
    implementations: SharedImports<ImplDecl>,
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
        let mut imported = DeclarationCatalog {
            documentation: Default::default(),
            types: imported(&catalog.types, &self.context, &mut self.types)?,
            traits: imported(&catalog.traits, &self.context, &mut self.traits)?,
            declarations: imported(&catalog.declarations, &self.context, &mut self.declarations)?,
            implementations: imported(
                &catalog.implementations,
                &self.context,
                &mut self.implementations,
            )?,
        };
        imported.documentation = imported.import_documentation(catalog)?;
        imported.collect_implicit_definitions()?;
        Ok(imported)
    }
}

fn imported<T: RemapDefinitions>(
    source: &Arc<DefinitionMap<T>>,
    context: &DefinitionContext,
    imports: &mut SharedImports<T>,
) -> Result<Arc<DefinitionMap<T>>, RuntimeError> {
    let key = Arc::as_ptr(source) as usize;
    if let Some((_, imported)) = imports.get(&key) {
        return Ok(imported.clone());
    }
    let imported = Arc::new(
        source
            .import_records(context, &CancellationToken::default())
            .map_err(|cause| RuntimeError::metadata_conflict(cause.to_string()))?,
    );
    // Keep the source Arc alive so an address cannot be reused during the batch.
    // A caller changing entries must use Arc::make_mut and receives a different key.
    imports.insert(key, (source.clone(), imported.clone()));
    Ok(imported)
}
