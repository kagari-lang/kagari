use kagari_ir::bytecode::{BytecodeInstruction, CallTarget, Register, RuntimeHelper};
use kagari_runtime::value::Value;

use crate::error::VmError;
use crate::executor::Executor;
use crate::frame::Frame;

impl<'a> Executor<'a> {
    pub(crate) fn dispatch_instruction(
        &mut self,
        instruction: BytecodeInstruction,
    ) -> Result<(), VmError> {
        match instruction {
            BytecodeInstruction::LoadConst { dst, constant } => {
                let value = Self::constant_to_value(constant);
                self.current_frame_mut()?.write_register(dst, value)?;
            }
            BytecodeInstruction::LoadLocal { dst, local } => {
                let value = self.current_frame()?.read_local(local)?;
                self.current_frame_mut()?.write_register(dst, value)?;
            }
            BytecodeInstruction::LoadModule { dst, slot } => {
                let value = self
                    .module_instance
                    .module_slots
                    .get(slot.index())
                    .cloned()
                    .ok_or(VmError::InvalidModuleSlot(slot))?;
                self.current_frame_mut()?.write_register(dst, value)?;
            }
            BytecodeInstruction::StoreLocal { local, src } => {
                let value = self.current_frame()?.read_register(src)?;
                self.current_frame_mut()?.write_local(local, value)?;
            }
            BytecodeInstruction::StoreModule { slot, src } => {
                let value = self.current_frame()?.read_register(src)?;
                let mutable = self
                    .module
                    .module_slots
                    .get(slot.index())
                    .map(|item| item.mutable)
                    .ok_or(VmError::InvalidModuleSlot(slot))?;
                if !mutable && !self.module_instance.is_initializing() {
                    return Err(VmError::ImmutableModuleSlot(slot));
                }
                let target = self
                    .module_instance
                    .module_slots
                    .get_mut(slot.index())
                    .ok_or(VmError::InvalidModuleSlot(slot))?;
                *target = value;
            }
            BytecodeInstruction::Move { dst, src } => {
                let value = self.current_frame()?.read_register(src)?;
                self.current_frame_mut()?.write_register(dst, value)?;
            }
            BytecodeInstruction::Unary { dst, op, operand } => {
                let value = self.current_frame()?.read_register(operand)?;
                let result = Self::apply_unary(op, value)?;
                self.current_frame_mut()?.write_register(dst, result)?;
            }
            BytecodeInstruction::Binary { dst, op, lhs, rhs } => {
                let lhs = self.current_frame()?.read_register(lhs)?;
                let rhs = self.current_frame()?.read_register(rhs)?;
                let result = Self::apply_binary(op, lhs, rhs)?;
                self.current_frame_mut()?.write_register(dst, result)?;
            }
            BytecodeInstruction::Jump { target } => {
                self.current_frame_mut()?.jump_to(target.index())?;
            }
            BytecodeInstruction::Branch {
                cond,
                then_target,
                else_target,
            } => {
                let cond = self.current_frame()?.read_register(cond)?;
                let target = match cond {
                    Value::Bool(true) => then_target,
                    Value::Bool(false) => else_target,
                    _ => return Err(VmError::InvalidBranchCondition),
                };
                self.current_frame_mut()?.jump_to(target.index())?;
            }
            BytecodeInstruction::Call { dst, callee, args } => {
                self.dispatch_call(dst, callee, args)?;
            }
            BytecodeInstruction::Unreachable => {
                return Err(VmError::UnsupportedInstruction("unreachable"));
            }
            BytecodeInstruction::MakeTuple { dst, elements } => {
                let value = self.make_tuple(&elements)?;
                self.current_frame_mut()?.write_register(dst, value)?;
            }
            BytecodeInstruction::MakeArray { dst, elements } => {
                let value = self.make_array(&elements)?;
                self.current_frame_mut()?.write_register(dst, value)?;
            }
            BytecodeInstruction::MakeStruct { dst, name, fields } => {
                let value = self.make_struct(name, &fields)?;
                self.current_frame_mut()?.write_register(dst, value)?;
            }
            BytecodeInstruction::ReadField { dst, base, name } => {
                let value = self.read_field(base, &name)?;
                self.current_frame_mut()?.write_register(dst, value)?;
            }
            BytecodeInstruction::ReadIndex { dst, base, index } => {
                let value = self.read_index(base, index)?;
                self.current_frame_mut()?.write_register(dst, value)?;
            }
            BytecodeInstruction::Return(_) => unreachable!("return handled in run loop"),
        }

        Ok(())
    }

