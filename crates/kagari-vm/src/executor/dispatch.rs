use kagari_bytecode::instruction::{
    BytecodeInstruction, CallTarget, PathId, Register, RuntimeHelper,
};
use kagari_common::identity::table::DefinitionId;
use kagari_contract::{operations::IterOp, standard::RuntimePrimitive, types::Ty};
use kagari_runtime::{host::HostPathDescriptorId, numeric, range::RangeValue, value::Value};
use std::iter;

use crate::{
    error::VmError,
    executor::{Executor, native::invoke_script},
};

impl<'a> Executor<'a> {
    fn dispatch_iterator(
        &mut self,
        source: &Value,
        ty: &Ty<DefinitionId>,
        op: IterOp,
        dst: Option<Register>,
    ) -> Result<(), VmError> {
        let result = self
            .runtime
            .iter_operation(self.current_frame()?.loaded(), source, ty, op)?;
        if let Some(dst) = dst {
            self.current_frame_mut()?.write_register(dst, result)?;
        }
        Ok(())
    }

    fn dispatch_range_bound(
        &mut self,
        value: Value,
        range: &Ty<DefinitionId>,
        bound: &Ty<DefinitionId>,
        upper: bool,
        dst: Option<Register>,
    ) -> Result<(), VmError> {
        let Value::Range(value) = value else {
            return Err(VmError::Trap("invalid range value"));
        };
        let result = value.bound(
            self.runtime.gc(),
            self.current_frame()?.loaded().definitions(),
            range,
            bound,
            upper,
        )?;
        if let Some(dst) = dst {
            self.current_frame_mut()?.write_register(dst, result)?;
        }
        Ok(())
    }

