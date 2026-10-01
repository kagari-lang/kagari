//! Normalize already checked default applications over the portable MIR closure.
use crate::bytecode::BytecodeLoweringError;
use kagari_abi::types::{PublicAbiItem, proofs::ProofCatalog};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionId, DefinitionKind, DefinitionPathSegment},
};
use kagari_mir::verify::VerifiedMirModule;

pub(super) fn catalog<'a>(
    closure: &[&'a VerifiedMirModule],
    cancel: &CancellationToken,
) -> Result<ProofCatalog<'a>, BytecodeLoweringError> {
    let contracts = closure.iter().flat_map(|module| {
        module
            .abi
            .public_items
            .iter()
            .filter_map(|item| {
                let PublicAbiItem::Trait(contract) = item else {
                    return None;
                };
                Some((
                    DefinitionId {
                        module: module.identity.clone(),
                        path: vec![DefinitionPathSegment {
                            kind: DefinitionKind::Trait,
                            name: contract.name.clone(),
                            occurrence: 0,
                        }],
                    },
                    contract,
                ))
            })
            .chain(
                module
                    .abi
                    .trait_contracts
                    .iter()
                    .map(|contract| (contract.declaration.clone(), &contract.abi)),
            )
    });
    ProofCatalog::new(
        closure
            .iter()
            .flat_map(|module| &module.abi.public_items)
            .filter_map(|item| {
                let PublicAbiItem::InterfaceTable(table) = item else {
                    return None;
                };
                (!table.native_bridge && !table.host_bridge).then_some(table.as_ref())
            })
            .collect(),
        closure
            .iter()
            .flat_map(|module| &module.host_types)
            .collect(),
        closure.iter().flat_map(|module| &module.enumerations),
        contracts,
        closure
            .iter()
            .flat_map(|module| &module.abi.native_declarations),
        cancel,
    )
    .map_err(|_| BytecodeLoweringError::InvalidNativeInterface)
}
