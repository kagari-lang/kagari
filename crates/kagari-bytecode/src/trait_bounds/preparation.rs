//! Program-owned declaration indexes and dependency-scoped proof views.
use crate::{
    module::BytecodeModule,
    trait_bounds::{LinkedValidationError, declarations},
    verifier::BytecodeVerificationError,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        ModuleIdentity,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_contract::types::{
    InterfaceTable, PublicItem,
    proofs::{
        ProofCatalog,
        implementation::Implementation,
        resources::{ProgramProofs, ProofModule},
    },
};
use kagari_types::declaration::{TraitDef, ownership::ReceiverOwners};
use std::collections::{HashMap, HashSet};

pub(crate) struct VerificationResources<'a> {
    pub modules: Vec<&'a BytecodeModule>,
    pub module_indices: HashMap<&'a ModuleIdentity, usize>,
    pub declarations: Vec<Vec<(DefinitionId, &'a TraitDef)>>,
    pub tables: Vec<Vec<&'a InterfaceTable>>,
    proofs: ProgramProofs<'a>,
    owners: ReceiverOwners,
}

pub(crate) struct VerificationScope<'a> {
    pub modules: Vec<&'a BytecodeModule>,
    pub catalog: ProofCatalog<'a>,
    pub owners: ReceiverOwners,
}

impl<'a> VerificationResources<'a> {
    pub fn new(
        modules: Vec<&'a BytecodeModule>,
        definitions: &DefinitionTable,
        cancel: &CancellationToken,
    ) -> Result<Self, BytecodeVerificationError> {
        let mut records = Vec::with_capacity(modules.len());
        let mut all_declarations = Vec::with_capacity(modules.len());
        let mut all_tables = Vec::with_capacity(modules.len());
        let mut module_indices = HashMap::new();
        for (index, module) in modules.iter().enumerate() {
            cancel
                .check()
                .map_err(|_| BytecodeVerificationError::Cancelled)?;
            module_indices.insert(&module.identity, index);
            let declarations = declarations(module, definitions)
                .map_err(LinkedValidationError::into_verification)?;
            let tables = module
                .public_items
                .iter()
                .filter_map(|item| match item {
                    PublicItem::InterfaceTable(table) if table.bridge.is_none() => {
                        Some(table.as_ref())
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            records.push(ProofModule {
                implementations: tables
                    .iter()
                    .copied()
                    .map(Implementation::Interface)
                    .collect(),
                enumerations: module.enumerations.iter().collect(),
                contracts: declarations.clone(),
                native_declarations: module.native_declarations.iter().collect(),
            });
            all_declarations.push(declarations);
            all_tables.push(tables);
        }
        let owners = ReceiverOwners::from_types(modules.iter().flat_map(|module| {
            module.public_items.iter().filter_map(|item| match item {
                PublicItem::Type(ty) => Some((&module.identity, ty)),
                _ => None,
            })
        }))
        .map_err(|_| BytecodeVerificationError::InvalidInterfaceTable)?;
        let proofs = ProgramProofs::new(records, definitions, cancel)
            .map_err(|error| LinkedValidationError::from(error).into_verification())?;
        Ok(Self {
            modules,
            module_indices,
            declarations: all_declarations,
            tables: all_tables,
            proofs,
            owners,
        })
    }

    pub fn receiver_owners(&self, indices: &[usize]) -> ReceiverOwners {
        let visible = indices
            .iter()
            .map(|index| &self.modules[*index].identity)
            .collect::<HashSet<_>>();
        self.owners.restricted_to(|module| visible.contains(module))
    }

    pub fn scope(
        &self,
        indices: &[usize],
        owners: ReceiverOwners,
        cancel: &CancellationToken,
    ) -> Result<VerificationScope<'a>, BytecodeVerificationError> {
        let catalog = self
            .proofs
            .view(indices.iter().copied(), cancel)
            .map_err(|error| LinkedValidationError::from(error).into_verification())?;
        let modules = indices
            .iter()
            .map(|index| self.modules[*index])
            .collect::<Vec<_>>();
        Ok(VerificationScope {
            modules,
            catalog,
            owners,
        })
    }
}
