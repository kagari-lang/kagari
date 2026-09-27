use crate::lower::IrLoweringError;
use crate::lower::state::FunctionLowerer;
use crate::module::abi::AbiType;
use crate::module::abi::NominalAbiType;
use crate::module::ids::BlockId;
use crate::module::ids::LocalId;
use crate::module::instruction::BinaryOp;
use crate::module::instruction::Constant;
use crate::module::instruction::Instruction;
use crate::module::instruction::IrValue;
use crate::module::instruction::StandardEnumOp;
use crate::module::instruction::Terminator;
use kagari_abi::representation::ValueType;
use kagari_hir::hir;
use kagari_hir::hir::PatternKind;
use kagari_hir::types::NominalType;
use kagari_hir::types::TypeId;
use std::collections::HashMap;

impl FunctionLowerer<'_, '_> {
    pub(super) fn lower_match(
        &mut self,
        expr_id: hir::ExprId,
        scrutinee: hir::ExprId,
        arms: hir::MatchArmBuffer,
    ) -> Result<IrValue, IrLoweringError> {
        let scrutinee_temp = self.lower_expr(scrutinee)?;
        if self.current_block_terminated() {
            return Ok(scrutinee_temp);
        }
        let result = self.alloc_temp(self.expr_type(expr_id)?);
        let exit_block = self.new_block();
        let fail_block = self.new_block();
        let mut decision_block = self.current_block;
        let scrutinee_ty = self
            .analyzed
            .typed
            .type_table
            .expr_type(scrutinee)
            .ok_or(IrLoweringError::MissingExprType(scrutinee))?;

        for arm in arms {
            let outer_scope = self.current_scope;
            let irrefutable = self
                .analyzed
                .names
                .pattern_is_irrefutable(&self.analyzed.lowered.module, arm.pattern);
            let arm_block = self.new_block();
            let next_decision = self.new_block();

            self.switch_to_block(decision_block);
            let mut bindings = Vec::new();
            self.lower_pattern_decision(
                arm.pattern,
                scrutinee_temp,
                &scrutinee_ty,
                next_decision,
                &mut bindings,
            )?;
            self.set_terminator(Terminator::Jump(arm_block));
            self.switch_to_block(arm_block);
            for (local, value) in bindings {
                self.emit(Instruction::StoreLocal { local, src: value });
                self.introduce_debug_local(local);
            }
            if let Some(guard) = arm.guard {
                let predicate = self.lower_expr(guard)?;
                if !self.current_block_terminated() {
                    let body_block = self.new_block();
                    self.set_terminator(Terminator::Branch {
                        cond: predicate,
                        then_block: body_block,
                        else_block: next_decision,
                    });
                    self.switch_to_block(body_block);
                }
            }
            if self.current_block_terminated() {
                self.current_scope = outer_scope;
                decision_block = next_decision;
                continue;
            }
            let arm_value = self.lower_expr(arm.expr)?;
            if !self.current_block_terminated() {
                self.emit(Instruction::Move {
                    dst: result,
                    src: arm_value,
                });
                self.set_terminator(Terminator::Jump(exit_block));
            }

            self.current_scope = outer_scope;

            decision_block = next_decision;
            if irrefutable && arm.guard.is_none() {
                break;
            }
        }

        self.switch_to_block(decision_block);
        self.set_terminator(Terminator::Jump(fail_block));

        self.switch_to_block(fail_block);
        self.set_terminator(Terminator::Unreachable);

        self.switch_to_join(exit_block);
        Ok(result)
    }

