mod aggregate_ops;
mod dispatch;
mod value_ops;

use kagari_ir::bytecode::{BytecodeInstruction, FunctionRef, ModuleRef};
use kagari_runtime::{LoadedModule, RootedInterfaceMethod, Runtime, value::Value};

use crate::error::VmError;
use kagari_runtime::{ExecutionFrame, ExecutionStack};

pub(crate) struct Executor<'a> {
    runtime: &'a Runtime,
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
        module
            .functions
            .get(entry.index())
            .ok_or(VmError::InvalidFunctionRef(entry))?;

        let mut executor = Self {
            runtime,
            stack: runtime.enter_execution_stack(loaded)?,
        };
        executor.push_frame(loaded.slot(), entry, args, None)?;
        Ok(executor)
    }

    pub(crate) fn new_interface(
        runtime: &'a Runtime,
        method: RootedInterfaceMethod,
        args: &[Value],
    ) -> Result<Self, VmError> {
        let loaded = method.implementation().clone();
        let stack = runtime.enter_execution_stack(&loaded)?;
        stack.push_interface_method(runtime, method, args, None)?;
        Ok(Self { runtime, stack })
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
                return Err(VmError::RuntimeError(
                    self.runtime
                        .quarantine_execution_invariant("verified function fell through"),
                ));
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
                    if let Some(method) = self.current_frame()?.interface_method()
                        && let Err(error) = self
                            .runtime
                            .validate_interface_method_result(method, &value)
                    {
                        self.runtime
                            .observe_execution(kagari_runtime::ExecutionEvent::Trap)?;
                        return Err(VmError::RuntimeError(error));
                    }
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

    pub(crate) fn current_loaded(&self) -> Result<LoadedModule, VmError> {
        Ok(self.current_frame()?.loaded().clone())
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
        function: FunctionRef,
        args: &[Value],
        return_dst: Option<kagari_ir::bytecode::Register>,
    ) -> Result<(), VmError> {
        Ok(self.stack.push(module, function, args, return_dst)?)
    }

    fn pop_frame(&mut self) -> Result<(), VmError> {
        Ok(self.stack.pop()?)
    }
}
