use kagari_ir::bytecode::{BytecodeInstruction, CallTarget, PathId, Register, RuntimeHelper};
use kagari_runtime::{HostPathDescriptorId, value::Value};

use crate::error::VmError;
use crate::executor::Executor;

impl<'a> Executor<'a> {
    pub(crate) fn dispatch_instruction(
        &mut self,
        instruction: BytecodeInstruction,
    ) -> Result<(), VmError> {
        match instruction {
            BytecodeInstruction::BeginIteration { collection } => {
                self.current_frame_mut()?.begin_iteration(collection)?;
            }
            BytecodeInstruction::EndIteration => {
                self.current_frame_mut()?.end_iteration()?;
            }
            BytecodeInstruction::TestEnumVariant {
                dst,
                value,
                enumeration,
                variant,
            } => {
                let result = self.test_enum_variant(value, enumeration, variant)?;
                self.current_frame_mut()?.write_register(dst, result)?;
            }
            BytecodeInstruction::ReadEnumPayload {
                dst,
                value,
                enumeration,
                variant,
                index,
            } => {
                let result = self.read_enum_payload(value, enumeration, variant, index)?;
                self.current_frame_mut()?.write_register(dst, result)?;
            }
            BytecodeInstruction::LoadConst { dst, constant } => {
                let value = Self::constant_to_value(constant);
                self.current_frame_mut()?.write_register(dst, value)?;
            }
            BytecodeInstruction::LoadLocal { dst, local } => {
                let value = self.current_frame()?.read_local(local)?;
                self.current_frame_mut()?.write_register(dst, value)?;
            }
            BytecodeInstruction::LoadModule { dst, slot } => {
                let loaded = self.current_loaded()?;
                let value = self
                    .runtime
                    .module_instance_mut(&loaded)
                    .map_err(VmError::RuntimeError)?
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
                let loaded = self.current_loaded()?;
                let mutable = loaded
                    .bytecode
                    .module_slots
                    .get(slot.index())
                    .map(|item| item.mutable)
                    .ok_or(VmError::InvalidModuleSlot(slot))?;
                let mut instance = self
                    .runtime
                    .module_instance_mut(&loaded)
                    .map_err(VmError::RuntimeError)?;
                if !mutable && !instance.is_initializing() {
                    return Err(VmError::ImmutableModuleSlot(slot));
                }
                *instance
                    .module_slots
                    .get_mut(slot.index())
                    .ok_or(VmError::InvalidModuleSlot(slot))? = value;
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
                let result = self.apply_binary(op, lhs, rhs)?;
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
                return Err(VmError::Trap("unreachable"));
            }
            BytecodeInstruction::MakeTuple { dst, elements } => {
                let value = self.make_tuple(&elements)?;
                self.current_frame_mut()?.write_register(dst, value)?;
            }
            BytecodeInstruction::MakeArray { dst, elements } => {
                let value = self.make_array(&elements)?;
                self.current_frame_mut()?.write_register(dst, value)?;
            }
            BytecodeInstruction::MakeClosure {
                dst,
                function,
                captures,
            } => {
                let values = self.read_path_args(&captures)?;
                let loaded = self.current_loaded()?;
                let closure = self
                    .runtime
                    .make_closure(&loaded, function, values)
                    .map_err(VmError::RuntimeError)?;
                self.current_frame_mut()?.write_register(dst, closure)?;
            }
            BytecodeInstruction::MakeCell { dst, value } => {
                let ty = self.current_frame()?.function().metadata.registers[value.index()];
                let value = self.current_frame()?.read_register(value)?;
                let cell = self
                    .runtime
                    .make_capture_cell(ty, value)
                    .map_err(VmError::RuntimeError)?;
                self.current_frame_mut()?.write_register(dst, cell)?;
            }
            BytecodeInstruction::ReadCell { dst, cell } => {
                let ty = self.current_frame()?.function().metadata.registers[dst.index()];
                let cell = self.current_frame()?.read_register(cell)?;
                let value = self
                    .runtime
                    .read_capture_cell(&cell, ty)
                    .map_err(VmError::RuntimeError)?;
                self.current_frame_mut()?.write_register(dst, value)?;
            }
            BytecodeInstruction::WriteCell { cell, value } => {
                let ty = self.current_frame()?.function().metadata.registers[value.index()];
                let cell = self.current_frame()?.read_register(cell)?;
                let value = self.current_frame()?.read_register(value)?;
                self.runtime
                    .write_capture_cell(&cell, ty, value)
                    .map_err(VmError::RuntimeError)?;
            }
            BytecodeInstruction::MakeInterface {
                dst,
                value,
                module,
                implementation,
            } => {
                let receiver = self.current_frame()?.read_register(value)?;
                let loaded = self.current_loaded()?.member(module).ok_or(
                    VmError::UnsupportedInstruction("invalid interface module slot"),
                )?;
                let interface = self
                    .runtime
                    .make_interface(&loaded, implementation.index(), receiver)
                    .map_err(VmError::RuntimeError)?;
                self.current_frame_mut()?.write_register(dst, interface)?;
            }
            BytecodeInstruction::MakeEnum {
                dst,
                enumeration,
                variant,
                fields,
            } => {
                let value = self.make_enum(enumeration, variant, &fields)?;
                self.current_frame_mut()?.write_register(dst, value)?;
            }
            BytecodeInstruction::MakeStruct {
                dst,
                structure,
                fields,
            } => {
                let value = self.make_struct(structure, &fields)?;
                self.current_frame_mut()?.write_register(dst, value)?;
            }
            BytecodeInstruction::ReadAggregateField { dst, base, field } => {
                let value = self.read_field(base, field)?;
                self.current_frame_mut()?.write_register(dst, value)?;
            }
            BytecodeInstruction::WriteAggregateField { base, field, value } => {
                self.write_field(base, field, value)?;
            }
            BytecodeInstruction::ReadAggregateIndex { dst, base, index } => {
                let value = self.read_index(base, index)?;
                self.current_frame_mut()?.write_register(dst, value)?;
            }
            BytecodeInstruction::WriteAggregateIndex { base, index, value } => {
                self.write_index(base, index, value)?;
            }
            BytecodeInstruction::ReadPath {
                dst,
                root_or_view,
                path,
                dynamic_args,
            } => {
                let root_or_view = self.current_frame()?.read_register(root_or_view)?;
                let dynamic_args = self.read_path_args(&dynamic_args)?;
                let value = self
                    .runtime
                    .read_host_path(&root_or_view, self.descriptor_id(path)?, dynamic_args)
                    .map_err(VmError::RuntimeError)?;
                self.current_frame_mut()?.write_register(dst, value)?;
            }
            BytecodeInstruction::SetPath {
                root_or_view,
                path,
                dynamic_args,
                value,
            } => {
                let root_or_view = self.current_frame()?.read_register(root_or_view)?;
                let dynamic_args = self.read_path_args(&dynamic_args)?;
                let value = self.current_frame()?.read_register(value)?;
                self.runtime
                    .set_host_path(
                        &root_or_view,
                        self.descriptor_id(path)?,
                        dynamic_args,
                        value,
                    )
                    .map_err(VmError::RuntimeError)?;
            }
            BytecodeInstruction::ModifyPath {
                dst,
                root_or_view,
                path,
                dynamic_args,
                op,
                value,
            } => {
                let root_or_view = self.current_frame()?.read_register(root_or_view)?;
                let dynamic_args = self.read_path_args(&dynamic_args)?;
                let value = self.current_frame()?.read_register(value)?;
                let value = self
                    .runtime
                    .modify_host_path(
                        &root_or_view,
                        self.descriptor_id(path)?,
                        dynamic_args,
                        op,
                        value,
                    )
                    .map_err(VmError::RuntimeError)?;
                if let Some(dst) = dst {
                    self.current_frame_mut()?.write_register(dst, value)?;
                }
            }
            BytecodeInstruction::MakePathView {
                dst,
                root_or_view,
                path,
                dynamic_args,
            } => {
                let root_or_view = self.current_frame()?.read_register(root_or_view)?;
                let dynamic_args = self.read_path_args(&dynamic_args)?;
                let view = self
                    .runtime
                    .make_host_path_view_from_value(
                        &root_or_view,
                        self.descriptor_id(path)?,
                        dynamic_args,
                    )
                    .map_err(VmError::RuntimeError)?;
                self.current_frame_mut()?
                    .write_register(dst, Value::HostPathView(view))?;
            }
            BytecodeInstruction::Return(_) => unreachable!("return handled in run loop"),
        }

        Ok(())
    }

