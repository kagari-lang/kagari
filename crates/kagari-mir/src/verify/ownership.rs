//! Checked identity ownership is attached only after semantic verification.
use crate::{
    analysis::FunctionAnalysis,
    function::MirModule,
    verify::{
        MirVerificationError, MirVerificationErrorKind, ValidatedMirModule, VerifiedMirModule,
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, DefinitionPath,
        mapping::{DefinitionMapper, DefinitionMappingError, DefinitionRecord},
        metadata::DefinitionMetadata,
        table::{DefinitionId, DefinitionTable, DefinitionTableBuilder, DefinitionTableError},
    },
};
use kagari_contract::types::{PublicItem, TraitDef, Ty};
use std::slice;

pub(crate) fn mapping_error(cause: DefinitionMappingError) -> MirVerificationError {
    MirVerificationError {
        function: None,
        block: None,
        instruction: None,
        span: None,
        kind: match cause {
            DefinitionMappingError::Cancelled => MirVerificationErrorKind::Cancelled,
            DefinitionMappingError::Identity(DefinitionTableError::PortableLimit) => {
                MirVerificationErrorKind::Limit {
                    resource: "definitions",
                    limit: 1_000_000,
                }
            }
            DefinitionMappingError::LimitExceeded => MirVerificationErrorKind::Limit {
                resource: "identity mapping",
                limit: 1_000_000,
            },
            _ => MirVerificationErrorKind::InvalidInstance,
        },
    }
}

pub(crate) fn unmapped() -> DefinitionMappingError {
    DefinitionTableError::UnmappedDefinition.into()
}

pub(crate) fn adopt(
    checked: ValidatedMirModule,
    cancel: &CancellationToken,
) -> Result<VerifiedMirModule, MirVerificationError> {
    let scoped = scope_modules(slice::from_ref(&checked.module), cancel).map_err(mapping_error)?;
    let definitions = scoped.definitions().clone();
    let module = scoped.into_records().pop().expect("one checked module");
    let metadata =
        DefinitionMetadata::checked(definitions, module, cancel).map_err(mapping_error)?;
    Ok(VerifiedMirModule {
        metadata,
        analyses: checked.analyses,
    })
}

/// Named ABI members are derived from the checked declaration, including unused
/// marker traits. These entries carry names, never registration authority.
pub(crate) fn scope_modules(
    modules: &[MirModule],
    cancel: &CancellationToken,
) -> Result<DefinitionMetadata<Vec<MirModule<DefinitionId>>>, DefinitionMappingError> {
    let mut builder = DefinitionTableBuilder::new()?;
    for module in modules {
        for item in &module.abi.public_items {
            cancel
                .check()
                .map_err(|_| DefinitionMappingError::Cancelled)?;
            match item {
                PublicItem::Trait(contract) => {
                    let root = builder.intern_root(&module.identity)?;
                    let owner =
                        builder.intern_child(root, DefinitionKind::Trait, &contract.name, 0)?;
                    trait_members(&mut builder, owner, contract)?;
                }
                PublicItem::InterfaceTable(table) => {
                    let owner = builder.intern_path(&table.declaration)?;
                    for method in &table.methods {
                        builder.intern_child(owner, DefinitionKind::Method, &method.name, 0)?;
                    }
                    if let Ty::Trait(interface) = &table.trait_type {
                        let owner = builder.intern_path(&interface.declaration)?;
                        for method in &table.methods {
                            builder.intern_child(owner, DefinitionKind::Method, &method.name, 0)?;
                        }
                    }
                }
                _ => {}
            }
        }
        for contract in &module.abi.trait_contracts {
            let owner = builder.intern_path(&contract.declaration)?;
            trait_members(&mut builder, owner, &contract.abi)?;
        }
    }
    let records = modules
        .iter()
        .map(|module| {
            module.map_identities(&mut DefinitionMapper::new(
                &mut |path| builder.intern_path(path).map_err(Into::into),
                cancel,
            ))
        })
        .collect::<Result<Vec<_>, _>>()?;
    DefinitionMetadata::checked(builder.freeze(), records, cancel)
}

fn trait_members(
    builder: &mut DefinitionTableBuilder,
    owner: DefinitionId,
    contract: &TraitDef,
) -> Result<(), DefinitionMappingError> {
    for method in &contract.methods {
        builder.intern_child(owner, DefinitionKind::Method, &method.name, 0)?;
    }
    Ok(())
}

pub(crate) fn retain(
    definitions: DefinitionTable,
    records: MirModule<DefinitionId>,
    analyses: Vec<FunctionAnalysis>,
    cancel: &CancellationToken,
) -> Result<VerifiedMirModule, MirVerificationError> {
    let metadata =
        DefinitionMetadata::checked(definitions, records, cancel).map_err(mapping_error)?;
    Ok(VerifiedMirModule { metadata, analyses })
}

impl VerifiedMirModule {
    pub fn definitions(&self) -> &DefinitionTable {
        self.metadata.definitions()
    }

    pub fn to_unverified(
        &self,
        cancel: &CancellationToken,
    ) -> Result<MirModule, MirVerificationError> {
        self.metadata.to_paths(cancel).map_err(mapping_error)
    }

    /// Mutable authoring extraction discards the verification seal.
    pub fn into_unverified(self) -> MirModule {
        self.to_unverified(&CancellationToken::default())
            .expect("checked MIR identity ownership")
    }

    pub fn paths<T: DefinitionRecord<DefinitionId>>(
        &self,
        records: &T,
        cancel: &CancellationToken,
    ) -> Result<T::Rebind<DefinitionPath>, DefinitionMappingError> {
        records.map_identities(&mut DefinitionMapper::new(
            &mut |id| Ok(self.definitions().resolve(*id)?.to_path()),
            cancel,
        ))
    }

    pub fn scope<T: DefinitionRecord<DefinitionPath>>(
        &self,
        records: &T,
        cancel: &CancellationToken,
    ) -> Result<T::Rebind<DefinitionId>, DefinitionMappingError> {
        records.map_identities(&mut DefinitionMapper::new(
            &mut |path| self.definitions().lookup(path).ok_or_else(unmapped),
            cancel,
        ))
    }
}
