//! Direct, unchecked HIR construction from owned registration records.
mod items;
mod types;

use crate::{
    hir::item::Item,
    lower::{LoweredModule, context::Lowerer},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        metadata::DefinitionMetadata,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_source::source::SourceFile;
use kagari_types::declaration::module::{DeclarationError, ModuleDecl};
use std::{collections::HashMap, sync::Arc};

/// Construction-time correspondence. Entries are produced with allocation, not
/// reconstructed from source ranges or name matching after lowering.
#[derive(Debug, Default)]
pub(crate) struct RegistrationMap {
    pub(crate) items: HashMap<DefinitionId, Item>,
}

impl RegistrationMap {
    pub(crate) fn item(&self, declaration: DefinitionId) -> Result<Item, DeclarationError> {
        self.items
            .get(&declaration)
            .copied()
            .ok_or_else(|| DeclarationError("missing registration HIR identity".into()))
    }
}

struct StructuredLowerer<'a> {
    lowerer: Lowerer,
    declaration: &'a ModuleDecl,
    providers: &'a [Arc<DefinitionMetadata<ModuleDecl>>],
    definitions: &'a DefinitionTable,
    correspondence: RegistrationMap,
    parameter_names: HashMap<(DefinitionId, usize), String>,
}

pub(crate) fn lower(
    metadata: &DefinitionMetadata<ModuleDecl>,
    providers: &[Arc<DefinitionMetadata<ModuleDecl>>],
    source: Arc<SourceFile>,
    cancel: &CancellationToken,
) -> Result<(LoweredModule, RegistrationMap), DeclarationError> {
    let mut builder = StructuredLowerer {
        lowerer: Lowerer::new(cancel.clone()),
        declaration: metadata.records(),
        providers,
        definitions: metadata.definitions(),
        correspondence: RegistrationMap::default(),
        parameter_names: HashMap::new(),
    };
    builder.lowerer.source_map.mark_structured_input();
    builder.prepare_parameters()?;
    builder.module()?;
    cancel
        .check()
        .map_err(|_| DeclarationError("native declaration lowering cancelled".into()))?;
    let (module, source_map) = builder.lowerer.finish();
    Ok((
        LoweredModule::from_parts(source, module, source_map),
        builder.correspondence,
    ))
}