    fn read_path_args(&self, args: &[Register]) -> Result<Vec<Value>, VmError> {
        args.iter()
            .map(|arg| Ok::<_, VmError>(self.current_frame()?.read_register(*arg)?))
            .collect()
    }

    fn dispatch_call(
        &mut self,
        dst: Option<Register>,
        callee: CallTarget,
        args: Vec<Register>,
    ) -> Result<(), VmError> {
        let arg_values = args
            .iter()
            .map(|arg| Ok::<_, VmError>(self.current_frame()?.read_register(*arg)?))
            .collect::<Result<Vec<_>, _>>()?;

        match callee {
            CallTarget::ModuleFunction { module, function } => {
                self.current_loaded()?
                    .member_data(module)
                    .and_then(|member| member.bytecode.functions.get(function.index()))
                    .ok_or(VmError::InvalidFunctionRef(function))?;
                self.push_frame(module, function, &arg_values, dst)
            }
            CallTarget::Function(id) => {
                self.current_loaded()?
                    .bytecode
                    .functions
                    .get(id.index())
                    .ok_or(VmError::InvalidFunctionRef(id))?;
                let module = self.current_frame()?.module();
                self.push_frame(module, id, &arg_values, dst)
            }
            CallTarget::InterfaceMethod {
                interface,
                method_slot,
                ..
            } => {
                let boxed = arg_values
                    .first()
                    .ok_or(VmError::TypeMismatch("interface method receiver"))?;
                let resolved = self
                    .runtime
                    .resolve_interface_method_slot(boxed, &interface, method_slot as usize)
                    .map_err(VmError::RuntimeError)?;
                let arguments = std::iter::once(resolved.receiver().clone())
                    .chain(arg_values.into_iter().skip(1))
                    .collect::<Vec<_>>();
                self.stack
                    .push_interface_method(self.runtime, resolved, &arguments, dst)
                    .map_err(VmError::RuntimeError)
            }
            CallTarget::HostFunction(import) => {
                let binding = self
                    .current_loaded()?
                    .host_binding(import)
                    .ok_or(VmError::UnsupportedInstruction("unlinked host import"))?;
                let value = self
                    .runtime
                    .invoke_bound_host(binding, &arg_values)
                    .map_err(VmError::RuntimeError)?;
                if let Some(dst) = dst {
                    self.current_frame_mut()?.write_register(dst, value)?;
                }
                Ok(())
            }
            CallTarget::Register(_) => Err(VmError::UnsupportedCallTarget(callee)),
            CallTarget::ClosureRegister {
                register,
                params,
                return_type,
            } => {
                let value = self.current_frame()?.read_register(register)?;
                let closure = self
                    .runtime
                    .resolve_closure(&value)
                    .map_err(VmError::RuntimeError)?;
                let metadata = closure
                    .implementation
                    .bytecode
                    .functions
                    .get(closure.function.index())
                    .ok_or(VmError::InvalidFunctionRef(closure.function))?;
                if metadata.metadata.return_type != return_type
                    || metadata.metadata.params.get(closure.captures.len()..)
                        != Some(params.as_slice())
                {
                    return Err(VmError::TypeMismatch("closure call contract"));
                }
                self.stack
                    .push_closure(self.runtime, closure, &arg_values, dst)
                    .map_err(VmError::RuntimeError)
            }
            CallTarget::StandardIntrinsic(intrinsic) => {
                self.dispatch_standard_intrinsic(intrinsic, dst, arg_values)
            }
            CallTarget::RuntimeHelper(helper) => {
                self.dispatch_runtime_helper(helper, dst, arg_values)
            }
        }
    }