    pub(crate) fn lower_pattern_decision(
        &mut self,
        pattern: hir::PatternId,
        value: IrValue,
        expected: &TypeId,
        fail: BlockId,
        bindings: &mut Vec<(LocalId, IrValue)>,
    ) -> Result<(), IrLoweringError> {
        if let Some(variant) = self.analyzed.typed.type_table.standard_pattern(pattern) {
            let cond = self.standard_enum_op(
                expected,
                StandardEnumOp::Test(variant.index() as u32),
                Some(value),
            )?;
            let next = self.new_block();
            self.set_terminator(Terminator::Branch {
                cond,
                then_block: next,
                else_block: fail,
            });
            self.switch_to_block(next);
            if let Some(index) = variant.payload() {
                let TypeId::StandardEnum { args, .. } = expected else {
                    return Err(IrLoweringError::MissingBinding("standard pattern type"));
                };
                let PatternKind::EnumVariant { fields, .. } =
                    &self.analyzed.lowered.module.pattern(pattern).kind
                else {
                    return Err(IrLoweringError::MissingBinding("standard payload pattern"));
                };
                let field = fields[0];
                let payload = self.standard_enum_op(
                    expected,
                    StandardEnumOp::Read(variant.index() as u32),
                    Some(value),
                )?;
                self.lower_pattern_decision(field, payload, &args[index], fail, bindings)?;
            }
            return Ok(());
        }
        match &self.analyzed.lowered.module.pattern(pattern).kind {
            PatternKind::Wildcard => {}
            PatternKind::Or(alternatives) => {
                let alternatives = alternatives.clone();
                let success = self.new_block();
                let mut canonical = HashMap::<String, IrValue>::new();
                for (index, alternative) in alternatives.iter().copied().enumerate() {
                    let locals_before = self.function.locals.len();
                    let next = if index + 1 == alternatives.len() {
                        fail
                    } else {
                        self.new_block()
                    };
                    let mut branch_bindings = Vec::new();
                    self.lower_pattern_decision(
                        alternative,
                        value,
                        expected,
                        next,
                        &mut branch_bindings,
                    )?;
                    for (local, value) in branch_bindings {
                        let name = self.function.locals[local.index()].name.clone();
                        let merged = if let Some(merged) = canonical.get(&name) {
                            *merged
                        } else {
                            let merged = self.alloc_temp(value.ty);
                            canonical.insert(name, merged);
                            bindings.push((local, merged));
                            merged
                        };
                        self.emit(Instruction::Move {
                            dst: merged,
                            src: value,
                        });
                    }
                    if index > 0 {
                        self.function.locals.truncate(locals_before);
                        self.function.debug.locals.truncate(locals_before);
                        self.locals.retain(|_, local| local.index() < locals_before);
                    }
                    self.set_terminator(Terminator::Jump(success));
                    if index + 1 < alternatives.len() {
                        self.switch_to_block(next);
                    }
                }
                self.switch_to_block(success);
            }
            PatternKind::Range { inclusive, .. } => {
                let (start, end) = self
                    .analyzed
                    .typed
                    .type_table
                    .pattern_range(pattern)
                    .cloned()
                    .ok_or(IrLoweringError::MissingBinding("checked range pattern"))?;
                let start = self.lower_constant(start.into(), ValueType::I32);
                let end = self.lower_constant(end.into(), ValueType::I32);
                for (op, bound) in [
                    (BinaryOp::Ge, start),
                    (
                        if *inclusive {
                            BinaryOp::Le
                        } else {
                            BinaryOp::Lt
                        },
                        end,
                    ),
                ] {
                    let cond = self.alloc_temp(ValueType::Bool);
                    self.emit(Instruction::Binary {
                        dst: cond,
                        op,
                        lhs: value,
                        rhs: bound,
                    });
                    let next = self.new_block();
                    self.set_terminator(Terminator::Branch {
                        cond,
                        then_block: next,
                        else_block: fail,
                    });
                    self.switch_to_block(next);
                }
            }
            PatternKind::Name { local, name } => {
                let local = *local;
                let name = name.clone();
                let local_ty = self
                    .analyzed
                    .typed
                    .type_table
                    .local_type(local)
                    .as_ref()
                    .map(|ty| self.value_type(ty))
                    .transpose()?
                    .ok_or(IrLoweringError::MissingLocalType(local))?;
                let ir_local = self.alloc_local(
                    name,
                    local_ty,
                    self.analyzed.lowered.source_map.local_span(local),
                );
                let semantic = self.semantic_type(
                    &self
                        .analyzed
                        .typed
                        .type_table
                        .local_type(local)
                        .ok_or(IrLoweringError::MissingLocalType(local))?,
                )?;
                self.function
                    .semantic
                    .locals
                    .insert(ir_local.index(), semantic);
                self.locals.insert(local, ir_local);
                bindings.push((ir_local, value));
            }
            PatternKind::Literal(_) => {
                let scalar = self
                    .analyzed
                    .typed
                    .type_table
                    .pattern_scalar_value(pattern)
                    .cloned()
                    .ok_or(IrLoweringError::MissingBinding("checked pattern literal"))?;
                let ty = AbiType::from_checked_type(&scalar.ty()).representation();
                let literal = self.lower_constant(scalar.into(), ty);
                let cond = self.alloc_temp(ValueType::Bool);
                self.emit(Instruction::Binary {
                    dst: cond,
                    op: BinaryOp::Eq,
                    lhs: value,
                    rhs: literal,
                });
                let next = self.new_block();
                self.set_terminator(Terminator::Branch {
                    cond,
                    then_block: next,
                    else_block: fail,
                });
                self.switch_to_block(next);
            }
            PatternKind::Tuple(elements) => {
                let TypeId::Tuple(types) = expected else {
                    return Err(IrLoweringError::MissingBinding("checked tuple pattern"));
                };
                let elements = elements.clone();
                let types = types.clone();
                if elements.len() != types.len() {
                    return Err(IrLoweringError::MissingBinding(
                        "checked tuple pattern arity",
                    ));
                }
                for (index, (element, ty)) in elements.into_iter().zip(types.iter()).enumerate() {
                    let index = i32::try_from(index)
                        .map_err(|_| IrLoweringError::MissingBinding("tuple pattern index"))?;
                    let index = self.lower_constant(Constant::I32(index), ValueType::I32);
                    let field = self.alloc_temp(self.value_type(ty)?);
                    self.emit(Instruction::ReadAggregateIndex {
                        dst: field,
                        base: value,
                        index,
                    });
                    self.lower_pattern_decision(element, field, ty, fail, bindings)?;
                }
            }
            PatternKind::Struct { fields, .. } => {
                let fields = fields.clone();
                let resolved = self
                    .analyzed
                    .typed
                    .type_table
                    .pattern_fields(pattern)
                    .ok_or(IrLoweringError::MissingBinding("checked struct pattern"))?
                    .to_vec();
                if fields.len() != resolved.len() {
                    return Err(IrLoweringError::MissingBinding(
                        "checked struct pattern fields",
                    ));
                }
                for (field, declaration) in fields.into_iter().zip(resolved.iter()) {
                    let field_ref = self.aggregate_field_ref(declaration, expected)?;
                    let signature = self.analyzed.aggregates.field(declaration).ok_or(
                        IrLoweringError::MissingBinding("struct pattern field signature"),
                    )?;
                    let TypeId::Struct(owner) = expected else {
                        return Err(IrLoweringError::MissingBinding(
                            "checked struct pattern type",
                        ));
                    };
                    let structure = self
                        .planner
                        .catalog
                        .structure(&owner.declaration)
                        .ok_or(IrLoweringError::MissingBinding("struct pattern layout"))?;
                    let substitution = structure
                        .generic_params
                        .iter()
                        .cloned()
                        .zip(owner.arguments.iter().cloned())
                        .collect();
                    let ty = signature.ty.instantiate(&substitution);
                    let member = self.alloc_temp(self.value_type(&ty)?);
                    self.emit(Instruction::ReadAggregateField {
                        dst: member,
                        base: value,
                        field: field_ref,
                    });
                    self.lower_pattern_decision(field.pattern, member, &ty, fail, bindings)?;
                }
            }
            PatternKind::EnumVariant { fields, .. } => {
                let fields = fields.clone();
                let TypeId::Enum(owner) = expected else {
                    return Err(IrLoweringError::MissingBinding("checked enum pattern type"));
                };
                let variant = self
                    .analyzed
                    .typed
                    .type_table
                    .pattern_variant(pattern)
                    .ok_or(IrLoweringError::MissingBinding("checked enum variant"))?;
                let signature = self
                    .planner
                    .catalog
                    .variant(variant)
                    .ok_or(IrLoweringError::MissingBinding("enum variant signature"))?;
                let enumeration = self
                    .planner
                    .catalog
                    .enumeration(&owner.declaration)
                    .ok_or(IrLoweringError::MissingBinding("enum pattern layout"))?;
                let substitution = enumeration
                    .generic_params
                    .iter()
                    .cloned()
                    .zip(owner.arguments.iter().cloned())
                    .collect();
                let payload = signature
                    .payload
                    .iter()
                    .map(|ty| ty.instantiate(&substitution))
                    .collect::<Vec<_>>();
                let slot = signature.slot;
                if fields.len() != payload.len() {
                    return Err(IrLoweringError::MissingBinding(
                        "checked enum pattern arity",
                    ));
                }
                let concrete = NominalType {
                    associated_types: Default::default(),
                    declaration: owner.declaration.clone(),
                    arguments: self.planner.arguments(
                        &owner.arguments,
                        &self.instance.substitution,
                        self.analyzed.lowered.source_map.pattern_span(pattern),
                    )?,
                };
                let enumeration = NominalAbiType::from_checked_type(&concrete);
                let cond = self.alloc_temp(ValueType::Bool);
                self.emit(Instruction::TestEnumVariant {
                    dst: cond,
                    value,
                    enumeration: enumeration.clone(),
                    variant: slot,
                });
                let next = self.new_block();
                self.set_terminator(Terminator::Branch {
                    cond,
                    then_block: next,
                    else_block: fail,
                });
                self.switch_to_block(next);
                for (index, (field, ty)) in fields.into_iter().zip(payload.iter()).enumerate() {
                    let member = self.alloc_temp(self.value_type(ty)?);
                    self.emit(Instruction::ReadEnumPayload {
                        dst: member,
                        value,
                        enumeration: enumeration.clone(),
                        variant: slot,
                        index,
                    });
                    self.lower_pattern_decision(field, member, ty, fail, bindings)?;
                }
            }
        }
        Ok(())
    }
}
