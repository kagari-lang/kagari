use super::*;
use kagari_hir::{builtin::traits::StandardTrait, types::TypeId};

impl FunctionLowerer<'_, '_> {
    pub(super) fn lower_selected_operator(
        &mut self,
        site: hir::ExprId,
        args: &[IrValue],
    ) -> Result<IrValue, IrLoweringError> {
        let call = self
            .analyzed
            .typed
            .type_table
            .call_resolution(site)
            .ok_or(IrLoweringError::MissingBinding("operator contract"))?;
        let kagari_hir::typeck::CallTarget::TraitMethod { method, interface } = call.target else {
            return Err(IrLoweringError::MissingBinding("operator method"));
        };
        let receiver = call
            .receiver
            .ok_or(IrLoweringError::MissingBinding("operator receiver"))?;
        let receiver = self
            .analyzed
            .typed
            .type_table
            .expr_type(receiver)
            .ok_or(IrLoweringError::MissingExprType(receiver))?;
        let ty = self
            .planner
            .arguments(
                &[receiver],
                &self.instance.substitution,
                self.function.debug.source_span,
            )?
            .remove(0);
        let TypeId::Trait(interface) = self
            .planner
            .arguments(
                &[TypeId::Trait(interface)],
                &self.instance.substitution,
                self.function.debug.source_span,
            )?
            .remove(0)
        else {
            unreachable!()
        };
        let contract = self
            .planner
            .catalog
            .trait_(&interface.declaration)
            .ok_or(IrLoweringError::MissingBinding("operator trait"))?;
        let signature = self
            .planner
            .catalog
            .trait_method(&method)
            .ok_or(IrLoweringError::MissingBinding("operator signature"))?;
        let mut substitution: kagari_hir::types::TypeSubstitution = contract
            .generic_params
            .iter()
            .cloned()
            .zip(interface.arguments.iter().cloned())
            .collect();
        substitution.insert_receiver(contract.id.clone(), ty.clone());
        let result = self.planner.catalog.normalize_type(
            &signature
                .return_type
                .with_self(&contract.id, &ty)
                .instantiate(&substitution)
                .with_associated_types(&interface),
        );
        let result_ty = self.value_type(&result)?;
        let callee = if let Some((declaration, arguments)) = self
            .planner
            .catalog
            .implementation_method(&method, &interface, &ty)
        {
            if declaration.module == *self.planner.owner().lowered.source.module_identity() {
                CallTarget::Function(self.planner.enqueue_declaration(
                    &declaration,
                    arguments,
                    self.function.debug.source_span,
                )?)
            } else {
                CallTarget::SourceFunction(Box::new(
                    crate::module::instruction::SourceFunctionContract {
                        declaration,
                        arguments,
                        params: args.iter().map(|a| a.ty).collect(),
                        return_type: result_ty,
                    },
                ))
            }
        } else {
            if let Some(protocol) = StandardTrait::from_id(&interface.declaration)
                && protocol.binary_operator()
            {
                let op = match protocol {
                    StandardTrait::Add => BinaryOp::Add,
                    StandardTrait::Sub => BinaryOp::Sub,
                    StandardTrait::Mul => BinaryOp::Mul,
                    StandardTrait::Div => BinaryOp::Div,
                    StandardTrait::Rem => BinaryOp::Rem,
                    _ => unreachable!(),
                };
                let dst = self.alloc_temp(result_ty);
                self.emit(Instruction::Binary {
                    dst,
                    op,
                    lhs: args[0],
                    rhs: args[1],
                });
                return Ok(dst);
            }
            let intrinsic = match StandardTrait::from_id(&interface.declaration) {
                Some(StandardTrait::PartialOrd) => StandardIntrinsic::ValuePartialCmp,
                Some(StandardTrait::Ord) => StandardIntrinsic::ValueCmp,
                _ => return Err(IrLoweringError::MissingBinding("builtin operator")),
            };
            CallTarget::StandardIntrinsic(intrinsic)
        };
        let dst = self.alloc_temp(result_ty);
        self.emit(Instruction::Call {
            dst: Some(dst),
            callee,
            args: args.iter().copied().collect(),
        });
        Ok(dst)
    }

    pub(super) fn lower_ordering_operator(
        &mut self,
        site: hir::ExprId,
        op: hir::BinaryOp,
        args: &[IrValue],
    ) -> Result<IrValue, IrLoweringError> {
        // Primitive numeric comparisons retain their direct instruction path.
        if matches!(
            args[0].ty,
            ValueType::I32 | ValueType::I64 | ValueType::F32 | ValueType::F64
        ) {
            let dst = self.alloc_temp(ValueType::Bool);
            let op = match op {
                hir::BinaryOp::Lt => BinaryOp::Lt,
                hir::BinaryOp::Le => BinaryOp::Le,
                hir::BinaryOp::Gt => BinaryOp::Gt,
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
        use crate::module::instruction::StandardEnumOp;
        let value = self.lower_selected_operator(site, args)?;
        let optional = kagari_hir::builtin::traits::ordering_type(true);
        let ordering = kagari_hir::builtin::traits::ordering_type(false);
        let some = self.standard_enum_op(&optional, StandardEnumOp::Test(0), Some(value))?;
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
        let order = self.standard_enum_op(&optional, StandardEnumOp::Read(0), Some(value))?;
        let tag = if matches!(op, hir::BinaryOp::Lt | hir::BinaryOp::Ge) {
            0
        } else {
            2
        };
        let test = self.standard_enum_op(&ordering, StandardEnumOp::Test(tag), Some(order))?;
        if matches!(op, hir::BinaryOp::Le | hir::BinaryOp::Ge) {
            self.emit(Instruction::Unary {
                dst,
                op: crate::module::instruction::UnaryOp::Not,
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
