//! Normalize already checked default applications over the portable MIR closure.
use crate::bytecode::BytecodeLoweringError;
use kagari_common::{
    cancellation::CancellationToken,
    host_interface::type_declaration::HostTypeDeclaration,
    identity::{DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity},
};
use kagari_contract::layout::EnumLayout;
use kagari_contract::types::{
    ModuleContract, PublicItem,
    proofs::{ProofCatalog, implementation::Implementation},
};
use kagari_mir::verify::VerifiedMirModule;

pub(super) struct Contracts {
    declarations: Vec<(ModuleIdentity, ModuleContract)>,
    hosts: Vec<HostTypeDeclaration>,
    enumerations: Vec<EnumLayout>,
}

impl Contracts {
    /// Declaration proof checking is an explicit authoring-contract boundary.
    /// Function bodies and backend facts stay in their compact immutable scope.
    pub(super) fn from_modules(
        modules: &[VerifiedMirModule],
        cancel: &CancellationToken,
    ) -> Result<Self, BytecodeLoweringError> {
        let mut contracts = Self {
            declarations: vec![],
            hosts: vec![],
            enumerations: vec![],
        };
        for module in modules {
            let project = |cause| {
                let _ = cause;
                BytecodeLoweringError::InvalidNativeInterface
            };
            contracts.declarations.push((
                module.identity.clone(),
                module.paths(&module.abi, cancel).map_err(project)?,
            ));
            contracts
                .hosts
                .extend(module.paths(&module.host_types, cancel).map_err(project)?);
            contracts.enumerations.extend(
                module
                    .paths(&module.enumerations, cancel)
                    .map_err(project)?,
            );
        }
        Ok(contracts)
    }

    pub(super) fn catalog(
        &self,
        cancel: &CancellationToken,
    ) -> Result<ProofCatalog<'_>, BytecodeLoweringError> {
        let contracts = self.declarations.iter().flat_map(|(identity, abi)| {
            abi.public_items
                .iter()
                .filter_map(|item| {
                    let PublicItem::Trait(contract) = item else {
                        return None;
                    };
                    Some((
                        DefinitionPath {
                            module: identity.clone(),
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
                    abi.trait_contracts
                        .iter()
                        .map(|contract| (contract.declaration.clone(), &contract.abi)),
                )
        });
        ProofCatalog::new(
            self.declarations
                .iter()
                .flat_map(|(_, abi)| &abi.public_items)
                .filter_map(|item| {
                    let PublicItem::InterfaceTable(table) = item else {
                        return None;
                    };
                    (!table.host_bridge).then_some(Implementation::Interface(table.as_ref()))
                })
                .collect(),
            self.hosts.iter().collect(),
            &self.enumerations,
            contracts,
            self.declarations
                .iter()
                .flat_map(|(_, abi)| &abi.native_declarations),
            cancel,
        )
        .map_err(|_| BytecodeLoweringError::InvalidNativeInterface)
    }
}
