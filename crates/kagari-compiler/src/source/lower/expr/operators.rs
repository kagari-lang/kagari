use crate::source::{
    elaborate::model::{CheckedCall, CheckedCallTarget},
    lower::{
        MirLoweringError,
        instances::methods::{MethodApplication, MethodOperation},
        state::FunctionLowerer,
    },
};
use kagari_abi::representation::ValueType;
use kagari_common::identity::table::DefinitionId;
use kagari_contract::{
    operations::{BinaryOp, UnaryOp},
    standard::RuntimePrimitive,
};
use kagari_hir::{
    hir::expr::ops::BinaryOp as HirBinaryOp,
    language::semantics as traits,
    types::{NominalType, TypeId},
};
use kagari_mir::instruction::{
    CallTarget, Constant, Instruction, MirValue, Terminator, ValueBuffer,
};
use kagari_types::scalar::BuiltinType;

impl FunctionLowerer<'_, '_> {
    pub(super) fn check_integer_range(&mut self, value: MirValue, target: &TypeId) {
        let representation = value.ty;

        let range = match target {
            TypeId::Builtin(BuiltinType::I8) => Some((i64::from(i8::MIN), i64::from(i8::MAX))),
            TypeId::Builtin(BuiltinType::I16) => Some((i64::from(i16::MIN), i64::from(i16::MAX))),
            TypeId::Builtin(BuiltinType::U8) => Some((0, i64::from(u8::MAX))),
            TypeId::Builtin(BuiltinType::U16) => Some((0, i64::from(u16::MAX))),
            TypeId::Builtin(BuiltinType::U32) => Some((0, i64::from(u32::MAX))),
            _ => None,
        };
        if let Some((minimum, maximum)) = range {
            let message =
                self.lower_constant(Constant::Str("integer overflow".into()), ValueType::Str);
            for (limit, comparison) in [(minimum, BinaryOp::Ge), (maximum, BinaryOp::Le)] {
                let constant = if representation == ValueType::I32 {
                    Constant::I32(limit as i32)
                } else {
                    Constant::I64(limit)
                };
                let limit = self.lower_constant(constant, representation);
                let valid = self.alloc_temp(ValueType::Bool);
                self.emit(Instruction::Binary {
                    dst: valid,
                    op: comparison,
                    lhs: value,
                    rhs: limit,
                });
                self.emit_intrinsic(RuntimePrimitive::Assert, &[valid, message], ValueType::Unit);
            }
        }
    }

    pub(super) fn lower_selected_operator(
        &mut self,
        call: &CheckedCall,
        args: &[MirValue],
    ) -> Result<MirValue, MirLoweringError> {
        let CheckedCallTarget::Method {
            method,
            interface,
            receiver,
            arguments,
            ..
        } = &call.target
        else {
            return Err(MirLoweringError::MissingBinding("operator method"));
        };
        self.lower_applied_method(interface.clone(), receiver.clone(), method, arguments, args)
    }

    pub(crate) fn lower_applied_operator(
        &mut self,
        interface: NominalType,
        receiver: TypeId,
        method: &DefinitionId,
        args: &[MirValue],
    ) -> Result<MirValue, MirLoweringError> {
        self.lower_applied_method(interface, receiver, method, &[], args)
    }

