//! Verify intrinsic interface shims without treating them as new implementations.
use crate::{
    function::MirModule,
    instruction::{CallTarget, Instruction, Terminator},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, table::DefinitionTable},
};
use kagari_contract::types::{InterfaceBridge, PublicItem};
use kagari_types::ty::{Ty, substitution::TypeTransformError};

pub(super) fn valid(
    module: &MirModule,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<bool, TypeTransformError> {
    for item in &module.abi.public_items {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        let PublicItem::InterfaceTable(table) = item else {
            continue;
        };
        if table.bridge != Some(InterfaceBridge::Intrinsic) {
            continue;
        }
        let Ty::Trait(interface) = &table.trait_type else {
            return Ok(false);
        };
        for method in &table.methods {
            let Some(id) = definitions.lookup_child(
                table.declaration,
                DefinitionKind::Method,
                &method.name,
                0,
            ) else {
                return Ok(false);
            };
            let Some(function) = module.functions.iter().find(|function| {
                function.instance.declaration == id && function.instance.arguments.is_empty()
            }) else {
                return Ok(false);
            };
            let [block] = function.blocks.as_slice() else {
                return Ok(false);
            };
            let count = function.params.len();
            if block.instructions.len() != count + 1 {
                return Ok(false);
            }
            let Instruction::Call {
                dst: Some(result),
                callee: CallTarget::Function(target),
                args,
            } = &block.instructions[count]
            else {
                return Ok(false);
            };
            let Some(required) = module
                .functions
                .get(target.index())
                .and_then(|function| function.semantic.protocol_adapter.as_ref())
            else {
                return Ok(false);
            };
            let forwarded = args.len() == count
                && block.instructions[..count].iter().zip(args).enumerate().all(|(index, (instruction, arg))| {
                    matches!(instruction, Instruction::LoadLocal { dst, local } if dst == arg && local.index() == index)
                })
                && matches!(&block.terminator, Some(Terminator::Return(Some(value))) if value == result);
            let member = definitions.lookup_child(
                interface.declaration,
                DefinitionKind::Method,
                &method.name,
                0,
            );
            if !forwarded
                || required.receiver != table.for_type
                || &required.interface != interface
                || !required.arguments.is_empty()
                || member != Some(required.member)
            {
                return Ok(false);
            }
        }
    }
    Ok(true)
}
