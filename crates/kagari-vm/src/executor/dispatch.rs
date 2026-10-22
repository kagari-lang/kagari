use crate::{error::VmError, executor::Executor};
use kagari_bytecode::instruction::{
    BytecodeInstruction, CallTarget, ConstructionTarget as BytecodeConstruction, PathId,
    RuntimeHelper, ScalarOperand, Slot,
};
use kagari_contract::{operations::IterOp, standard::RuntimePrimitive};
use kagari_runtime::{
    frame::transfer::ReturnValue, host::HostPathDescriptorId, numeric, range::RangeValue,
    value::Value,
};
use kagari_types::ty::Ty;
use std::{ops::Bound, slice};

pub(super) enum InstructionProgress {
    Continue,
    Await,
    Return(ReturnValue),
}

impl<'a> Executor<'a> {
    fn dispatch_iterator(
        &mut self,
        source: &Value,
        ty: &Ty,
        op: IterOp,
        dst: Option<Slot>,
    ) -> Result<(), VmError> {
        let result = self
            .runtime
            .iter_operation(self.current_frame()?.loaded(), source, ty, op)?;
        if let Some(dst) = dst {
            self.current_frame_mut()?
                .write_slot(self.runtime, dst, result)?;
        }
        Ok(())
    }

    fn dispatch_range_bound(
        &mut self,
        value: Value,
        range: &Ty,
        bound: &Ty,
        upper: bool,
        dst: Option<Slot>,
    ) -> Result<(), VmError> {
        let Value::Range(value) = value else {
            return Err(VmError::Trap("invalid range value"));
        };
        let value = self
            .runtime
            .gc()
            .range(value)
            .ok_or(VmError::Trap("invalid range value"))?;
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
                .write_slot(self.runtime, dst, result)?;
        }
        Ok(())
    }

    pub(super) fn dispatch_instruction(
        &mut self,
        instruction: &BytecodeInstruction,
    ) -> Result<InstructionProgress, VmError> {
        match *instruction {
            BytecodeInstruction::Unary { dst, op, operand } => {
                let value = self.current_frame()?.read_slot(self.runtime, operand)?;
                let value = Self::apply_unary(op, value)?;
                self.current_frame_mut()?
                    .write_slot(self.runtime, dst, value)?;
            }
            BytecodeInstruction::PredicateBranch {
                op,
                lhs: ScalarOperand::Slot(lhs),
                rhs: ScalarOperand::Slot(rhs),
                negated,
                then_target,
                else_target,
            } => {
                let lhs = self.current_frame()?.read_slot(self.runtime, lhs)?;
                let rhs = self.current_frame()?.read_slot(self.runtime, rhs)?;
                let Value::Bool(value) = self.apply_binary(op, lhs, rhs)? else {
                    return Err(VmError::TypeMismatch("predicate result must be bool"));
                };
                let target = if value != negated {
                    then_target
                } else {
                    else_target
                };
                self.current_frame_mut()?
                    .jump_to(self.runtime, target.index())?;
            }
            BytecodeInstruction::Binary { dst, op, lhs, rhs } => {
                let lhs = self.current_frame()?.read_slot(self.runtime, lhs)?;
                let rhs = self.current_frame()?.read_slot(self.runtime, rhs)?;
                let value = self.apply_binary(op, lhs, rhs)?;
                self.current_frame_mut()?
                    .write_slot(self.runtime, dst, value)?;
            }
            BytecodeInstruction::Convert {
                dst,
                src,
                conversion,
            } => {
                let value = self.current_frame()?.read_slot(self.runtime, src)?;
                let value = numeric::convert(conversion, value)?;
                self.current_frame_mut()?
                    .write_slot(self.runtime, dst, value)?;
            }
            BytecodeInstruction::Numeric {
                dst,
                operation,
                lhs,
                rhs,
            } => {
                let lhs = self.current_frame()?.read_slot(self.runtime, lhs)?;
                let rhs = rhs
                    .map(|r| {
                        self.current_frame()?
                            .read_slot(self.runtime, r)
                            .map_err(VmError::RuntimeError)
                    })
                    .transpose()?;
                let value = numeric::fixed_integer(operation, lhs, rhs)?;
                self.current_frame_mut()?
                    .write_slot(self.runtime, dst, value)?;
            }
            BytecodeInstruction::Iter {
                dst,
                value,
                ref ty,
                op,
            } => {
                let source = self.current_frame()?.read_slot(
                    self.runtime,
                    value.ok_or(VmError::TypeMismatch("iterator source"))?,
                )?;
                self.dispatch_iterator(&source, ty, op, Some(dst))?;
            }

            BytecodeInstruction::TestEnumVariant { dst, value, .. } => {
                let result = self.test_enum_variant(value)?;
                self.current_frame_mut()?
                    .write_slot(self.runtime, dst, result)?;
            }
            BytecodeInstruction::ReadEnumPayload {
                dst, value, index, ..
            } => {
                let result = self.read_enum_payload(value, index)?;
                self.current_frame_mut()?
                    .write_slot(self.runtime, dst, result)?;
            }
            BytecodeInstruction::LoadModule { dst, slot } => {
                if !self.stack.ensure_static_ready(self.runtime, slot)? {
                    return Ok(InstructionProgress::Continue);
                }
                let loaded = self.current_loaded()?;
                let value = self.runtime.read_module_slot(&loaded, slot)?;
                self.current_frame_mut()?
                    .write_slot(self.runtime, dst, value)?;
            }
            BytecodeInstruction::StoreModule { slot, src } => {
                if !self.stack.ensure_static_ready(self.runtime, slot)? {
                    return Ok(InstructionProgress::Continue);
                }
                let value = self.current_frame()?.read_slot(self.runtime, src)?;
                let loaded = self.current_loaded()?;
                self.runtime.write_module_slot(&loaded, slot, value)?;
            }
            BytecodeInstruction::Call {
                dst,
                ref callee,
                ref args,
            } => {
                return self.dispatch_call(dst, callee, args);
            }
            BytecodeInstruction::Unreachable => {
                return Err(VmError::Trap("unreachable"));
            }
            BytecodeInstruction::MakeTuple { dst, ref elements } => {
                let value = self.make_tuple(elements)?;
                self.current_frame_mut()?
                    .write_slot(self.runtime, dst, value)?;
            }
            BytecodeInstruction::RangeBound {
                dst,
                value,
                ref range,
                ref bound,
                upper,
            } => {
                let value = self.current_frame()?.read_slot(self.runtime, value)?;
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
                    .map(|r| frame.read_slot(self.runtime, r))
                    .transpose()?;
                let end = end.map(|r| frame.read_slot(self.runtime, r)).transpose()?;
                drop(frame);
                let value = RangeValue::new(ty, start.as_ref(), end.as_ref())
                    .map_err(VmError::RuntimeError)?;
                let value = self.runtime.gc().alloc_range(value)?;
                self.current_frame_mut()?
                    .write_slot(self.runtime, dst, value)?;
            }
            BytecodeInstruction::RepeatArray {
                dst,
                value,
                count,
                ref element,
            } => {
                let value = self.current_frame()?.read_slot(self.runtime, value)?;
                let Value::U64(count) = self.current_frame()?.read_slot(self.runtime, count)?
                else {
                    return Err(VmError::Trap("invalid repeat array count"));
                };
                let array = self
                    .current_frame()?
                    .alloc_array_repeat(self.runtime, element, value, count)
                    .map_err(VmError::RuntimeError)?;
                self.current_frame_mut()?
                    .write_slot(self.runtime, dst, Value::Array(array))?;
            }
            BytecodeInstruction::MakeArray {
                dst,
                ref elements,
                ref element,
            } => {
                let value = self.make_array(element, elements)?;
                self.current_frame_mut()?
                    .write_slot(self.runtime, dst, value)?;
            }
            BytecodeInstruction::MakeFuture {
                dst,
                function,
                ref arguments,
                ref future,
            } => {
                let values = self.read_path_args(arguments)?;
                let value = self
                    .stack
                    .make_future(self.runtime, function, values, future)?;
                self.current_frame_mut()?
                    .write_slot(self.runtime, dst, value)?;
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
                    .write_slot(self.runtime, dst, closure)?;
            }
            BytecodeInstruction::MakeCell { dst, value } => {
                let ty = self.current_frame()?.slot_type(self.runtime, value)?;
                let value = self.current_frame()?.read_slot(self.runtime, value)?;
                let cell = self
                    .runtime
                    .make_capture_cell(ty, value)
                    .map_err(VmError::RuntimeError)?;
                self.current_frame_mut()?
                    .write_slot(self.runtime, dst, cell)?;
            }
            BytecodeInstruction::ReadCell { dst, cell } => {
                let ty = self.current_frame()?.slot_type(self.runtime, dst)?;
                let cell = self.current_frame()?.read_slot(self.runtime, cell)?;
                let value = self
                    .runtime
                    .read_capture_cell(&cell, ty)
                    .map_err(VmError::RuntimeError)?;
                self.current_frame_mut()?
                    .write_slot(self.runtime, dst, value)?;
            }
            BytecodeInstruction::WriteCell { cell, value } => {
                let ty = self.current_frame()?.slot_type(self.runtime, value)?;
                let cell = self.current_frame()?.read_slot(self.runtime, cell)?;
                let value = self.current_frame()?.read_slot(self.runtime, value)?;
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
                let value = self.current_frame()?.read_slot(self.runtime, value)?;
                let view = self
                    .runtime
                    .upcast_interface(&value, &source, &target)
                    .map_err(VmError::RuntimeError)?;
                self.current_frame_mut()?
                    .write_slot(self.runtime, dst, view)?;
            }
            BytecodeInstruction::MakeInterface {
                dst,
                value,
                target: BytecodeConstruction::Bound(ref required),
            } => {
                let frame = self.current_frame()?;
                let receiver = frame.read_slot(self.runtime, value)?;
                let environment = frame.environment().ok_or(VmError::UnsupportedInstruction(
                    "missing construction environment",
                ))?;
                let owner = frame.loaded().clone();
                drop(frame);
                let interface = self
                    .runtime
                    .make_bound_interface(&owner, &environment, required, receiver)
                    .map_err(VmError::RuntimeError)?;
                self.current_frame_mut()?
                    .write_slot(self.runtime, dst, interface)?;
            }
            BytecodeInstruction::MakeInterface {
                dst,
                value,
                target:
                    BytecodeConstruction::Declared {
                        module,
                        implementation,
                        ref arguments,
                    },
            } => {
                let receiver = self.current_frame()?.read_slot(self.runtime, value)?;
                let arguments = self
                    .current_frame()?
                    .type_arguments(self.runtime, arguments)?;
                let loaded = self.current_loaded()?.member(module).ok_or(
                    VmError::UnsupportedInstruction("invalid interface module slot"),
                )?;
                let environment = self.current_frame()?.environment();
                let interface = self
                    .runtime
                    .make_interface_applied(
                        &loaded,
                        implementation.index(),
                        &arguments,
                        environment.as_ref(),
                        receiver,
                    )
                    .map_err(VmError::RuntimeError)?;
                self.current_frame_mut()?
                    .write_slot(self.runtime, dst, interface)?;
            }
            BytecodeInstruction::MakeEnum {
                dst, ref fields, ..
            } => {
                let value = self.make_enum(fields)?;
                self.current_frame_mut()?
                    .write_slot(self.runtime, dst, value)?;
            }
            BytecodeInstruction::MakeStruct {
                dst, ref fields, ..
            } => {
                let value = self.make_struct(fields)?;
                self.current_frame_mut()?
                    .write_slot(self.runtime, dst, value)?;
            }
            BytecodeInstruction::ReadPath {
                dst,
                root_or_view,
                path,
                ref dynamic_args,
            } => {
                let root_or_view = self
                    .current_frame()?
                    .read_slot(self.runtime, root_or_view)?;
                let dynamic_args = self.read_path_args(dynamic_args)?;
                let value = self
                    .runtime
                    .read_host_path(&root_or_view, self.descriptor_id(path)?, dynamic_args)
                    .map_err(VmError::RuntimeError)?;
                self.current_frame_mut()?
                    .write_slot(self.runtime, dst, value)?;
            }
            BytecodeInstruction::SetPath {
                root_or_view,
                path,
                ref dynamic_args,
                value,
            } => {
                let root_or_view = self
                    .current_frame()?
                    .read_slot(self.runtime, root_or_view)?;
                let dynamic_args = self.read_path_args(dynamic_args)?;
                let value = self.current_frame()?.read_slot(self.runtime, value)?;
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
                    .read_slot(self.runtime, root_or_view)?;
                let dynamic_args = self.read_path_args(dynamic_args)?;
                let value = self.current_frame()?.read_slot(self.runtime, value)?;
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
                        .write_slot(self.runtime, dst, value)?;
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
                    .read_slot(self.runtime, root_or_view)?;
                let dynamic_args = self.read_path_args(dynamic_args)?;
                let view = self
                    .runtime
                    .make_host_path_view_from_value(
                        &root_or_view,
                        self.descriptor_id(path)?,
                        dynamic_args,
                    )
                    .map_err(VmError::RuntimeError)?;
                let value = self.runtime.gc().alloc_host_path(view)?;
                self.current_frame_mut()?
                    .write_slot(self.runtime, dst, value)?;
            }
            BytecodeInstruction::Await {
                dst,
                value,
                ref future,
            } => {
                let value = self.current_frame()?.read_slot(self.runtime, value)?;
                self.stack.begin_await(self.runtime, value, dst, future)?;
                return Ok(InstructionProgress::Await);
            }
            BytecodeInstruction::ReadAggregateField { .. }
            | BytecodeInstruction::WriteAggregateField { .. }
            | BytecodeInstruction::ReadAggregateIndex { .. }
            | BytecodeInstruction::WriteAggregateIndex { .. }
            | BytecodeInstruction::LoadConst { .. }
            | BytecodeInstruction::Jump { .. }
            | BytecodeInstruction::Branch { .. }
            | BytecodeInstruction::BinaryOperands { .. }
            | BytecodeInstruction::PredicateBranch { .. }
            | BytecodeInstruction::Move { .. }
            | BytecodeInstruction::Return(_) => {
                return Err(VmError::UnsupportedInstruction(
                    "cursor operation at slow boundary",
                ));
            }
        }

        Ok(InstructionProgress::Continue)
    }

    fn read_path_args(&self, args: &[Slot]) -> Result<Vec<Value>, VmError> {
        args.iter()
            .map(|arg| Ok::<_, VmError>(self.current_frame()?.read_slot(self.runtime, *arg)?))
            .collect()
    }

    fn dispatch_call(
        &mut self,
        dst: Option<Slot>,
        callee: &CallTarget,
        args: &[Slot],
    ) -> Result<InstructionProgress, VmError> {
        if matches!(
            callee,
            CallTarget::Native(_)
                | CallTarget::Function(_)
                | CallTarget::ModuleFunction { .. }
                | CallTarget::Shared { .. }
                | CallTarget::InterfaceMethod { .. }
                | CallTarget::ClosureRegister { .. }
        ) {
            return Err(VmError::RuntimeError(
                self.runtime
                    .quarantine_execution_invariant("prepared call reached canonical dispatch"),
            ));
        }
        let arg_values = args
            .iter()
            .map(|arg| Ok::<_, VmError>(self.current_frame()?.read_slot(self.runtime, *arg)?))
            .collect::<Result<Vec<_>, _>>()?;

        match *callee {
            CallTarget::Native(_)
            | CallTarget::ModuleFunction { .. }
            | CallTarget::Function(_)
            | CallTarget::Shared { .. }
            | CallTarget::InterfaceMethod { .. }
            | CallTarget::ClosureRegister { .. } => {
                unreachable!("prepared calls use frame windows")
            }
            CallTarget::Slot(_) => Err(VmError::UnsupportedCallTarget(Box::new(callee.clone()))),
            CallTarget::RuntimePrimitive(intrinsic) => self
                .dispatch_standard_intrinsic(intrinsic, dst, arg_values)
                .map(|()| InstructionProgress::Continue),
            CallTarget::RuntimeHelper(ref helper) => self
                .dispatch_runtime_helper(helper, dst, arg_values)
                .map(|()| InstructionProgress::Continue),
        }
    }

    fn dispatch_standard_intrinsic(
        &mut self,
        intrinsic: RuntimePrimitive,
        dst: Option<Slot>,
        args: Vec<Value>,
    ) -> Result<(), VmError> {
        let owner = self.current_loaded()?;
        let value = self
            .runtime
            .invoke_standard_builtin(&owner, intrinsic, &args)
            .map_err(VmError::from)?;
        if let Some(dst) = dst {
            self.current_frame_mut()?
                .write_slot(self.runtime, dst, value)?;
        }
        Ok(())
    }

    fn dispatch_runtime_helper(
        &mut self,
        helper: &RuntimeHelper,
        dst: Option<Slot>,
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
                        .write_slot(self.runtime, dst, reflected)?;
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
                        .write_slot(self.runtime, dst, reflected)?;
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
                    .reflect_set_field(base, field_name, *next_value)
                    .map_err(VmError::RuntimeError)?;
                if let Some(dst) = dst {
                    self.current_frame_mut()?
                        .write_slot(self.runtime, dst, reflected)?;
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
                    .reflect_set_index(base, index, *next_value)
                    .map_err(VmError::RuntimeError)?;
                if let Some(dst) = dst {
                    self.current_frame_mut()?
                        .write_slot(self.runtime, dst, reflected)?;
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
