mod aggregate_ops;
mod dispatch;
mod value_ops;

use kagari_ir::bytecode::{BytecodeInstruction, BytecodeModule, FunctionRef};
use kagari_runtime::{Runtime, value::Value};

use crate::error::VmError;
use crate::frame::Frame;
use crate::vm::ModuleInstance;

#[derive(Debug)]
pub(crate) struct Executor<'a> {
    runtime: &'a mut Runtime,
    module: &'a BytecodeModule,
    module_instance: &'a mut ModuleInstance,
    frames: Vec<Frame<'a>>,
}

impl<'a> Executor<'a> {
    pub(crate) fn new(
        runtime: &'a mut Runtime,
        module: &'a BytecodeModule,
        module_instance: &'a mut ModuleInstance,
        entry: FunctionRef,
    ) -> Result<Self, VmError> {
        let function = module
            .functions
            .get(entry.index())
            .ok_or(VmError::InvalidFunctionRef(entry))?;

        Ok(Self {
            runtime,
            module,
            module_instance,
            frames: vec![Frame::new(function, &[], None)],
        })
    }

    pub(crate) fn run(&mut self) -> Result<Value, VmError> {
        loop {
            let instruction = {
                let frame = self
                    .frames
                    .last_mut()
                    .expect("executor should have a frame");
                frame.next_instruction().cloned()
            };

            let Some(instruction) = instruction else {
                return Ok(Value::Unit);
            };

            match instruction {
                BytecodeInstruction::Return(value) => {
                    let value = match value {
                        Some(register) => self.current_frame()?.read_register(register)?,
                        None => Value::Unit,
                    };
                    let return_dst = self.current_frame()?.return_dst();
                    self.frames.pop();
                    if let Some(frame) = self.frames.last_mut() {
                        if let Some(dst) = return_dst {
                            frame.write_register(dst, value)?;
                        }
                    } else {
                        return Ok(value);
                    }
                }
                instruction => self.dispatch_instruction(instruction)?,
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
}
