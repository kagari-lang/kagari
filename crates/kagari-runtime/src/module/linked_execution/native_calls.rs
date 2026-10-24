//! Native implementation identity is resolved before the supplying program is published.
use crate::{
    Runtime,
    error::RuntimeError,
    module::{
        ExecutableModule,
        execution::ExecutionFunction,
        linked_execution::{
            LinkedNativeBody, LinkedNativeCall, LinkedNativeCallback, LinkedVectorPrimitive,
        },
    },
    native::primitive::NativePrimitive,
};
use kagari_bytecode::{
    instruction::{BytecodeInstruction, CallTarget, Slot},
    module::BytecodeFunction,
};

impl Runtime {
    pub(super) fn link_native_calls(
        &self,
        owner: &ExecutableModule,
        function: &BytecodeFunction,
        prepared: &ExecutionFunction,
    ) -> Result<Box<[LinkedNativeCall]>, RuntimeError> {
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
                    // The supplying record already owns this exact closed native
                    // signature. Its type provenance belongs to the pinned program.
                    if args.len() != binding.signature.params.len() {
                        return Err(RuntimeError::module_validation("native call arity"));
                    }
                    let location = |register: Slot| {
                        prepared
                            .frame
                            .slot_layout
                            .location(register.index())
                            .ok_or_else(|| {
                                RuntimeError::module_validation("native operand location")
                            })
                    };
                    let body = match binding.closed_primitive() {
                        Some(NativePrimitive::StringByteLength) => {
                            LinkedNativeBody::StringByteLength(location(args[0])?)
                        }
                        Some(
                            operation @ (NativePrimitive::VecIndex
                            | NativePrimitive::VecSet
                            | NativePrimitive::VecSetFluent),
                        ) => LinkedNativeBody::Vector(LinkedVectorPrimitive {
                            operation,
                            arguments: args
                                .iter()
                                .copied()
                                .map(location)
                                .collect::<Result<_, _>>()?,
                            function: binding,
                        }),
                        None => LinkedNativeBody::Callback(LinkedNativeCallback {
                            arguments: args
                                .iter()
                                .copied()
                                .map(location)
                                .collect::<Result<_, _>>()?,
                            function: binding,
                        }),
                    };
                    Ok(LinkedNativeCall {
                        body,
                        destination: dst.map(location).transpose()?,
                    })
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
