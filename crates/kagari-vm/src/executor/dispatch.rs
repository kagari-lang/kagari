use crate::{
    error::VmError,
    executor::{Executor, native::invoke_script},
};
use kagari_bytecode::instruction::{
    BytecodeInstruction, CallTarget, ConstantOperand, PathId, Register, RuntimeHelper,
};
use kagari_common::identity::table::DefinitionId;
use kagari_contract::{operations::IterOp, standard::RuntimePrimitive};
use kagari_runtime::{host::HostPathDescriptorId, range::RangeValue, value::Value};
use kagari_types::ty::Ty;
use std::{iter, ops::Bound, slice, sync::Arc};

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
            self.current_frame_mut()?
                .write_register(self.runtime, dst, result)?;
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
        let owner = self.current_frame()?.loaded().clone();
        let (member, fields) = match value.bound(owner.definitions(), range, bound, upper)? {
            Bound::Included(value) => ("Included", vec![value]),
            Bound::Excluded(value) => ("Excluded", vec![value]),
            Bound::Unbounded => ("Unbounded", vec![]),
        };
        let applied = self
            .runtime
            .resolve_type_arguments(&owner, slice::from_ref(bound))?
            .pop()
            .ok_or(VmError::TypeMismatch("range bound type scope"))?;
        let result = self
            .runtime
            .make_enum_member(&owner, &applied, member, fields)?;
        if let Some(dst) = dst {
            self.current_frame_mut()?
                .write_register(self.runtime, dst, result)?;
        }
        Ok(())
    }

    pub(crate) fn dispatch_instruction(
        &mut self,
        instruction: &BytecodeInstruction<DefinitionId>,
    ) -> Result<(), VmError> {
        match *instruction {
            BytecodeInstruction::Iter {
                dst,
                value,
                ref ty,
                op,
            } => {
                let source = self.current_frame()?.read_register(
                    self.runtime,
                    value.ok_or(VmError::TypeMismatch("iterator source"))?,
                )?;
                self.dispatch_iterator(&source, ty, op, Some(dst))?;
            }

            BytecodeInstruction::BeginIteration { collection } => {
                self.current_frame_mut()?
                    .begin_iteration(self.runtime, collection)?;
            }
            BytecodeInstruction::EndIteration => {
                self.current_frame_mut()?.end_iteration()?;
            }
            BytecodeInstruction::TestEnumVariant {
                dst,
                value,
                enumeration,
                ref arguments,
                variant,
            } => {
                let result = self.test_enum_variant(value, enumeration, arguments, variant)?;
                self.current_frame_mut()?
                    .write_register(self.runtime, dst, result)?;
            }
            BytecodeInstruction::ReadEnumPayload {
                dst,
                value,
                enumeration,
                ref arguments,
                variant,
                index,
            } => {
                let result =
                    self.read_enum_payload(value, enumeration, arguments, variant, index)?;
                self.current_frame_mut()?
                    .write_register(self.runtime, dst, result)?;
            }
            BytecodeInstruction::LoadConst {
                dst,
                constant: ConstantOperand::Str(ref value),
            } => {
                self.current_frame_mut()?.write_register(
                    self.runtime,
                    dst,
                    Value::Str(value.clone()),
                )?;
            }
            BytecodeInstruction::LoadModule { dst, slot } => {
                let loaded = self.current_loaded()?;
                let value = self.runtime.read_module_slot(&loaded, slot)?;
                self.current_frame_mut()?
                    .write_register(self.runtime, dst, value)?;
            }
            BytecodeInstruction::StoreModule { slot, src } => {
                let value = self.current_frame()?.read_register(self.runtime, src)?;
                let loaded = self.current_loaded()?;
                self.runtime.write_module_slot(&loaded, slot, value)?;
            }
            BytecodeInstruction::Call {
                dst,
                ref callee,
                ref args,
            } => {
                self.dispatch_call(dst, callee, args)?;
            }
            BytecodeInstruction::Unreachable => {
                return Err(VmError::Trap("unreachable"));
            }
            BytecodeInstruction::MakeTuple { dst, ref elements } => {
                let value = self.make_tuple(elements)?;
                self.current_frame_mut()?
                    .write_register(self.runtime, dst, value)?;
            }
            BytecodeInstruction::RangeBound {
                dst,
                value,
                ref range,
                ref bound,
                upper,
            } => {
                let value = self.current_frame()?.read_register(self.runtime, value)?;
                self.dispatch_range_bound(value, range, bound, upper, Some(dst))?;
            }
            BytecodeInstruction::MakeRange {
                dst,
                start,
                end,
                ref ty,
            } => {
                let frame = self.current_frame()?;
                let start = start
                    .map(|r| frame.read_register(self.runtime, r))
                    .transpose()?;
                let end = end
                    .map(|r| frame.read_register(self.runtime, r))
                    .transpose()?;
                drop(frame);
                let value = RangeValue::new(ty, start.as_ref(), end.as_ref())
                    .map_err(VmError::RuntimeError)?;
                self.current_frame_mut()?
                    .write_register(self.runtime, dst, Value::Range(value))?;
            }
            BytecodeInstruction::RepeatArray {
                dst,
                value,
                count,
                ref element,
            } => {
                let value = self.current_frame()?.read_register(self.runtime, value)?;
                let Value::U64(count) = self.current_frame()?.read_register(self.runtime, count)?
                else {
                    return Err(VmError::Trap("invalid repeat array count"));
                };
                let count = usize::try_from(count)
                    .map_err(|_| VmError::Trap("array length exceeds platform capacity"))?;
                let array = self
                    .current_frame()?
                    .alloc_array_repeat(self.runtime, element, value, count)
                    .map_err(VmError::RuntimeError)?;
                self.current_frame_mut()?
                    .write_register(self.runtime, dst, Value::Array(array))?;
            }
            BytecodeInstruction::MakeArray {
                dst,
                ref elements,
                ref element,
            } => {
                let value = self.make_array(element, elements)?;
                self.current_frame_mut()?
                    .write_register(self.runtime, dst, value)?;
            }
            BytecodeInstruction::MakeClosure {
                dst,
                function,
                ref captures,
            } => {
                let values = self.read_path_args(captures)?;
                let loaded = self.current_loaded()?;
                let environment = self.current_frame()?.environment();
                let closure = self
                    .runtime
                    .make_closure(&loaded, function, values, environment)
                    .map_err(VmError::RuntimeError)?;
                self.current_frame_mut()?
                    .write_register(self.runtime, dst, closure)?;
            }
            BytecodeInstruction::MakeCell { dst, value } => {
                let ty = self.current_frame()?.register_type(self.runtime, value)?;
                let value = self.current_frame()?.read_register(self.runtime, value)?;
                let cell = self
                    .runtime
                    .make_capture_cell(ty, value)
                    .map_err(VmError::RuntimeError)?;
                self.current_frame_mut()?
                    .write_register(self.runtime, dst, cell)?;
            }
            BytecodeInstruction::ReadCell { dst, cell } => {
                let ty = self.current_frame()?.register_type(self.runtime, dst)?;
                let cell = self.current_frame()?.read_register(self.runtime, cell)?;
                let value = self
                    .runtime
                    .read_capture_cell(&cell, ty)
                    .map_err(VmError::RuntimeError)?;
                self.current_frame_mut()?
                    .write_register(self.runtime, dst, value)?;
            }
            BytecodeInstruction::WriteCell { cell, value } => {
                let ty = self.current_frame()?.register_type(self.runtime, value)?;
                let cell = self.current_frame()?.read_register(self.runtime, cell)?;
                let value = self.current_frame()?.read_register(self.runtime, value)?;
                self.runtime
                    .write_capture_cell(&cell, ty, value)
                    .map_err(VmError::RuntimeError)?;
            }
            BytecodeInstruction::UpcastInterface {
                dst,
                value,
                ref source,
                ref target,
            } => {
                let Ty::Trait(source) = self
                    .current_frame()?
                    .resolve_type(&Ty::Trait(source.clone()))?
                    .into_owned()
                else {
                    return Err(VmError::TypeMismatch("interface upcast source"));
                };
                let Ty::Trait(target) = self
                    .current_frame()?
                    .resolve_type(&Ty::Trait(target.clone()))?
                    .into_owned()
                else {
                    return Err(VmError::TypeMismatch("interface upcast target"));
                };
                let value = self.current_frame()?.read_register(self.runtime, value)?;
                let view = self
                    .runtime
                    .upcast_interface(&value, &source, &target)
                    .map_err(VmError::RuntimeError)?;
                self.current_frame_mut()?
                    .write_register(self.runtime, dst, view)?;
            }
            BytecodeInstruction::MakeInterface {
                dst,
                value,
                module,
                implementation,
                ref arguments,
            } => {
                let receiver = self.current_frame()?.read_register(self.runtime, value)?;
                let arguments = self
                    .current_frame()?
                    .type_arguments(self.runtime, arguments)?;
                let loaded = self.current_loaded()?.member(module).ok_or(
                    VmError::UnsupportedInstruction("invalid interface module slot"),
                )?;
                let interface = self
                    .runtime
                    .make_interface_applied(&loaded, implementation.index(), &arguments, receiver)
                    .map_err(VmError::RuntimeError)?;
                self.current_frame_mut()?
                    .write_register(self.runtime, dst, interface)?;
            }
            BytecodeInstruction::MakeEnum {
                dst,
                enumeration,
                ref arguments,
                variant,
                ref fields,
            } => {
                let value = self.make_enum(enumeration, arguments, variant, fields)?;
                self.current_frame_mut()?
                    .write_register(self.runtime, dst, value)?;
            }
            BytecodeInstruction::MakeStruct {
                dst,
                structure,
                ref arguments,
                ref fields,
            } => {
                let value = self.make_struct(structure, arguments, fields)?;
                self.current_frame_mut()?
                    .write_register(self.runtime, dst, value)?;
            }
            BytecodeInstruction::ReadAggregateField {
                dst,
                base,
                ref field,
            } => {
                let value = self.read_field(base, field)?;
                self.current_frame_mut()?
                    .write_register(self.runtime, dst, value)?;
            }
            BytecodeInstruction::WriteAggregateField {
                base,
                ref field,
                value,
            } => {
                self.write_field(base, field, value)?;
            }
            BytecodeInstruction::ReadAggregateIndex { dst, base, index } => {
                let value = self.read_index(base, index)?;
                self.current_frame_mut()?
                    .write_register(self.runtime, dst, value)?;
            }
            BytecodeInstruction::WriteAggregateIndex { base, index, value } => {
                self.write_index(base, index, value)?;
            }
            BytecodeInstruction::ReadPath {
                dst,
                root_or_view,
                path,
                ref dynamic_args,
            } => {
                let root_or_view = self
                    .current_frame()?
                    .read_register(self.runtime, root_or_view)?;
                let dynamic_args = self.read_path_args(dynamic_args)?;
                let value = self
                    .runtime
                    .read_host_path(&root_or_view, self.descriptor_id(path)?, dynamic_args)
                    .map_err(VmError::RuntimeError)?;
                self.current_frame_mut()?
                    .write_register(self.runtime, dst, value)?;
            }
            BytecodeInstruction::SetPath {
                root_or_view,
                path,
                ref dynamic_args,
                value,
            } => {
                let root_or_view = self
                    .current_frame()?
                    .read_register(self.runtime, root_or_view)?;
                let dynamic_args = self.read_path_args(dynamic_args)?;
                let value = self.current_frame()?.read_register(self.runtime, value)?;
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
                ref dynamic_args,
                op,
                value,
            } => {
                let root_or_view = self
                    .current_frame()?
                    .read_register(self.runtime, root_or_view)?;
                let dynamic_args = self.read_path_args(dynamic_args)?;
                let value = self.current_frame()?.read_register(self.runtime, value)?;
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
                    self.current_frame_mut()?
                        .write_register(self.runtime, dst, value)?;
                }
            }
            BytecodeInstruction::MakePathView {
                dst,
                root_or_view,
                path,
                ref dynamic_args,
            } => {
                let root_or_view = self
                    .current_frame()?
                    .read_register(self.runtime, root_or_view)?;
                let dynamic_args = self.read_path_args(dynamic_args)?;
                let view = self
                    .runtime
                    .make_host_path_view_from_value(
                        &root_or_view,
                        self.descriptor_id(path)?,
                        dynamic_args,
                    )
                    .map_err(VmError::RuntimeError)?;
                self.current_frame_mut()?.write_register(
                    self.runtime,
                    dst,
                    Value::HostPathView(Arc::new(view)),
                )?;
            }
            BytecodeInstruction::Return(_)
            | BytecodeInstruction::LoadConst { .. }
            | BytecodeInstruction::LoadLocal { .. }
            | BytecodeInstruction::StoreLocal { .. }
            | BytecodeInstruction::Move { .. }
            | BytecodeInstruction::Unary { .. }
            | BytecodeInstruction::Binary { .. }
            | BytecodeInstruction::Jump { .. }
            | BytecodeInstruction::Branch { .. }
            | BytecodeInstruction::Convert { .. }
            | BytecodeInstruction::Numeric { .. } => {
                return Err(VmError::UnsupportedInstruction(
                    "cursor operation at slow boundary",
                ));
            }
        }

        Ok(())
    }

    fn read_path_args(&self, args: &[Register]) -> Result<Vec<Value>, VmError> {
        args.iter()
            .map(|arg| Ok::<_, VmError>(self.current_frame()?.read_register(self.runtime, *arg)?))
            .collect()
    }

    fn dispatch_call(
        &mut self,
        dst: Option<Register>,
        callee: &CallTarget<DefinitionId>,
        args: &[Register],
    ) -> Result<(), VmError> {
        if let CallTarget::Native(import) = callee {
            return self
                .stack
                .invoke_native(self.runtime, *import, args, dst, invoke_script)
                .map_err(VmError::RuntimeError);
        }
        let direct = match *callee {
            CallTarget::Function(function) => Some((self.current_frame()?.module(), function)),
            CallTarget::ModuleFunction { module, function } => Some((module, function)),
            _ => None,
        };
        if let Some((module, function)) = direct {
            return self
                .stack
                .push_registers(self.runtime, module, function, args, dst)
                .map_err(VmError::RuntimeError);
        }
        let arg_values = args
            .iter()
            .map(|arg| Ok::<_, VmError>(self.current_frame()?.read_register(self.runtime, *arg)?))
            .collect::<Result<Vec<_>, _>>()?;

        match *callee {
            CallTarget::Shared { .. } => self
                .stack
                .push_shared_call(self.runtime, &arg_values, dst)
                .map_err(VmError::RuntimeError),
            CallTarget::Native(_) => unreachable!("native calls execute before argument packing"),
            CallTarget::ModuleFunction { .. } | CallTarget::Function(_) => {
                unreachable!("direct script calls use frame windows")
            }
            CallTarget::InterfaceMethod { ref contract, .. } => {
                let receiver = arg_values.first().unwrap_or(&Value::Unit);
                let resolved = self
                    .runtime
                    .resolve_interface_call(&*self.current_frame()?, contract, receiver)
                    .map_err(VmError::RuntimeError)?;
                let arguments = if contract.receiver.is_some() {
                    arg_values
                } else {
                    iter::once(resolved.receiver().clone())
                        .chain(arg_values.into_iter().skip(1))
                        .collect::<Vec<_>>()
                };
                self.stack
                    .push_interface_method(self.runtime, resolved, &arguments, dst)
                    .map_err(VmError::RuntimeError)
            }
            CallTarget::Register(_) => {
                Err(VmError::UnsupportedCallTarget(Box::new(callee.clone())))
            }
            CallTarget::ClosureRegister {
                register,
                ref params,
                return_type,
            } => {
                let (params, return_type) = self
                    .current_frame()?
                    .closure_signature(register, params, return_type)
                    .map_err(VmError::RuntimeError)?;
                let value = self
                    .current_frame()?
                    .read_register(self.runtime, register)?;
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
                drop(actual_params);
                drop(closure);
                self.stack
                    .push_closure(self.runtime, &value, &arg_values, dst)
                    .map_err(VmError::RuntimeError)
            }
            CallTarget::RuntimePrimitive(intrinsic) => {
                self.dispatch_standard_intrinsic(intrinsic, dst, arg_values)
            }
            CallTarget::RuntimeHelper(ref helper) => {
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
            .invoke_standard_builtin(self.current_frame()?.loaded(), intrinsic, &args)
            .map_err(VmError::from)?;
        if let Some(dst) = dst {
            self.current_frame_mut()?
                .write_register(self.runtime, dst, value)?;
        }
        Ok(())
    }

    fn dispatch_runtime_helper(
        &mut self,
        helper: &RuntimeHelper,
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
                    self.current_frame_mut()?
                        .write_register(self.runtime, dst, reflected)?;
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
                    .reflect_get_field(base, field_name)
                    .map_err(VmError::RuntimeError)?;
                if let Some(dst) = dst {
                    self.current_frame_mut()?
                        .write_register(self.runtime, dst, reflected)?;
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
                    .reflect_set_field(base, field_name, next_value.clone())
                    .map_err(VmError::RuntimeError)?;
                if let Some(dst) = dst {
                    self.current_frame_mut()?
                        .write_register(self.runtime, dst, reflected)?;
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
                    self.current_frame_mut()?
                        .write_register(self.runtime, dst, reflected)?;
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
