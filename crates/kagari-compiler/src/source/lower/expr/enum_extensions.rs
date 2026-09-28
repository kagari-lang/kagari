use crate::source::{
    lower::{MirLoweringError, state::FunctionLowerer},
    types::lower_type,
};
use kagari_abi::{
    operations::StandardEnumOp as Op,
    representation::ValueType,
    scalar::BuiltinType,
    standard::{StandardIntrinsic, surface::StandardEnum},
};
use kagari_hir::{hir, types::TypeId};
use kagari_mir::instruction::{Constant, Instruction, MirValue, Terminator};
use std::slice;

fn enum_args(ty: &TypeId) -> Result<&[TypeId], MirLoweringError> {
    match ty {
        TypeId::StandardEnum { args, .. } => Ok(args),
        _ => Err(MirLoweringError::MissingBinding("enum combinator type")),
    }
}

impl FunctionLowerer<'_, '_> {
    pub(super) fn branch_enum_value(
        &mut self,
        condition: MirValue,
        output: &TypeId,
        yes: impl FnOnce(&mut Self) -> Result<MirValue, MirLoweringError>,
        no: impl FnOnce(&mut Self) -> Result<MirValue, MirLoweringError>,
    ) -> Result<MirValue, MirLoweringError> {
        let yes_block = self.new_block();
        let no_block = self.new_block();
        let join = self.new_block();
        let dst = self.alloc_temp(self.value_type(output)?);
        self.set_terminator(Terminator::Branch {
            cond: condition,
            then_block: yes_block,
            else_block: no_block,
        });
        self.switch_to_block(yes_block);
        let value = yes(self)?;
        self.emit(Instruction::Move { dst, src: value });
        self.set_terminator(Terminator::Jump(join));
        self.switch_to_block(no_block);
        let value = no(self)?;
        self.emit(Instruction::Move { dst, src: value });
        self.set_terminator(Terminator::Jump(join));
        self.switch_to_block(join);
        Ok(dst)
    }

    fn preserve_enum_error(
        &mut self,
        output: &TypeId,
        original: MirValue,
        error: MirValue,
    ) -> Result<MirValue, MirLoweringError> {
        let concrete = self
            .planner
            .arguments(
                slice::from_ref(output),
                &self.instance.substitution,
                self.function.debug.source_span,
            )?
            .remove(0);
        let dst = self.alloc_temp(ValueType::HeapObject);
        self.emit(Instruction::MapResultError {
            dst,
            original,
            error,
            ty: lower_type(&concrete),
        });
        Ok(dst)
    }

    pub(super) fn lower_enum_extension(
        &mut self,
        site: hir::ExprId,
        operation: StandardIntrinsic,
        input: &TypeId,
        args: &[MirValue],
    ) -> Result<MirValue, MirLoweringError> {
        let output = self
            .analyzed
            .typed
            .type_table
            .expr_type(site)
            .ok_or(MirLoweringError::MissingExprType(site))?;
        let concrete = self.planner.arguments(
            &[input.clone(), output],
            &self.instance.substitution,
            self.function.debug.source_span,
        )?;
        let input = &concrete[0];
        let output = &concrete[1];
        let cond = self.standard_enum_op(input, Op::Test(0), Some(args[0]))?;
        self.branch_enum_value(
            cond,
            output,
            |this| this.enum_extension_branch(operation, input, output, args, 0),
            |this| this.enum_extension_branch(operation, input, output, args, 1),
        )
    }

