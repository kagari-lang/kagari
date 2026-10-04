//! Resolve shared entries by verified executable slots and recheck call applications.
use crate::{
    instruction::{BytecodeInstruction, CallTarget},
    module::{BytecodeModule, CallableTarget},
    program::BytecodeProgram,
    trait_bounds::callables,
};
use kagari_common::cancellation::CancellationToken;
use kagari_contract::{
    callable::generic::GenericBody,
    native_import::NativeSignature,
    types::{ConcreteFunctionIdentity, proofs::ProofCatalog},
};
use kagari_types::{callable::CallableImplementation, ty::substitution::TypeTransformError};

pub(crate) struct SharedEntry<'a> {
    pub identity: &'a ConcreteFunctionIdentity,
    pub body: &'a GenericBody,
    pub signature: NativeSignature,
    pub implementation: CallableImplementation,
}

pub(crate) fn entry(module: &BytecodeModule, target: CallableTarget) -> Option<SharedEntry<'_>> {
    Some(match target {
        CallableTarget::Script(target) => {
            let function = module.functions.get(target.index())?;
            let semantic = &function.metadata.semantic;
            SharedEntry {
                identity: function.identity.as_ref()?,
                body: semantic.generic.as_ref()?,
                signature: NativeSignature {
                    params: (0..function.metadata.params.len())
                        .map(|index| semantic.params.get(&index).cloned())
                        .collect::<Option<_>>()?,
                    result: semantic.result.clone()?,
                },
                implementation: CallableImplementation::Script,
            }
        }
        CallableTarget::Native(target) => {
            let import = module.native_imports.get(target.index())?;
            if import.host.is_some() {
                return None;
            }
            SharedEntry {
                identity: &import.instance,
                body: import.generic.as_ref()?,
                signature: import.signature.clone(),
                implementation: CallableImplementation::Native(import.binding.clone()),
            }
        }
    })
}

pub(super) fn valid(
    module: &BytecodeModule,
    closure: &[&BytecodeModule],
    program: Option<&BytecodeProgram>,
    catalog: &ProofCatalog<'_>,
    cancel: &CancellationToken,
) -> Result<bool, TypeTransformError> {
    for function in &module.functions {
        for instruction in &function.instructions {
            let BytecodeInstruction::Call {
                callee:
                    CallTarget::Shared {
                        module: owner,
                        target,
                        contract,
                    },
                ..
            } = instruction
            else {
                continue;
            };
            let Some(owner) = program
                .and_then(|program| program.modules.get(owner.index()))
                .or_else(|| (program.is_none() && owner.index() == 0).then_some(module))
            else {
                return Ok(false);
            };
            let Some(entry) = entry(owner, *target) else {
                return Ok(false);
            };
            let body = function.metadata.semantic.generic.as_ref();
            if *entry.identity != contract.instance
                || entry.implementation != contract.implementation
                || !contract.check(
                    entry.body,
                    &entry.signature,
                    catalog,
                    body.map_or(&[], |body| body.parameters.as_slice()),
                    body.map_or(&[], |body| body.bounds.as_slice()),
                    cancel,
                )?
            {
                return Ok(false);
            }
            for operation in &contract.operations {
                if !callables::witness_valid(operation, closure) {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}