    fn dispatch_call(
        &mut self,
        dst: Option<Register>,
        callee: CallTarget,
        args: Vec<Register>,
    ) -> Result<(), VmError> {
        let arg_values = args
            .iter()
            .map(|arg| self.current_frame()?.read_register(*arg))
            .collect::<Result<Vec<_>, _>>()?;

        match callee {
            CallTarget::Function(id) => {
                let function = self
                    .module
                    .functions
                    .get(id.index())
                    .ok_or(VmError::InvalidFunctionRef(id))?;
                self.frames.push(Frame::new(function, &arg_values, dst));
                Ok(())
            }
            CallTarget::Register(_) => Err(VmError::UnsupportedCallTarget(callee)),
            CallTarget::RuntimeHelper(helper) => {
                self.dispatch_runtime_helper(helper, dst, arg_values)
            }
        }
    }

    fn dispatch_runtime_helper(
        &mut self,
        helper: RuntimeHelper,
        dst: Option<Register>,
        args: Vec<Value>,
    ) -> Result<(), VmError> {
        match helper {
            RuntimeHelper::HostFunction(symbol) => {
                let value = self
                    .runtime
                    .invoke_host(&symbol, &args)
                    .map_err(VmError::HostError)?;
                if let Some(dst) = dst {
                    self.current_frame_mut()?.write_register(dst, value)?;
                }
                Ok(())
            }
            RuntimeHelper::ReflectTypeOf => {
                let Some(value) = args.first() else {
                    return Err(VmError::TypeMismatch(
                        "reflect_type_of expects one argument",
                    ));
                };
                let reflected = self.runtime.reflect_type_of(value);
                if let Some(dst) = dst {
                    self.current_frame_mut()?.write_register(dst, reflected)?;
                }
                Ok(())
            }
            RuntimeHelper::ReflectGetField(field_name) => {
                let Some(base) = args.first() else {
                    return Err(VmError::TypeMismatch(
                        "reflect_get_field expects struct argument",
                    ));
                };
                let reflected = self
                    .runtime
                    .reflect_get_field(base, &field_name)
                    .map_err(VmError::ReflectionError)?;
                if let Some(dst) = dst {
                    self.current_frame_mut()?.write_register(dst, reflected)?;
                }
                Ok(())
            }
            RuntimeHelper::ReflectSetField(field_name) => {
                let [base, next_value] = args.as_slice() else {
                    return Err(VmError::TypeMismatch(
                        "reflect_set_field expects struct and value arguments",
                    ));
                };
                let reflected = self
                    .runtime
                    .reflect_set_field(base, &field_name, next_value.clone())
                    .map_err(VmError::ReflectionError)?;
                if let Some(dst) = dst {
                    self.current_frame_mut()?.write_register(dst, reflected)?;
                }
                Ok(())
            }
            RuntimeHelper::ReflectSetIndex => {
                let [base, index, next_value] = args.as_slice() else {
                    return Err(VmError::TypeMismatch(
                        "reflect_set_index expects value, index and next value arguments",
                    ));
                };
                let reflected = self
                    .runtime
                    .reflect_set_index(base, index, next_value.clone())
                    .map_err(VmError::ReflectionError)?;
                if let Some(dst) = dst {
                    self.current_frame_mut()?.write_register(dst, reflected)?;
                }
                Ok(())
            }
            RuntimeHelper::DynamicCall => Err(VmError::UnsupportedInstruction(
                "runtime_helper_dynamic_call",
            )),
        }
    }
}
