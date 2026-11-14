use crate::{
    error::{RuntimeError, fault::EngineFault, trap::TrapReason},
    executor::Interpreter,
    frame::{paths::PathAction, transfer::ReturnValue},
    host::HostPathDescriptorId,
};

use kagari_bytecode::instruction::{BytecodeInstruction, CallTarget, PathId, ScalarOperand, Slot};

pub(super) enum InstructionProgress {
    Continue,
    Await,
    Return(ReturnValue),
}

impl<'a> Interpreter<'a> {
    pub(super) fn dispatch_instruction(
        &mut self,
        instruction: &BytecodeInstruction,
    ) -> Result<InstructionProgress, RuntimeError> {
        match *instruction {
            BytecodeInstruction::Unary { dst, op, operand } => {
                self.stack
                    .as_ref()
                    .expect("active interpreter")
                    .apply_scalar_unary(self.runtime, dst, op, operand)?;
            }
            BytecodeInstruction::PredicateBranch {
                op,
                lhs: ScalarOperand::Slot(lhs),
                rhs: ScalarOperand::Slot(rhs),
                negated,
                then_target,
                else_target,
            } => {
                self.stack
                    .as_ref()
                    .expect("active interpreter")
                    .branch_scalar_predicate(
                        self.runtime,
                        op,
                        [lhs, rhs],
                        negated,
                        (then_target.index(), else_target.index()),
                    )?;
            }
            BytecodeInstruction::Binary { dst, op, lhs, rhs } => {
                self.stack
                    .as_ref()
                    .expect("active interpreter")
                    .apply_scalar_binary(self.runtime, dst, op, lhs, rhs)?;
            }
            BytecodeInstruction::Convert {
                dst,
                src,
                conversion,
            } => {
                self.stack
                    .as_ref()
                    .expect("active interpreter")
                    .convert_scalar(self.runtime, dst, conversion, src)?;
            }
            BytecodeInstruction::Numeric {
                dst,
                operation,
                lhs,
                rhs,
            } => {
                self.stack
                    .as_ref()
                    .expect("active interpreter")
                    .apply_numeric(self.runtime, dst, operation, lhs, rhs)?;
            }

            BytecodeInstruction::TestEnumVariant { dst, value, .. } => {
                self.stack
                    .as_ref()
                    .expect("active interpreter")
                    .test_enum_variant(self.runtime, dst, value)?;
            }
            BytecodeInstruction::ReadEnumPayload {
                dst, value, index, ..
            } => {
                self.stack
                    .as_ref()
                    .expect("active interpreter")
                    .read_enum_payload(self.runtime, dst, value, index)?;
            }
            BytecodeInstruction::LoadModule { dst, slot } => {
                let stack = self.stack.as_ref().expect("active interpreter");
                if !stack.ensure_static_ready(self.runtime, slot)? {
                    return Ok(InstructionProgress::Continue);
                }
                stack.load_module_slot(self.runtime, dst, slot)?;
            }
            BytecodeInstruction::StoreModule { slot, src } => {
                let stack = self.stack.as_ref().expect("active interpreter");
                if !stack.ensure_static_ready(self.runtime, slot)? {
                    return Ok(InstructionProgress::Continue);
                }
                stack.store_module_slot(self.runtime, slot, src)?;
            }
            BytecodeInstruction::Call {
                dst,
                ref callee,
                ref args,
            } => {
                return self.dispatch_call(dst, callee, args);
            }
            BytecodeInstruction::Unreachable => {
                return Err(RuntimeError::from(TrapReason::Provider(
                    ("unreachable").into(),
                )));
            }
            BytecodeInstruction::MakeTuple { dst, ref elements } => {
                self.stack
                    .as_ref()
                    .expect("active interpreter")
                    .make_tuple(self.runtime, dst, elements)?;
            }
            BytecodeInstruction::RangeBound {
                dst,
                value,
                ref range,
                ref bound,
                upper,
            } => {
                self.stack
                    .as_ref()
                    .expect("active interpreter")
                    .range_bound(self.runtime, dst, value, range, bound, upper)?;
            }
            BytecodeInstruction::MakeRange {
                dst,
                start,
                end,
                ref ty,
            } => {
                self.stack
                    .as_ref()
                    .expect("active interpreter")
                    .make_range(self.runtime, dst, ty, start, end)?;
            }
            BytecodeInstruction::RepeatArray {
                dst,
                value,
                count,
                ref element,
            } => {
                self.stack
                    .as_ref()
                    .expect("active interpreter")
                    .repeat_array(self.runtime, dst, element, value, count)?;
            }
            BytecodeInstruction::MakeArray {
                dst,
                ref elements,
                ref element,
            } => {
                self.stack
                    .as_ref()
                    .expect("active interpreter")
                    .make_array(self.runtime, dst, element, elements)?;
            }
            BytecodeInstruction::MakeFuture {
                dst,
                function,
                ref arguments,
                ref future,
            } => {
                self.stack
                    .as_ref()
                    .expect("active interpreter")
                    .make_future(self.runtime, function, arguments, dst, future)?;
            }
            BytecodeInstruction::MakeClosure {
                dst,
                function,
                ref captures,
            } => {
                self.stack
                    .as_ref()
                    .expect("active interpreter")
                    .make_closure(self.runtime, function, captures, dst)?;
            }
            BytecodeInstruction::MakeCell { dst, value } => {
                self.stack.as_ref().expect("active interpreter").make_cell(
                    self.runtime,
                    dst,
                    value,
                )?;
            }
            BytecodeInstruction::ReadCell { dst, cell } => {
                self.stack.as_ref().expect("active interpreter").read_cell(
                    self.runtime,
                    dst,
                    cell,
                )?;
            }
            BytecodeInstruction::WriteCell { cell, value } => {
                self.stack
                    .as_ref()
                    .expect("active interpreter")
                    .write_cell(self.runtime, cell, value)?;
            }
            BytecodeInstruction::UpcastInterface { .. } => {
                return Err(self.runtime.quarantine_execution_invariant(
                    EngineFault::PreparedInterfaceUpcastReachedCanonicalDispatch,
                ));
            }
            BytecodeInstruction::MakeInterface {
                dst,
                value,
                ref target,
            } => {
                self.stack
                    .as_ref()
                    .expect("active interpreter")
                    .make_interface(self.runtime, dst, value, target)?;
            }
            BytecodeInstruction::MakeEnum {
                dst, ref fields, ..
            } => {
                self.stack.as_ref().expect("active interpreter").make_enum(
                    self.runtime,
                    dst,
                    fields,
                )?;
            }
            BytecodeInstruction::MakeStruct {
                dst, ref fields, ..
            } => {
                self.stack
                    .as_ref()
                    .expect("active interpreter")
                    .make_struct(self.runtime, dst, fields)?;
            }
            BytecodeInstruction::ReadPath {
                dst,
                root_or_view,
                path,
                ref dynamic_args,
            } => {
                let descriptor = self.descriptor_id(path)?;
                self.stack
                    .as_ref()
                    .expect("active interpreter")
                    .access_host_path(
                        self.runtime,
                        descriptor,
                        root_or_view,
                        dynamic_args,
                        PathAction::Read(dst),
                    )?;
            }
            BytecodeInstruction::SetPath {
                root_or_view,
                path,
                ref dynamic_args,
                value,
            } => {
                let descriptor = self.descriptor_id(path)?;
                self.stack
                    .as_ref()
                    .expect("active interpreter")
                    .access_host_path(
                        self.runtime,
                        descriptor,
                        root_or_view,
                        dynamic_args,
                        PathAction::Set(value),
                    )?;
            }
            BytecodeInstruction::ModifyPath {
                dst,
                root_or_view,
                path,
                ref dynamic_args,
                op,
                value,
            } => {
                let descriptor = self.descriptor_id(path)?;
                self.stack
                    .as_ref()
                    .expect("active interpreter")
                    .access_host_path(
                        self.runtime,
                        descriptor,
                        root_or_view,
                        dynamic_args,
                        PathAction::Modify {
                            destination: dst,
                            operation: op,
                            value,
                        },
                    )?;
            }
            BytecodeInstruction::MakePathView {
                dst,
                root_or_view,
                path,
                ref dynamic_args,
            } => {
                let descriptor = self.descriptor_id(path)?;
                self.stack
                    .as_ref()
                    .expect("active interpreter")
                    .access_host_path(
                        self.runtime,
                        descriptor,
                        root_or_view,
                        dynamic_args,
                        PathAction::MakeView(dst),
                    )?;
            }
            BytecodeInstruction::Await {
                dst,
                value,
                ref future,
            } => {
                self.stack
                    .as_ref()
                    .expect("active interpreter")
                    .begin_await(self.runtime, value, dst, future)?;
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
                return Err(RuntimeError::from(EngineFault::HostInvariant(
                    "cursor operation at slow boundary",
                )));
            }
        }

        Ok(InstructionProgress::Continue)
    }

