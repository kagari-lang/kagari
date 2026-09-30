mod aggregate_ops;
mod dispatch;
mod value_ops;

use kagari_abi::{budget::LogicalBudgetCharge, ids::FunctionRef};
use kagari_bytecode::{BytecodeInstruction, ModuleRef, Register};
use kagari_runtime::{
    ExecutionEvent, ExecutionFrame, ExecutionStack, LoadedModule, NativeProgress,
    RootedInterfaceMethod, Runtime, value::Value,
};
use std::cell::{Ref, RefMut};

use crate::error::VmError;

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
        self.run_inner()
            .map_err(|error| error.with_trace(self.runtime.capture_error_trace()))
    }
    fn run_inner(&mut self) -> Result<Value, VmError> {
        loop {
            if self.stack.has_native_continuation()? {
                self.runtime.gc_safepoint()?;
                self.runtime
                    .observe_execution(ExecutionEvent::BeforeInstruction)?;
                self.runtime
                    .consume_logical_charge(LogicalBudgetCharge::Step)?;
                let result = self
                    .stack
                    .advance_native(self.runtime)
                    .and_then(|progress| match progress {
                        NativeProgress::Continue | NativeProgress::Finished => Ok(()),
                        NativeProgress::Callback(request) => {
                            self.stack.push_native_callback(self.runtime, request)
                        }
                    });
                self.report_operation(result.map_err(VmError::RuntimeError))?;
                continue;
            }
            self.current_frame_mut()?.prepare_instruction();
            self.runtime.gc_safepoint().map_err(VmError::RuntimeError)?;
            self.runtime
                .observe_execution(ExecutionEvent::BeforeInstruction)?;

            let instruction = {
                let mut frame = self.current_frame_mut()?;
                frame.next_instruction()
            };

            let Some((instruction, charge)) = instruction else {
                return Err(VmError::RuntimeError(
                    self.runtime
                        .quarantine_execution_invariant("verified function fell through"),
                ));
            };

            self.runtime
                .consume_logical_charge(charge)
                .map_err(VmError::RuntimeError)?;

            match instruction {
                BytecodeInstruction::Return(value) => {
                    let value = match value {
                        Some(register) => self.current_frame()?.read_register(register)?,
                        None => Value::Unit,
                    };
                    let result = self.stack.finish_return(self.runtime, value);
                    if let Some(value) =
                        self.report_operation(result.map_err(VmError::RuntimeError))?
                    {
                        return Ok(value);
                    }
                }
                instruction => {
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
        Ok(self.stack.push(module, function, args, return_dst)?)
    }
}