    pub(crate) fn dispatch_instruction(
        &mut self,
        instruction: BytecodeInstruction<DefinitionId>,
    ) -> Result<(), VmError> {
        match instruction {
            BytecodeInstruction::Convert {
                dst,
                src,
                conversion,
            } => {
                let value = self.current_frame()?.read_register(src)?;
                let value = numeric::convert(self.runtime.gc(), conversion, value)?;
                self.current_frame_mut()?.write_register(dst, value)?;
            }
            BytecodeInstruction::Numeric {
                dst,
                operation,
                lhs,
                rhs,
            } => {
                let lhs = self.current_frame()?.read_register(lhs)?;
                let rhs = rhs
                    .map(|r| {
                        self.current_frame()
                            .and_then(|frame| frame.read_register(r).map_err(Into::into))
                    })
                    .transpose()?;
                let value = numeric::fixed_integer(operation, lhs, rhs)?;
                self.current_frame_mut()?.write_register(dst, value)?;
            }
            BytecodeInstruction::MapResultError {
                dst,
                original,
                error,
                ty,
            } => {
                let frame = self.current_frame()?;
                let mapped = self.runtime.map_result_error(
                    frame.loaded(),
                    &frame.read_register(original)?,
                    frame.read_register(error)?,
                    &ty,
                )?;
                drop(frame);
                self.current_frame_mut()?.write_register(dst, mapped)?;
            }

            BytecodeInstruction::Iter { dst, value, ty, op } => {
                let source = self
                    .current_frame()?
                    .read_register(value.ok_or(VmError::TypeMismatch("iterator source"))?)?;
                self.dispatch_iterator(&source, &ty, op, Some(dst))?;
            }
            BytecodeInstruction::StandardEnum { dst, value, ty, op } => {
                let ty = self.current_frame()?.resolve_type(&ty)?;
                let result = self.standard_enum_operation(value, &ty, op)?;
                self.current_frame_mut()?.write_register(dst, result)?;
            }

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
                arguments,
                variant,
            } => {
                let result = self.test_enum_variant(value, enumeration, &arguments, variant)?;
                self.current_frame_mut()?.write_register(dst, result)?;
            }
            BytecodeInstruction::ReadEnumPayload {
                dst,
                value,
                enumeration,
                arguments,
                variant,
                index,
            } => {
                let result =
                    self.read_enum_payload(value, enumeration, &arguments, variant, index)?;
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
                if !mutable {
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
            BytecodeInstruction::RangeBound {
                dst,
                value,
                range,
                bound,
                upper,
            } => {
                let value = self.current_frame()?.read_register(value)?;
                self.dispatch_range_bound(value, &range, &bound, upper, Some(dst))?;
            }
            BytecodeInstruction::MakeRange {
                dst,
                start,
                end,
                ty,
            } => {
                let frame = self.current_frame()?;
                let start = start.map(|r| frame.read_register(r)).transpose()?;
                let end = end.map(|r| frame.read_register(r)).transpose()?;
                drop(frame);
                let value = RangeValue::new(&ty, start.as_ref(), end.as_ref())
                    .map_err(VmError::RuntimeError)?;
                self.current_frame_mut()?
                    .write_register(dst, Value::Range(value))?;
            }
            BytecodeInstruction::RepeatArray {
                dst,
                value,
                count,
                element,
            } => {
                let value = self.current_frame()?.read_register(value)?;
                let Value::U64(count) = self.current_frame()?.read_register(count)? else {
                    return Err(VmError::Trap("invalid repeat array count"));
                };
                let count = usize::try_from(count)
                    .map_err(|_| VmError::Trap("array length exceeds platform capacity"))?;
                let array = self
                    .current_frame()?
                    .alloc_array_repeat(self.runtime, &element, value, count)
                    .map_err(VmError::RuntimeError)?;
                self.current_frame_mut()?
                    .write_register(dst, Value::Array(array))?;
            }
            BytecodeInstruction::MakeArray {
                dst,
                elements,
                element,
            } => {
                let value = self.make_array(&element, &elements)?;
                self.current_frame_mut()?.write_register(dst, value)?;
            }
            BytecodeInstruction::MakeClosure {
                dst,
                function,
                captures,
            } => {
                let values = self.read_path_args(&captures)?;
                let loaded = self.current_loaded()?;
                let environment = self.current_frame()?.environment();
                let closure = self
                    .runtime
                    .make_closure(&loaded, function, values, environment)
                    .map_err(VmError::RuntimeError)?;
                self.current_frame_mut()?.write_register(dst, closure)?;
            }
            BytecodeInstruction::MakeCell { dst, value } => {
                let ty = self.current_frame()?.register_type(value)?;
                let value = self.current_frame()?.read_register(value)?;
                let cell = self
                    .runtime
                    .make_capture_cell(ty, value)
                    .map_err(VmError::RuntimeError)?;
                self.current_frame_mut()?.write_register(dst, cell)?;
            }
            BytecodeInstruction::ReadCell { dst, cell } => {
                let ty = self.current_frame()?.register_type(dst)?;
                let cell = self.current_frame()?.read_register(cell)?;
                let value = self
                    .runtime
                    .read_capture_cell(&cell, ty)
                    .map_err(VmError::RuntimeError)?;
                self.current_frame_mut()?.write_register(dst, value)?;
            }
            BytecodeInstruction::WriteCell { cell, value } => {
                let ty = self.current_frame()?.register_type(value)?;
                let cell = self.current_frame()?.read_register(cell)?;
                let value = self.current_frame()?.read_register(value)?;
                self.runtime
                    .write_capture_cell(&cell, ty, value)
                    .map_err(VmError::RuntimeError)?;
            }
            BytecodeInstruction::UpcastInterface {
                dst,
                value,
                source,
                target,
            } => {
                let Ty::Trait(source) = self
                    .current_frame()?
                    .resolve_type(&Ty::Trait(source))?
                    .into_owned()
                else {
                    return Err(VmError::TypeMismatch("interface upcast source"));
                };
                let Ty::Trait(target) = self
                    .current_frame()?
                    .resolve_type(&Ty::Trait(target))?
                    .into_owned()
                else {
                    return Err(VmError::TypeMismatch("interface upcast target"));
                };
                let value = self.current_frame()?.read_register(value)?;
                let view = self
                    .runtime
                    .upcast_interface(&value, &source, &target)
                    .map_err(VmError::RuntimeError)?;
                self.current_frame_mut()?.write_register(dst, view)?;
            }
            BytecodeInstruction::MakeInterface {
                dst,
                value,
                module,
                implementation,
                arguments,
            } => {
                let receiver = self.current_frame()?.read_register(value)?;
                let arguments = self
                    .current_frame()?
                    .type_arguments(self.runtime, &arguments)?;
                let loaded = self.current_loaded()?.member(module).ok_or(
                    VmError::UnsupportedInstruction("invalid interface module slot"),
                )?;
                let interface = self
                    .runtime
                    .make_interface_applied(&loaded, implementation.index(), &arguments, receiver)
                    .map_err(VmError::RuntimeError)?;
                self.current_frame_mut()?.write_register(dst, interface)?;
            }
            BytecodeInstruction::MakeEnum {
                dst,
                enumeration,
                arguments,
                variant,
                fields,
            } => {
                let value = self.make_enum(enumeration, &arguments, variant, &fields)?;
                self.current_frame_mut()?.write_register(dst, value)?;
            }
            BytecodeInstruction::MakeStruct {
                dst,
                structure,
                arguments,
                fields,
            } => {
                let value = self.make_struct(structure, &arguments, &fields)?;
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
        callee: CallTarget<DefinitionId>,
        args: Vec<Register>,
    ) -> Result<(), VmError> {
        if let CallTarget::Native(import) = callee {
            return self
                .stack
                .invoke_native(self.runtime, import, &args, dst, invoke_script)
                .map_err(VmError::RuntimeError);
        }
        let arg_values = args
            .iter()
            .map(|arg| Ok::<_, VmError>(self.current_frame()?.read_register(*arg)?))
            .collect::<Result<Vec<_>, _>>()?;

        match callee {
            CallTarget::Shared { .. } => self
                .stack
                .push_shared_call(self.runtime, &arg_values, dst)
                .map_err(VmError::RuntimeError),
            CallTarget::Native(_) => unreachable!("native calls execute before argument packing"),
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
            CallTarget::InterfaceMethod { contract, .. } => {
                let receiver = arg_values
                    .first()
                    .ok_or(VmError::TypeMismatch("interface method receiver"))?;
                let resolved = self
                    .runtime
                    .resolve_interface_call(&*self.current_frame()?, &contract, receiver)
                    .map_err(VmError::RuntimeError)?;
                let arguments = iter::once(resolved.receiver().clone())
                    .chain(arg_values.into_iter().skip(1))
                    .collect::<Vec<_>>();
                self.stack
                    .push_interface_method(self.runtime, resolved, &arguments, dst)
                    .map_err(VmError::RuntimeError)
            }
            CallTarget::Register(_) => Err(VmError::UnsupportedCallTarget(Box::new(callee))),
            CallTarget::ClosureRegister {
                register,
                params,
                return_type,
            } => {
                let (params, return_type) = self
                    .current_frame()?
                    .closure_signature(register, &params, return_type)
                    .map_err(VmError::RuntimeError)?;
                let value = self.current_frame()?.read_register(register)?;
                let closure = self
                    .runtime
                    .resolve_closure(&value)
                    .map_err(VmError::RuntimeError)?;
                let (actual_params, actual_result) = closure
                    .physical_signature()
                    .map_err(VmError::RuntimeError)?;
                if actual_result != return_type || actual_params != params {
                    return Err(VmError::TypeMismatch("closure call contract"));
                }
                self.stack
                    .push_closure(self.runtime, &closure, &arg_values, dst)
                    .map_err(VmError::RuntimeError)
            }
            CallTarget::RuntimePrimitive(intrinsic) => {
                self.dispatch_standard_intrinsic(intrinsic, dst, arg_values)
            }
            CallTarget::RuntimeHelper(helper) => {
                self.dispatch_runtime_helper(helper, dst, arg_values)
            }
        }
    }

    fn dispatch_standard_intrinsic(
        &mut self,
        intrinsic: RuntimePrimitive,
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
                    .resources()
                    .ensure_execution_allowed()
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
