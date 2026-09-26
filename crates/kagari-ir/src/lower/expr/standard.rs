use super::*;

impl FunctionLowerer<'_, '_> {
    pub(crate) fn standard_enum_op(
        &mut self,
        ty: &kagari_hir::types::TypeId,
        op: crate::module::instruction::StandardEnumOp,
        value: Option<IrValue>,
    ) -> Result<IrValue, IrLoweringError> {
        let concrete = self.planner.arguments(
            std::slice::from_ref(ty),
            &self.instance.substitution,
            self.function.debug.source_span,
        )?;
        let ty = crate::module::abi::AbiType::from_checked_type(&concrete[0]);
        let (_, output) = op
            .contract(&ty)
            .ok_or(IrLoweringError::MissingBinding("standard enum contract"))?;
        let dst = self.alloc_temp(output);
        self.emit(Instruction::StandardEnum { dst, value, ty, op });
        Ok(dst)
    }
}

impl FunctionLowerer<'_, '_> {
    pub(super) fn lower_standard_combinator(
        &mut self,
        site: hir::ExprId,
        intrinsic: kagari_hir::builtin::surface::StandardIntrinsic,
        input: &kagari_hir::types::TypeId,
        args: &[IrValue],
    ) -> Result<IrValue, IrLoweringError> {
        use crate::module::instruction::StandardEnumOp;
        use kagari_hir::builtin::surface::{StandardEnum, StandardIntrinsic::*};
        use kagari_hir::types::TypeId;
        let TypeId::StandardEnum {
            kind,
            args: input_args,
        } = input
        else {
            return Err(IrLoweringError::MissingBinding("standard enum input"));
        };
        let output = self
            .analyzed
            .typed
            .type_table
            .expr_type(site)
            .ok_or(IrLoweringError::MissingExprType(site))?;
        let TypeId::StandardEnum {
            args: output_args, ..
        } = &output
        else {
            return Err(IrLoweringError::MissingBinding("standard enum output"));
        };
        let cond = self.standard_enum_op(input, StandardEnumOp::Test(0), Some(args[0]))?;
        let success = self.new_block();
        let failure = self.new_block();
        let join = self.new_block();
        let dst = self.alloc_temp(ValueType::HeapObject);
        self.set_terminator(Terminator::Branch {
            cond,
            then_block: success,
            else_block: failure,
        });
        for (variant, block) in [(0, success), (1, failure)] {
            self.switch_to_block(block);
            let payload = if variant == 0 || *kind == StandardEnum::Result {
                Some(self.standard_enum_op(input, StandardEnumOp::Read(variant), Some(args[0]))?)
            } else {
                None
            };
            let invokes = match intrinsic {
                OptionMap | OptionAndThen | ResultMap | ResultAndThen => variant == 0,
                ResultMapErr | OptionOkOrElse => variant == 1,
                _ => false,
            };
            let next = if invokes {
                let callback_result = if matches!(intrinsic, OptionAndThen | ResultAndThen) {
                    output.clone()
                } else {
                    output_args[usize::from(matches!(intrinsic, ResultMapErr | OptionOkOrElse))]
                        .clone()
                };
                let span = self.analyzed.lowered.source_map.expr_span(site);
                let return_type =
                    self.planner
                        .value_type(&callback_result, &self.instance.substitution, span)?;
                let params = if intrinsic == OptionOkOrElse {
                    vec![]
                } else {
                    vec![self.planner.value_type(
                        &input_args[variant as usize],
                        &self.instance.substitution,
                        span,
                    )?]
                };
                let value = self.alloc_temp(return_type);
                self.emit(Instruction::Call {
                    dst: Some(value),
                    callee: CallTarget::Closure {
                        value: args[1],
                        params,
                        return_type,
                    },
                    args: payload.into_iter().collect(),
                });
                if matches!(intrinsic, OptionAndThen | ResultAndThen) {
                    value
                } else if intrinsic == ResultMapErr {
                    let concrete = self
                        .planner
                        .arguments(
                            std::slice::from_ref(&output),
                            &self.instance.substitution,
                            span,
                        )?
                        .remove(0);
                    let mapped = self.alloc_temp(ValueType::HeapObject);
                    self.emit(Instruction::MapResultError {
                        dst: mapped,
                        original: args[0],
                        error: value,
                        ty: crate::module::abi::AbiType::from_checked_type(&concrete),
                    });
                    mapped
                } else {
                    self.standard_enum_op(&output, StandardEnumOp::Make(variant), Some(value))?
                }
            } else if matches!(intrinsic, OptionOkOr | OptionOkOrElse) {
                self.standard_enum_op(
                    &output,
                    StandardEnumOp::Make(variant),
                    if variant == 0 { payload } else { Some(args[1]) },
                )?
            } else {
                // The untouched variant has identical payload semantics even when the success type changes.
                args[0]
            };
            self.emit(Instruction::Move { dst, src: next });
            self.set_terminator(Terminator::Jump(join));
        }
        self.switch_to_block(join);
        Ok(dst)
    }
}