    fn dispatch_standard_intrinsic(
        &mut self,
        intrinsic: kagari_ir::bytecode::StandardIntrinsic,
        dst: Option<Register>,
        args: Vec<Value>,
    ) -> Result<(), VmError> {
        let value = self
            .runtime
            .invoke_standard_builtin(intrinsic, &args)
            .map_err(VmError::from)?;
        if let Some(dst) = dst {
            self.current_frame_mut()?.write_register(dst, value)?;
        }
        Ok(())
    }

    fn dispatch_runtime_helper(
        &mut self,
        helper: RuntimeHelper,
        dst: Option<Register>,
        args: Vec<Value>,
    ) -> Result<(), VmError> {
        match helper {
            RuntimeHelper::ReflectTypeOf => {
                let Some(value) = args.first() else {
                    return Err(VmError::TypeMismatch(
                        "reflect_type_of expects one argument",
                    ));
                };
                let reflected = self
                    .runtime
                    .reflect_type_of(value)
                    .map_err(VmError::RuntimeError)?;
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
                    .map_err(VmError::RuntimeError)?;
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
                    .map_err(VmError::RuntimeError)?;
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
                    .map_err(VmError::RuntimeError)?;
                if let Some(dst) = dst {
                    self.current_frame_mut()?.write_register(dst, reflected)?;
                }
                Ok(())
            }
            RuntimeHelper::DynamicCall => {
                self.runtime
                    .validate_dynamic_invocation_boundary()
                    .map_err(VmError::RuntimeError)?;
                Err(VmError::UnsupportedInstruction(
                    "runtime_helper_dynamic_call",
                ))
            }
        }
    }
}

impl Executor<'_> {
    fn descriptor_id(&self, path: PathId) -> Result<HostPathDescriptorId, VmError> {
        self.current_loaded()?
            .path_binding(path)
            .ok_or(VmError::UnsupportedInstruction("missing linked path"))
    }
}
