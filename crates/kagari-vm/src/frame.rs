use kagari_ir::bytecode::{BytecodeFunction, BytecodeInstruction, JumpTarget, LocalSlot, Register};
use kagari_runtime::value::Value;

use crate::error::VmError;

#[derive(Debug)]
pub(crate) struct Frame<'a> {
    function: &'a BytecodeFunction,
    ip: usize,
    registers: Vec<Value>,
    locals: Vec<Value>,
    return_dst: Option<Register>,
}

impl<'a> Frame<'a> {
    pub(crate) fn new(
        function: &'a BytecodeFunction,
        args: &[Value],
        return_dst: Option<Register>,
    ) -> Self {
        let mut locals = vec![Value::Unit; usize::from(function.local_count)];
        for (slot, value) in args
            .iter()
            .take(usize::from(function.parameter_count))
            .enumerate()
        {
            locals[slot] = value.clone();
        }

        Self {
            function,
            ip: 0,
            registers: vec![Value::Unit; usize::from(function.register_count)],
            locals,
            return_dst,
        }
    }

    pub(crate) fn next_instruction(&mut self) -> Option<&'a BytecodeInstruction> {
        let instruction = self.function.instructions.get(self.ip);
        if instruction.is_some() {
            self.ip += 1;
        }
        instruction
    }

    pub(crate) fn jump_to(&mut self, offset: usize) -> Result<(), VmError> {
        if offset >= self.function.instructions.len() {
            return Err(VmError::InvalidJumpTarget(JumpTarget::new(offset)));
        }
        self.ip = offset;
        Ok(())
    }

    pub(crate) fn read_register(&self, register: Register) -> Result<Value, VmError> {
        self.registers
            .get(register.index())
            .cloned()
            .ok_or(VmError::InvalidRegister(register))
    }

    pub(crate) fn write_register(
        &mut self,
        register: Register,
        value: Value,
    ) -> Result<(), VmError> {
        let slot = self
            .registers
            .get_mut(register.index())
            .ok_or(VmError::InvalidRegister(register))?;
        *slot = value;
        Ok(())
    }

    pub(crate) fn read_local(&self, local: LocalSlot) -> Result<Value, VmError> {
        self.locals
            .get(local.index())
            .cloned()
            .ok_or(VmError::InvalidLocal(local))
    }

    pub(crate) fn write_local(&mut self, local: LocalSlot, value: Value) -> Result<(), VmError> {
        let slot = self
            .locals
            .get_mut(local.index())
            .ok_or(VmError::InvalidLocal(local))?;
        *slot = value;
        Ok(())
    }

    pub(crate) fn return_dst(&self) -> Option<Register> {
        self.return_dst
    }
}