    fn enum_extension_branch(
        &mut self,
        operation: StandardIntrinsic,
        input: &TypeId,
        output: &TypeId,
        args: &[MirValue],
        variant: u32,
    ) -> Result<MirValue, MirLoweringError> {
        let result_input = matches!(
            input,
            TypeId::StandardEnum {
                kind: StandardEnum::Result,
                ..
            }
        );
        let payload = if variant == 0 || result_input {
            Some(self.standard_enum_op(input, Op::Read(variant), Some(args[0]))?)
        } else {
            None
        };
        let callback_args: Vec<_> = payload.into_iter().collect();
        match operation {
            StandardIntrinsic::OptionUnwrapOrElse | StandardIntrinsic::ResultUnwrapOrElse => {
                if variant == 0 {
                    Ok(payload.unwrap())
                } else {
                    self.call_function_value(args[1], output, &callback_args)
                }
            }
            StandardIntrinsic::OptionOrElse | StandardIntrinsic::ResultOrElse => {
                if variant == 0 {
                    self.standard_enum_op(output, Op::Make(0), payload)
                } else {
                    self.call_function_value(args[1], output, &callback_args)
                }
            }
            StandardIntrinsic::OptionMapOr
            | StandardIntrinsic::ResultMapOr
            | StandardIntrinsic::OptionMapOrElse
            | StandardIntrinsic::ResultMapOrElse => {
                if variant == 0 {
                    self.call_function_value(args[2], output, &callback_args)
                } else if matches!(
                    operation,
                    StandardIntrinsic::OptionMapOr | StandardIntrinsic::ResultMapOr
                ) {
                    Ok(args[1])
                } else {
                    self.call_function_value(args[1], output, &callback_args)
                }
            }
            StandardIntrinsic::OptionIsSomeAnd
            | StandardIntrinsic::ResultIsOkAnd
            | StandardIntrinsic::ResultIsErrAnd => {
                if (variant == 1) == (operation == StandardIntrinsic::ResultIsErrAnd) {
                    self.call_function_value(args[1], output, &callback_args)
                } else {
                    Ok(self.lower_constant(Constant::Bool(false), ValueType::Bool))
                }
            }
            StandardIntrinsic::OptionFilter => {
                if variant == 1 {
                    return Ok(args[0]);
                }
                let cond = self.call_function_value(
                    args[1],
                    &TypeId::Builtin(BuiltinType::Bool),
                    &callback_args,
                )?;
                self.branch_enum_value(
                    cond,
                    output,
                    |_| Ok(args[0]),
                    |this| this.standard_enum_op(output, Op::Make(1), None),
                )
            }
            StandardIntrinsic::OptionZip => {
                if variant == 1 {
                    return self.standard_enum_op(output, Op::Make(1), None);
                }
                let TypeId::Tuple(fields) = &enum_args(output)?[0] else {
                    return Err(MirLoweringError::MissingBinding("zip tuple"));
                };
                let other = TypeId::StandardEnum {
                    kind: StandardEnum::Option,
                    args: vec![fields[1].clone()],
                };
                let cond = self.standard_enum_op(&other, Op::Test(0), Some(args[1]))?;
                self.branch_enum_value(
                    cond,
                    output,
                    |this| {
                        let right = this.standard_enum_op(&other, Op::Read(0), Some(args[1]))?;
                        let pair = this.alloc_temp(ValueType::HeapObject);
                        this.emit(Instruction::MakeTuple {
                            dst: pair,
                            elements: vec![payload.unwrap(), right].into(),
                        });
                        this.standard_enum_op(output, Op::Make(0), Some(pair))
                    },
                    |this| this.standard_enum_op(output, Op::Make(1), None),
                )
            }
            StandardIntrinsic::OptionFlatten | StandardIntrinsic::ResultFlatten => {
                if variant == 0 {
                    Ok(payload.unwrap())
                } else if result_input {
                    self.preserve_enum_error(output, args[0], payload.unwrap())
                } else {
                    self.standard_enum_op(output, Op::Make(1), None)
                }
            }
            StandardIntrinsic::ResultOk | StandardIntrinsic::ResultErr => {
                if (variant == 1) == (operation == StandardIntrinsic::ResultErr) {
                    self.standard_enum_op(output, Op::Make(0), payload)
                } else {
                    self.standard_enum_op(output, Op::Make(1), None)
                }
            }
            StandardIntrinsic::OptionTranspose => {
                let option = &enum_args(output)?[0];
                if variant == 1 {
                    let none = self.standard_enum_op(option, Op::Make(1), None)?;
                    return self.standard_enum_op(output, Op::Make(0), Some(none));
                }
                let inner = &enum_args(input)?[0];
                let value = payload.unwrap();
                let cond = self.standard_enum_op(inner, Op::Test(0), Some(value))?;
                self.branch_enum_value(
                    cond,
                    output,
                    |this| {
                        let member = this.standard_enum_op(inner, Op::Read(0), Some(value))?;
                        let some = this.standard_enum_op(option, Op::Make(0), Some(member))?;
                        this.standard_enum_op(output, Op::Make(0), Some(some))
                    },
                    |this| {
                        let error = this.standard_enum_op(inner, Op::Read(1), Some(value))?;
                        this.preserve_enum_error(output, value, error)
                    },
                )
            }
            StandardIntrinsic::ResultTranspose => {
                let result = &enum_args(output)?[0];
                if variant == 1 {
                    let error = self.preserve_enum_error(result, args[0], payload.unwrap())?;
                    return self.standard_enum_op(output, Op::Make(0), Some(error));
                }
                let inner = &enum_args(input)?[0];
                let value = payload.unwrap();
                let cond = self.standard_enum_op(inner, Op::Test(0), Some(value))?;
                self.branch_enum_value(
                    cond,
                    output,
                    |this| {
                        let member = this.standard_enum_op(inner, Op::Read(0), Some(value))?;
                        let ok = this.standard_enum_op(result, Op::Make(0), Some(member))?;
                        this.standard_enum_op(output, Op::Make(0), Some(ok))
                    },
                    |this| this.standard_enum_op(output, Op::Make(1), None),
                )
            }
            _ => Err(MirLoweringError::MissingBinding("enum extension")),
        }
    }
}
