//! Verify intrinsic interface shims without treating them as new implementations.
use crate::{
    function::MirModule,
    instruction::{CallTarget, Instruction, Terminator},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, table::DefinitionTable},
};
use kagari_contract::{
    callable::adapter::SharedAdapterWitness,
    types::{InterfaceBridge, PublicItem},
};
use kagari_types::{
    declaration::requirement::NativeCallableRequirement,
    ty::{GenericParam, Ty, substitution::TypeTransformError},
};

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
                callee,
                args,
            } = &block.instructions[count]
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
            let Some(member) = member else {
                return Ok(false);
            };
            let target_matches = match callee {
                CallTarget::Function(target) => module
                    .functions
                    .get(target.index())
                    .and_then(|function| function.semantic.protocol_adapter.as_ref())
                    .is_some_and(|required| {
                        required.receiver == table.for_type
                            && &required.interface == interface
                            && required.member == member
                            && required.arguments.is_empty()
                    }),
                CallTarget::Shared(call) => {
                    let Some(body) = &function.semantic.generic else {
                        return Ok(false);
                    };
                    let arguments: Vec<_> =
                        body.parameters.iter().map(GenericParam::as_type).collect();
                    let Some(applied) =
                        table.instantiate(&arguments, &body.parameters, definitions)
                    else {
                        return Ok(false);
                    };
                    let witness = SharedAdapterWitness {
                        requirement: NativeCallableRequirement {
                            receiver: applied.for_type,
                            interface: interface.clone(),
                            member,
                            arguments: Vec::new(),
                        },
                        call: (**call).clone(),
                    };
                    let Some((generic, required, _)) = witness.template(definitions) else {
                        return Ok(false);
                    };
                    module
                        .functions
                        .iter()
                        .find(|target| target.instance == call.instance)
                        .is_some_and(|target| {
                            target.semantic.generic.as_ref() == Some(&generic)
                                && target.semantic.protocol_adapter.as_ref() == Some(&required)
                        })
                }
                _ => false,
            };
            if !forwarded || !target_matches {
                return Ok(false);
            }
        }
    }
    Ok(true)
}
