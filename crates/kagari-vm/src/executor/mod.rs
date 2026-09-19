mod aggregate_ops;
mod dispatch;
mod value_ops;

use kagari_ir::bytecode::{BytecodeInstruction, FunctionRef, ModuleRef};
use kagari_runtime::{LoadedModule, Runtime, value::Value};

use crate::error::VmError;
use kagari_runtime::{ExecutionFrame, ExecutionStack};

pub(crate) struct Executor<'a> {
    runtime: &'a Runtime,
    loaded: &'a LoadedModule,
    stack: ExecutionStack,
}

impl<'a> Executor<'a> {
    pub(crate) fn new(
        runtime: &'a Runtime,
        loaded: &'a LoadedModule,
        entry: FunctionRef,
        args: &[Value],
    ) -> Result<Self, VmError> {
        let module = &loaded.bytecode;
        let function = module
            .functions
            .get(entry.index())
            .ok_or(VmError::InvalidFunctionRef(entry))?;

        let mut executor = Self {
            runtime,
            loaded,
            stack: runtime.enter_execution_stack(loaded)?,
        };
        executor.push_frame(loaded.slot(), function, args, None)?;
        Ok(executor)
    }

    pub(crate) fn run(&mut self) -> Result<Value, VmError> {
        loop {
            self.runtime.gc_safepoint().map_err(VmError::RuntimeError)?;
            self.current_frame_mut()?.prepare_instruction();
            self.runtime
                .observe_execution(kagari_runtime::ExecutionEvent::BeforeInstruction)?;

            let instruction = {
                let mut frame = self.current_frame_mut()?;
                frame.next_instruction()
            };

            let Some(instruction) = instruction else {
                return Ok(Value::Unit);
            };

            self.runtime
                .consume_instruction_step()
                .map_err(VmError::RuntimeError)?;

            match instruction {
                BytecodeInstruction::Return(value) => {
                    let value = match value {
                        Some(register) => self.current_frame()?.read_register(register)?,
                        None => Value::Unit,
                    };
                    let return_dst = self.current_frame()?.return_dst();
                    self.pop_frame()?;
                    if !self.stack.is_empty()? {
                        let mut frame = self.current_frame_mut()?;
                        if let Some(dst) = return_dst {
                            frame.write_register(dst, value)?;
                        }
                    } else {
                        return Ok(value);
                    }
                }
                instruction => {
                    if let Err(error) = self.dispatch_instruction(instruction) {
                        self.runtime
                            .observe_execution(kagari_runtime::ExecutionEvent::Trap)?;
                        return Err(error);
                    }
                }
            }
        }
    }

    pub(crate) fn current_module(
        &self,
    ) -> Result<&'a kagari_runtime::module::LinkedModule, VmError> {
        self.loaded
            .member_data(self.current_frame()?.module())
            .ok_or(VmError::UnsupportedInstruction("invalid module slot"))
    }
    pub(crate) fn current_loaded(&self) -> Result<LoadedModule, VmError> {
        self.loaded
            .member(self.current_frame()?.module())
            .ok_or(VmError::UnsupportedInstruction("invalid module slot"))
    }
    pub(crate) fn current_frame(&self) -> Result<std::cell::Ref<'_, ExecutionFrame>, VmError> {
        Ok(self.stack.current()?)
    }

    pub(crate) fn current_frame_mut(
        &self,
    ) -> Result<std::cell::RefMut<'_, ExecutionFrame>, VmError> {
        Ok(self.stack.current_mut()?)
    }

    pub(crate) fn push_frame(
        &mut self,
        module: ModuleRef,
        function: &'a kagari_ir::bytecode::BytecodeFunction,
        args: &[Value],
        return_dst: Option<kagari_ir::bytecode::Register>,
    ) -> Result<(), VmError> {
        Ok(self.stack.push(module, function.id, args, return_dst)?)
    }

    fn pop_frame(&mut self) -> Result<(), VmError> {
        Ok(self.stack.pop()?)
    }
}
