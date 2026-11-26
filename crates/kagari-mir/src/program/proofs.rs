//! Immutable graph resources with admission reused only for identical visibility.
use crate::verify::ValidatedMirModule;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, table::DefinitionTable},
};
use kagari_contract::types::{
    PublicItem,
    proofs::{
        ProofCatalog,
        implementation::Implementation,
        resources::{ProgramProofs, ProofModule},
    },
};
use kagari_types::{declaration::ownership::ReceiverOwners, ty::substitution::TypeTransformError};
use std::{collections::HashMap, sync::Arc};

pub(super) struct ProofResources<'a> {
    proofs: ProgramProofs<'a>,
    scopes: HashMap<Vec<usize>, Arc<ProofCatalog<'a>>>,
}

impl<'a> ProofResources<'a> {
    pub fn new(
        modules: &'a [ValidatedMirModule],
        definitions: &DefinitionTable,
        cancel: &CancellationToken,
    ) -> Result<Self, TypeTransformError> {
        let mut records = Vec::with_capacity(modules.len());
        for module in modules {
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            let mut contracts = Vec::new();
            let mut implementations = Vec::new();
            for item in &module.abi.public_items {
                match item {
                    PublicItem::Trait(record) => {
                        let owner = definitions
                            .lookup_root(&module.identity)
                            .and_then(|root| {
                                definitions.lookup_child(
                                    root,
                                    DefinitionKind::Trait,
                                    &record.name,
                                    0,
                                )
                            })
                            .ok_or(TypeTransformError::InvalidContract)?;
                        contracts.push((owner, record));
                    }
                    PublicItem::InterfaceTable(table) => {
                        implementations.push(Implementation::Interface(table.as_ref()));
                    }
                    _ => {}
                }
            }
            contracts.extend(
                module
                    .abi
                    .trait_contracts
                    .iter()
                    .map(|record| (record.declaration, &record.abi)),
            );
            records.push(ProofModule {
                implementations,
                contracts,
                enumerations: module.enumerations.iter().collect(),
                native_declarations: module.abi.native_declarations.iter().collect(),
            });
        }
        ReceiverOwners::from_types(modules.iter().flat_map(|module| {
            module
                .abi
                .public_items
                .iter()
                .filter_map(|item| match item {
                    PublicItem::Type(ty) => Some((&module.identity, ty)),
                    _ => None,
                })
        }))
        .map_err(|_| TypeTransformError::InvalidContract)?;
        Ok(Self {
            proofs: ProgramProofs::new(records, definitions, cancel)?,
            scopes: HashMap::new(),
        })
    }

    /// The caller supplies sorted module positions, once per module in this graph.
    /// Only closure-wide admission is shared; local assumptions and queries are not.
    pub fn scope(
        &mut self,
        indices: Vec<usize>,
        cancel: &CancellationToken,
    ) -> Result<Arc<ProofCatalog<'a>>, TypeTransformError> {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        if let Some(catalog) = self.scopes.get(&indices) {
            return Ok(catalog.clone());
        }

        let catalog = self.proofs.view(indices.iter().copied(), cancel)?;
        if !catalog.overrides_valid(cancel)? {
            return Err(TypeTransformError::InvalidContract);
        }
        let catalog = Arc::new(catalog);
        self.scopes.insert(indices, catalog.clone());
        Ok(catalog)
    }
}
