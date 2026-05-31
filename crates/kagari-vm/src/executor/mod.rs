mod aggregate_ops;
mod dispatch;
mod value_ops;

use std::cell::RefMut;

use kagari_ir::bytecode::{BytecodeInstruction, BytecodeModule, FunctionRef};
use kagari_runtime::{ModuleInstance, Runtime, value::Value};

use crate::debug::DebugSession;
use crate::error::VmError;
use crate::frame::Frame;

#[derive(Debug)]
pub(crate) struct Executor<'a> {
    runtime: &'a Runtime,
    module: &'a BytecodeModule,
    module_instance: RefMut<'a, ModuleInstance>,
    frames: Vec<Frame<'a>>,
    debug_session: Option<&'a mut DebugSession>,
}

impl<'a> Executor<'a> {
    pub(crate) fn new(
        runtime: &'a Runtime,
        module: &'a BytecodeModule,
        module_instance: RefMut<'a, ModuleInstance>,
        entry: FunctionRef,
        debug_session: Option<&'a mut DebugSession>,
    ) -> Result<Self, VmError> {
        let function = module
            .functions
            .get(entry.index())
            .ok_or(VmError::InvalidFunctionRef(entry))?;

        let mut executor = Self {
            runtime,
            module,
            module_instance,
            frames: Vec::new(),
            debug_session,
        };
        executor.push_frame(function, &[], None)?;
        Ok(executor)
    }

    pub(crate) fn run(&mut self) -> Result<Value, VmError> {
        loop {
            if let Some(debug_session) = self.debug_session.as_deref_mut() {
                debug_session.before_instruction(
                    self.runtime,
                    &self.module_instance.name,
                    self.module_instance.id,
                    self.module_instance.epoch.0,
                    &self.frames,
                )?;
            }

            let instruction = {
                let frame = self.current_frame_mut()?;
                frame.next_instruction().cloned()
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
                    if let Some(frame) = self.frames.last_mut() {
                        if let Some(dst) = return_dst {
                            frame.write_register(dst, value)?;
                        }
                    } else {
                        return Ok(value);
                    }
                }
                instruction => {
                    if let Err(error) = self.dispatch_instruction(instruction) {
                        if let Some(debug_session) = self.debug_session.as_deref_mut() {
                            debug_session.record_trap(
                                self.runtime,
                                self.module_instance.id,
                                self.module_instance.epoch.0,
                                &self.frames,
                            )?;
                        }
                        return Err(error);
                    }
                }
            }
        }
    }

    pub(crate) fn current_frame(&self) -> Result<&Frame<'a>, VmError> {
        self.frames
            .last()
            .ok_or(VmError::UnsupportedInstruction("missing_frame"))
    }

    pub(crate) fn current_frame_mut(&mut self) -> Result<&mut Frame<'a>, VmError> {
        self.frames
            .last_mut()
            .ok_or(VmError::UnsupportedInstruction("missing_frame"))
    }

    pub(crate) fn push_frame(
        &mut self,
        function: &'a kagari_ir::bytecode::BytecodeFunction,
        args: &[Value],
        return_dst: Option<kagari_ir::bytecode::Register>,
    ) -> Result<(), VmError> {
        self.runtime.enter_call().map_err(VmError::RuntimeError)?;
        match Frame::new(function, args, return_dst) {
            Ok(frame) => {
                self.frames.push(frame);
                Ok(())
            }
            Err(error) => {
                self.runtime.leave_call();
                Err(error)
            }
        }
    }

    fn pop_frame(&mut self) -> Result<(), VmError> {
        self.frames
            .pop()
            .ok_or(VmError::UnsupportedInstruction("missing_frame"))?;
        self.runtime.leave_call();
        Ok(())
    }
}

impl Drop for Executor<'_> {
    fn drop(&mut self) {
        while self.frames.pop().is_some() {
            self.runtime.leave_call();
        }
    }
}
