//! Link provider-qualified imports against carried declarations and witnesses.
use crate::{CallTarget, Instruction, MirModule, VerifiedMirModule};
use kagari_abi::{
    callable::NativeCall,
    types::{PublicAbiItem, proofs::ProofCatalog, substitution::TypeTransformError},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionId, DefinitionKind, DefinitionPathSegment},
};

pub(super) fn validate(
    caller: &MirModule,
    closure: &[&VerifiedMirModule],
    cancel: &CancellationToken,
) -> Result<bool, TypeTransformError> {
    let tables = closure
        .iter()
        .flat_map(|module| &module.abi.public_items)
        .filter_map(|item| {
            if let PublicAbiItem::InterfaceTable(table) = item {
                (!table.host_bridge && !table.native_bridge).then_some(table.as_ref())
            } else {
                None
            }
        })
        .collect();
    let declarations = closure.iter().flat_map(|module| {
        let public = module.abi.public_items.iter().filter_map(|item| {
            let PublicAbiItem::Trait(record) = item else {
                return None;
            };
            Some((
                DefinitionId {
                    module: module.identity.clone(),
                    path: vec![DefinitionPathSegment {
                        kind: DefinitionKind::Trait,
                        name: record.name.clone(),
                        occurrence: 0,
                    }],
                },
                record,
            ))
        });
        public.chain(
            module
                .abi
                .trait_contracts
                .iter()
                .map(|contract| (contract.declaration.clone(), &contract.abi)),
        )
    });
    let catalog = ProofCatalog::new(
        tables,
        closure
            .iter()
            .flat_map(|module| &module.host_types)
            .collect(),
        closure.iter().flat_map(|module| &module.enumerations),
        declarations,
        cancel,
    )?;
    if !catalog.overrides_valid(cancel)? {
        return Ok(false);
    }
    for import in caller
        .functions
        .iter()
        .flat_map(|function| &function.blocks)
        .flat_map(|block| &block.instructions)
        .filter_map(|instruction| {
            if let Instruction::Call {
                callee: CallTarget::Native(NativeCall::Engine(import)),
                ..
            } = instruction
            {
                Some(import)
            } else {
                None
            }
        })
    {
        let Some(owner) = closure
            .iter()
            .find(|owner| owner.identity == import.instance.declaration.module)
        else {
            return Ok(false);
        };
        let Some(declaration) = owner
            .abi
            .native_declarations
            .iter()
            .find(|declaration| declaration.declaration == import.instance.declaration)
        else {
            return Ok(false);
        };
        if !import.matches_declaration(
            declaration,
            &catalog,
            |id| {
                closure
                    .iter()
                    .find(|owner| owner.identity == id.module)?
                    .abi
                    .public_items
                    .iter()
                    .find_map(|item| {
                        if let PublicAbiItem::InterfaceTable(table) = item {
                            (table.declaration == *id).then_some(table.as_ref())
                        } else {
                            None
                        }
                    })
            },
            cancel,
        )? {
            return Ok(false);
        }
    }
    Ok(true)
}