    fn dispatch_call(
        &mut self,
        dst: Option<Slot>,
        callee: &CallTarget,
        args: &[Slot],
    ) -> Result<InstructionProgress, RuntimeError> {
        if matches!(
            callee,
            CallTarget::Native(_)
                | CallTarget::Function(_)
                | CallTarget::ModuleFunction { .. }
                | CallTarget::Shared { .. }
                | CallTarget::InterfaceMethod { .. }
                | CallTarget::ClosureRegister { .. }
        ) {
            return Err(self.runtime.quarantine_execution_invariant(
                EngineFault::PreparedCallReachedCanonicalDispatch,
            ));
        }
        if matches!(callee, CallTarget::Slot(_)) {
            return Err(RuntimeError::from(
                EngineFault::VerifiedCallTargetIsUnsupported,
            ));
        }
        self.stack
            .as_ref()
            .expect("active interpreter")
            .invoke_runtime_call(self.runtime, callee, args, dst)?;
        Ok(InstructionProgress::Continue)
    }
}

impl Interpreter<'_> {
    fn descriptor_id(&self, path: PathId) -> Result<HostPathDescriptorId, RuntimeError> {
        self.current_loaded()?
            .path_binding(path)
            .ok_or(RuntimeError::from(EngineFault::HostInvariant(
                "missing linked path",
            )))
    }
}
