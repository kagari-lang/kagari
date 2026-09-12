use kagari_ir::bytecode::{BytecodeFunction, BytecodeInstruction, JumpTarget, LocalSlot, Register};
use kagari_runtime::gc::{GcHeap, RootSet};
use kagari_runtime::value::Value;

use crate::error::VmError;

#[derive(Debug)]
pub(crate) struct Frame<'a> {
    pub(crate) module: kagari_ir::bytecode::ModuleRef,
    function: &'a BytecodeFunction,
    ip: usize,
    heap: &'a GcHeap,
    slots: RootSet,
    register_count: usize,
    return_dst: Option<Register>,
}

impl<'a> Frame<'a> {
    pub(crate) fn new(
        heap: &'a GcHeap,
        module: kagari_ir::bytecode::ModuleRef,
        function: &'a BytecodeFunction,
        args: &[Value],
        return_dst: Option<Register>,
    ) -> Result<Self, VmError> {
        let expected = usize::from(function.parameter_count);
        if args.len() != expected {
            return Err(VmError::InvalidFrameArity {
                function: function.id,
                expected,
                found: args.len(),
            });
        }
        let register_count = usize::from(function.register_count);
        let mut slots = vec![Value::Unit; register_count + usize::from(function.local_count)];
        for (slot, value) in args.iter().enumerate() {
            slots[register_count + slot] = value.clone();
        }

        Ok(Self {
            module,
            function,
            ip: 0,
            heap,
            slots: heap
                .root_execution_values(slots)
                .ok_or(VmError::UnsupportedInstruction("invalid heap argument"))?,
            register_count,
            return_dst,
        })
    }

    pub(crate) fn next_instruction(&mut self) -> Option<&'a BytecodeInstruction> {
        let instruction = self.function.instructions.get(self.ip);
        if instruction.is_some() {
            self.ip += 1;
        }
        instruction
    }

    pub(crate) fn function(&self) -> &'a BytecodeFunction {
        self.function
    }

    pub(crate) fn instruction_offset(&self) -> usize {
        self.ip
    }

    pub(crate) fn jump_to(&mut self, offset: usize) -> Result<(), VmError> {
        if offset >= self.function.instructions.len() {
            return Err(VmError::InvalidJumpTarget(JumpTarget::new(offset)));
        }
        self.ip = offset;
        Ok(())
    }

    pub(crate) fn read_register(&self, register: Register) -> Result<Value, VmError> {
        if register.index() >= self.register_count {
            return Err(VmError::InvalidRegister(register));
        }
        self.slots
            .get(register.index())
            .ok_or(VmError::InvalidRegister(register))
    }

    pub(crate) fn write_register(
        &mut self,
        register: Register,
        value: Value,
    ) -> Result<(), VmError> {
        if register.index() >= self.register_count {
            return Err(VmError::InvalidRegister(register));
        }
        self.slots
            .set(self.heap, register.index(), value)
            .ok_or(VmError::InvalidRegister(register))
    }

    pub(crate) fn read_local(&self, local: LocalSlot) -> Result<Value, VmError> {
        self.slots
            .get(self.register_count + local.index())
            .ok_or(VmError::InvalidLocal(local))
    }

    pub(crate) fn write_local(&mut self, local: LocalSlot, value: Value) -> Result<(), VmError> {
        self.slots
            .set(self.heap, self.register_count + local.index(), value)
            .ok_or(VmError::InvalidLocal(local))
    }

    pub(crate) fn return_dst(&self) -> Option<Register> {
        self.return_dst
    }
}
