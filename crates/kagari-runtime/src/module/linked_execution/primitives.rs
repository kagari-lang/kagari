//! Native implementation identity is resolved before the supplying program is published.
use crate::{
    Runtime,
    error::RuntimeError,
    module::{
        LoadedModule,
        execution::ExecutionFunction,
        linked_execution::{LinkedPrimitive, LinkedPrimitiveBody, LinkedVectorPrimitive},
    },
    native::primitive::NativePrimitive,
};
use kagari_bytecode::{
    instruction::{BytecodeInstruction, CallTarget, Register},
    module::BytecodeFunction,
};
use kagari_common::identity::table::DefinitionId;

impl Runtime {
    pub(super) fn link_primitives(
        &self,
        owner: &LoadedModule,
        function: &BytecodeFunction<DefinitionId>,
        prepared: &ExecutionFunction,
    ) -> Result<Box<[Option<LinkedPrimitive>]>, RuntimeError> {
        let operations: Box<[_]> = function
            .instructions
            .iter()
            .filter_map(|instruction| {
                let BytecodeInstruction::Call {
                    dst,
                    callee: CallTarget::Native(import),
                    args,
                } = instruction
                else {
                    return None;
                };
                Some((|| {
                    let binding = self.modules.native_binding(owner, *import).ok_or_else(|| {
                        RuntimeError::module_validation("missing linked native implementation")
                    })?;
                    let Some(operation) = binding.closed_primitive() else {
                        return Ok(None);
                    };
                    // The supplying record already owns this exact closed native
                    // signature. Its type provenance belongs to the pinned program.
                    if args.len() != binding.signature.params.len() {
                        return Err(RuntimeError::module_validation("primitive call arity"));
                    }
                    let location = |register: Register| {
                        prepared
                            .registers
                            .location(register.index())
                            .ok_or_else(|| {
                                RuntimeError::module_validation("primitive operand location")
                            })
                    };
                    let body = match operation {
                        NativePrimitive::StringByteLength => {
                            LinkedPrimitiveBody::StringByteLength(location(args[0])?)
                        }
                        NativePrimitive::VecIndex
                        | NativePrimitive::VecSet
                        | NativePrimitive::VecSetFluent => {
                            LinkedPrimitiveBody::Vector(LinkedVectorPrimitive {
                                operation,
                                arguments: args
                                    .iter()
                                    .copied()
                                    .map(location)
                                    .collect::<Result<_, _>>()?,
                                function: binding,
                            })
                        }
                    };
                    Ok(Some(LinkedPrimitive {
                        body,
                        destination: dst.map(location).transpose()?,
                    }))
                })())
            })
            .collect::<Result<_, RuntimeError>>()?;
        if operations.len() != prepared.native_calls {
            return Err(RuntimeError::module_validation(
                "native execution ordinal mismatch",
            ));
        }
        Ok(operations)
    }
}