    pub(crate) fn lower_applied_method(
        &mut self,
        interface: NominalType,
        receiver: TypeId,
        method: &DefinitionId,
        method_arguments: &[TypeId],
        args: &[MirValue],
    ) -> Result<MirValue, MirLoweringError> {
        let plan = self.planner.method_plan(
            MethodApplication {
                interface: &interface,
                receiver: &receiver,
                method,
                arguments: method_arguments,
            },
            &self.instance.substitution,
            &self.implementation_bounds,
            self.function.debug.source_span,
        )?;
        if matches!(plan.operation, MethodOperation::Identity) {
            return Ok(args[0]);
        }
        if let MethodOperation::Protocol { protocol, receiver } = &plan.operation {
            return self.lower_protocol(*protocol, receiver, args, 0);
        }
        let dst = self.alloc_temp(self.value_type(&plan.result)?);
        self.function
            .semantic
            .registers
            .insert(dst.temp.index(), self.planner.encode_type(&plan.result));
        match plan.operation {
            MethodOperation::Call(callee) => self.emit(Instruction::Call {
                dst: Some(dst),
                callee,
                args: args.iter().copied().collect(),
            }),
            MethodOperation::Callable { params } => {
                let mut values = ValueBuffer::new();
                let mut representations = Vec::new();
                for (position, param) in params.iter().enumerate() {
                    let representation = self.value_type(param)?;
                    let value = self.alloc_temp(representation);
                    self.function
                        .semantic
                        .registers
                        .insert(value.temp.index(), self.planner.encode_type(param));
                    let index = self.lower_constant(Constant::I32(position as i32), ValueType::I32);
                    self.emit(Instruction::ReadAggregateIndex {
                        dst: value,
                        base: args[1],
                        index,
                    });
                    values.push(value);
                    representations.push(representation);
                }
                self.emit(Instruction::Call {
                    dst: Some(dst),
                    callee: CallTarget::Closure {
                        value: args[0],
                        params: representations,
                        return_type: dst.ty,
                    },
                    args: values,
                });
            }
            MethodOperation::Convert(conversion) => self.emit(Instruction::Convert {
                dst,
                src: args[0],
                conversion,
            }),
            MethodOperation::Numeric(operation) => self.emit(Instruction::Numeric {
                dst,
                operation,
                lhs: args[0],
                rhs: args.get(1).copied(),
            }),
            MethodOperation::NegateSmallInteger(operation) => {
                let zero = self.lower_constant(Constant::I32(0), dst.ty);
                self.function.semantic.registers.insert(
                    zero.temp.index(),
                    self.planner.encode_type(&TypeId::Builtin(operation.input)),
                );
                self.emit(Instruction::Numeric {
                    dst,
                    operation,
                    lhs: zero,
                    rhs: Some(args[0]),
                });
            }
            MethodOperation::Binary(op) => {
                self.emit(Instruction::Binary {
                    dst,
                    op,
                    lhs: args[0],
                    rhs: args[1],
                });
                self.check_integer_range(dst, &plan.result);
            }
            MethodOperation::Unary(op) => {
                self.emit(Instruction::Unary {
                    dst,
                    op,
                    operand: args[0],
                });
                self.check_integer_range(dst, &plan.result);
            }
            MethodOperation::Index => self.emit(Instruction::ReadAggregateIndex {
                dst,
                base: args[0],
                index: args[1],
            }),
            MethodOperation::Identity | MethodOperation::Protocol { .. } => unreachable!(),
        }
        Ok(dst)
    }

    pub(super) fn lower_ordering_operator(
        &mut self,
        call: &CheckedCall,
        op: HirBinaryOp,
        args: &[MirValue],
    ) -> Result<MirValue, MirLoweringError> {
        // Primitive numeric and character comparisons use direct instructions.
        if matches!(
            args[0].ty,
            ValueType::I32
                | ValueType::I64
                | ValueType::U64
                | ValueType::F32
                | ValueType::F64
                | ValueType::Char
        ) {
            let dst = self.alloc_temp(ValueType::Bool);
            let op = match op {
                HirBinaryOp::Lt => BinaryOp::Lt,
                HirBinaryOp::Le => BinaryOp::Le,
                HirBinaryOp::Gt => BinaryOp::Gt,
                _ => BinaryOp::Ge,
            };
            self.emit(Instruction::Binary {
                dst,
                op,
                lhs: args[0],
                rhs: args[1],
            });
            return Ok(dst);
        }

        let value = self.lower_selected_operator(call, args)?;
        let optional = traits::ordering_type(&self.planner.definitions(), true);
        let ordering = traits::ordering_type(&self.planner.definitions(), false);
        let some = self.test_enum_variant(&optional, value, 0)?;
        let body = self.new_block();
        let absent = self.new_block();
        let join = self.new_block();
        let dst = self.alloc_temp(ValueType::Bool);
        self.set_terminator(Terminator::Branch {
            cond: some,
            then_block: body,
            else_block: absent,
        });
        self.switch_to_block(body);
        let order = self.read_enum_field(&optional, value, 0, 0)?;
        let tag = if matches!(op, HirBinaryOp::Lt | HirBinaryOp::Ge) {
            0
        } else {
            2
        };
        let test = self.test_enum_variant(&ordering, order, tag)?;
        if matches!(op, HirBinaryOp::Le | HirBinaryOp::Ge) {
            self.emit(Instruction::Unary {
                dst,
                op: UnaryOp::Not,
                operand: test,
            });
        } else {
            self.emit(Instruction::Move { dst, src: test });
        }
        self.set_terminator(Terminator::Jump(join));
        self.switch_to_block(absent);
        let no = self.lower_constant(Constant::Bool(false), ValueType::Bool);
        self.emit(Instruction::Move { dst, src: no });
        self.set_terminator(Terminator::Jump(join));
        self.switch_to_block(join);
        Ok(dst)
    }
}
