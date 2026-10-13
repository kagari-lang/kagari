//! Normalize checked default applications in the retained MIR identity scope.
use crate::bytecode::BytecodeLoweringError;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, table::DefinitionTable},
};
use kagari_contract::types::{
    PublicItem,
    proofs::{ProofCatalog, implementation::Implementation},
};
use kagari_mir::verify::VerifiedMirModule;

pub(super) fn catalog<'a>(
    modules: &'a [VerifiedMirModule],
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<ProofCatalog<'a>, BytecodeLoweringError> {
    let mut contracts = Vec::new();
    for module in modules {
        for item in &module.abi.public_items {
            cancel
                .check()
                .map_err(|_| BytecodeLoweringError::InvalidNativeInterface)?;
            let PublicItem::Trait(contract) = item else {
                continue;
            };
            let owner = definitions
                .lookup_root(&module.identity)
                .and_then(|root| {
                    definitions.lookup_child(root, DefinitionKind::Trait, &contract.name, 0)
                })
                .ok_or(BytecodeLoweringError::InvalidNativeInterface)?;
            contracts.push((owner, contract));
        }
        contracts.extend(
            module
                .abi
                .trait_contracts
                .iter()
                .map(|contract| (contract.declaration, &contract.abi)),
        );
    }
    ProofCatalog::new(
        modules
            .iter()
            .flat_map(|module| &module.abi.public_items)
            .filter_map(|item| {
                let PublicItem::InterfaceTable(table) = item else {
                    return None;
                };
                (table.bridge.is_none()).then_some(Implementation::Interface(table.as_ref()))
            })
            .collect(),
        modules
            .iter()
            .flat_map(|module| &module.host_types)
            .collect(),
        modules.iter().flat_map(|module| &module.enumerations),
        contracts,
        modules
            .iter()
            .flat_map(|module| &module.abi.native_declarations),
        definitions,
        cancel,
    )
    .map_err(|_| BytecodeLoweringError::InvalidNativeInterface)
}
