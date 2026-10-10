//! Native implementation identity is resolved before the supplying program is published.
use crate::{
    Runtime,
    error::RuntimeError,
    module::{LoadedModule, execution::ExecutionFunction, linked_execution::LinkedPrimitive},
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
                    // The fixed body has one String input and a usize result. No
                    // metadata/heap edges or callable capture survive in this record;
                    // the admitted program root pins the exact installed binding.
                    let [source] = args.as_slice() else {
                        return Err(RuntimeError::module_validation("primitive call arity"));
                    };
                    let location = |register: Register| {
                        prepared
                            .registers
                            .location(register.index())
                            .ok_or_else(|| {
                                RuntimeError::module_validation("primitive operand location")
                            })
                    };
                    Ok(Some(LinkedPrimitive {
                        operation,
                        source: location(*source)?,
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
