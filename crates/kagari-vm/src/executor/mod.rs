mod aggregate_ops;
mod dispatch;
mod loop_body;
pub(crate) mod native;
mod value_ops;

use kagari_bytecode::{
    instruction::{BytecodeInstruction, Register},
    module::CallableTarget,
    program::ModuleRef,
};
use kagari_contract::ids::FunctionRef;
use kagari_runtime::{
    RootedInterfaceMethod, Runtime,
    frame::{ExecutionFrame, ExecutionStack, transfer::ReturnValue},
    module::LoadedModule,
    session::ExecutionEvent,
    value::Value,
};
use std::cell::{Ref, RefMut};

use crate::{error::VmError, executor::loop_body::LoopExit};

pub(crate) struct Executor<'a> {
    runtime: &'a Runtime,
    stack: ExecutionStack<'a>,
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
        let loaded = method.implementation(runtime)?;
        let stack = runtime.enter_execution_stack(&loaded)?;
        stack.push_interface_method(runtime, method, args, None)?;
        Ok(Self { runtime, stack })
    }

    pub(crate) fn run(&mut self) -> Result<Value, VmError> {
        self.run_inner()
            .map_err(|error| error.with_trace(self.runtime.capture_error_trace()))
    }

    fn run_inner(&mut self) -> Result<Value, VmError> {
        loop {
            let native_return = self.current_frame()?.native_return(self.runtime)?;
            if let Some(value) = native_return {
                let result = self
                    .stack
                    .finish_return(self.runtime, ReturnValue::general(value));
                if let Some(value) = self.report_operation(result.map_err(VmError::RuntimeError))? {
                    return Ok(value);
                }
                continue;
            }
            if self.current_frame()?.has_pending_native_entry() {
                self.runtime.gc_safepoint()?;
                self.runtime
                    .observe_execution(ExecutionEvent::BeforeInstruction)?;
                let result = self
                    .stack
                    .start_native_entry(self.runtime, native::invoke_script)
                    .map_err(VmError::RuntimeError);
                self.report_operation(result)?;
                continue;
            }
            self.current_frame_mut()?.prepare_instruction();
            self.runtime.gc_safepoint().map_err(VmError::RuntimeError)?;
            self.runtime
                .observe_execution(ExecutionEvent::BeforeInstruction)?;

            let result = self.run_cursor();
            match self.report_operation(result)? {
                LoopExit::Safepoint => continue,
                LoopExit::Return(value) => {
                    let result = self.stack.finish_return(self.runtime, value);
                    if let Some(value) =
                        self.report_operation(result.map_err(VmError::RuntimeError))?
                    {
                        return Ok(value);
                    }
                }
                LoopExit::Boundary => {
                    let (loaded, target, pc) = {
                        let frame = self.current_frame()?;
                        (
                            frame.loaded().clone(),
                            frame.target(),
                            frame.instruction_offset(),
                        )
                    };
                    let CallableTarget::Script(function) = target else {
                        return Err(VmError::UnsupportedInstruction(
                            "native boundary in script cursor",
                        ));
                    };
                    let instruction = loaded
                        .bytecode
                        .functions
                        .get(function.index())
                        .and_then(|function| function.instructions.get(pc))
                        .ok_or(VmError::UnsupportedInstruction(
                            "missing boundary instruction",
                        ))?;
                    if let BytecodeInstruction::Return(register) = instruction {
                        let value = register
                            .map(|register| {
                                self.current_frame()?
                                    .read_register(self.runtime, register)
                                    .map_err(VmError::RuntimeError)
                            })
                            .transpose()?
                            .unwrap_or(Value::Unit);
                        let result = self
                            .stack
                            .finish_return(self.runtime, ReturnValue::general(value));
                        if let Some(value) =
                            self.report_operation(result.map_err(VmError::RuntimeError))?
                        {
                            return Ok(value);
                        }
                        continue;
                    }
                    let result = self.dispatch_instruction(instruction);
                    self.report_operation(result)?;
                }
            }
        }
    }

    fn report_operation<T>(&self, result: Result<T, VmError>) -> Result<T, VmError> {
        result.map_err(|error| {
            if let Some(reason) = error.invariant_reason() {
                return VmError::RuntimeError(self.runtime.quarantine_execution_invariant(reason));
            }
            match self.runtime.observe_execution(ExecutionEvent::Trap) {
                Ok(()) => error,
                Err(observer_error) => VmError::RuntimeError(observer_error),
            }
        })
    }

    pub(crate) fn current_loaded(&self) -> Result<LoadedModule, VmError> {
        Ok(self.current_frame()?.loaded().clone())
    }

    pub(crate) fn current_frame(&self) -> Result<Ref<'_, ExecutionFrame>, VmError> {
        Ok(self.stack.current()?)
    }

    pub(crate) fn current_frame_mut(&self) -> Result<RefMut<'_, ExecutionFrame>, VmError> {
        Ok(self.stack.current_mut()?)
    }

    pub(crate) fn push_frame(
        &mut self,
        module: ModuleRef,
        function: FunctionRef,
        args: &[Value],
        return_dst: Option<Register>,
    ) -> Result<(), VmError> {
        Ok(self
            .stack
            .push(self.runtime, module, function, args, return_dst)?)
    }
}
