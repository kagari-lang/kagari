//! Prove ordinary shared call applications against selected executable bodies.
use crate::{
    function::MirModule,
    instruction::{CallTarget, Instruction},
    verify::VerifiedMirModule,
};
use kagari_abi::{
    callable::CallableImplementation,
    native_import::NativeSignature,
    types::{proofs::ProofCatalog, substitution::TypeTransformError},
};
use kagari_common::cancellation::CancellationToken;

pub(super) fn valid(
    caller: &MirModule,
    closure: &[&VerifiedMirModule],
    catalog: &ProofCatalog<'_>,
    cancel: &CancellationToken,
) -> Result<bool, TypeTransformError> {
    for function in &caller.functions {
        for instruction in function.blocks.iter().flat_map(|block| &block.instructions) {
            let Instruction::Call {
                callee: CallTarget::Shared(call),
                ..
            } = instruction
            else {
                continue;
            };
            let (body, signature) = match &call.implementation {
                CallableImplementation::Script => {
                    let Some(target) = closure
                        .iter()
                        .flat_map(|module| &module.functions)
                        .find(|target| target.instance == call.instance)
                    else {
                        return Ok(false);
                    };
                    let Some(body) = &target.semantic.generic else {
                        return Ok(false);
                    };
                    let Some(params) = (0..target.params.len())
                        .map(|index| target.semantic.params.get(&index).cloned())
                        .collect::<Option<_>>()
                    else {
                        return Ok(false);
                    };
                    let Some(result) = target.semantic.result.clone() else {
                        return Ok(false);
                    };
                    (body, NativeSignature { params, result })
                }
                CallableImplementation::Native(binding) => {
                    let Some(import) = caller.native_targets.iter().find(|import| {
                        import.instance == call.instance
                            && import.binding == *binding
                            && import.host.is_none()
                    }) else {
                        return Ok(false);
                    };
                    let Some(body) = &import.generic else {
                        return Ok(false);
                    };
                    (body, import.signature.clone())
                }
                _ => return Ok(false),
            };
            let caller_body = function.semantic.generic.as_ref();
            if !call.check(
                body,
                &signature,
                catalog,
                caller_body.map_or(&[], |body| body.parameters.as_slice()),
                caller_body.map_or(&[], |body| body.bounds.as_slice()),
                cancel,
            )? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}
