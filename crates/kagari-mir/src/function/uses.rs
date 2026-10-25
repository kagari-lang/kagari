//! Checked callable uses extracted once from a newly completed body.

use crate::{
    function::MirFunction,
    instruction::{CallTarget, ConstructionTarget, Instruction},
};
use kagari_common::cancellation::CancellationToken;
use kagari_contract::{
    callable::witness::OperationWitness, native_import::NativeImport,
    types::ConcreteFunctionIdentity, updates::BodyApplication,
};
use kagari_types::{callable::CallableImplementation, ty::substitution::TypeTransformError};

/// Scheduling facts, not proof authority or a request to specialize shared code.
/// Native imports borrow the body; all identity/application records are owned.
#[derive(Default)]
pub struct CallableUses<'a> {
    pub functions: Vec<ConcreteFunctionIdentity>,
    pub shared: Vec<BodyApplication>,
    pub interfaces: Vec<ConcreteFunctionIdentity>,
    pub natives: Vec<&'a NativeImport>,
}

impl MirFunction {
    /// Extract source/shared calls, interface allocations and nested selected uses.
    /// Local numeric targets already belong to the enclosing module's worklist.
    /// The caller decides whether a semantic target is already materialized.
    pub fn callable_uses(
        &self,
        cancel: &CancellationToken,
    ) -> Result<CallableUses<'_>, TypeTransformError> {
        let mut uses = CallableUses::default();
        for instruction in self.blocks.iter().flat_map(|block| &block.instructions) {
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            match instruction {
                Instruction::Call {
                    callee: CallTarget::Shared(call),
                    ..
                } if call.implementation == CallableImplementation::Script => {
                    uses.shared.push(BodyApplication {
                        body: call.instance.clone(),
                        arguments: call.arguments.clone(),
                    });
                }
                Instruction::Call {
                    callee: CallTarget::Shared(call),
                    ..
                } => {
                    uses.functions.push(call.instance.clone());
                }
                Instruction::Call {
                    callee: CallTarget::SourceFunction(call),
                    ..
                } => {
                    uses.functions.push(ConcreteFunctionIdentity {
                        declaration: call.declaration,
                        arguments: call.arguments.clone(),
                    });
                }
                Instruction::Call {
                    callee: CallTarget::Native(import),
                    ..
                } => {
                    uses.natives.push(import);
                }
                Instruction::MakeInterface {
                    target:
                        ConstructionTarget::Declared {
                            implementation,
                            arguments,
                            ..
                        },
                    ..
                } => uses.interfaces.push(ConcreteFunctionIdentity {
                    declaration: *implementation,
                    arguments: arguments.clone(),
                }),
                _ => {}
            }
            for operation in instruction.operation_witnesses() {
                cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
                if let OperationWitness::Selected(call) = operation {
                    uses.functions.push(call.instance.clone());
                }
            }
        }
        Ok(uses)
    }
}
