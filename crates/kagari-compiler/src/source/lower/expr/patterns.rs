use crate::source::lower::{MirLoweringError, state::FunctionLowerer, support::lower_scalar};
use kagari_abi::representation::ValueType;
use kagari_contract::{operations::BinaryOp, representation::semantic_representation};
use kagari_hir::{
    hir::{
        expr::MatchArmBuffer,
        ids::{ExprId, LocalId as HirLocalId, PatternId},
        pattern::PatternKind,
    },
    types::{TypeId, semantic::lower_type},
};
use kagari_mir::{
    ids::BlockId,
    instruction::{Constant, Instruction, MirValue, Terminator},
};
use kagari_types::scalar::BuiltinType;
use std::collections::HashMap;

/// A checked leaf value staged outside lexical locals until extraction succeeds.
pub(crate) struct PatternBinding {
    local: HirLocalId,
    name: String,
    value: MirValue,
}

impl FunctionLowerer<'_, '_> {
    pub(super) fn lower_match(
        &mut self,
        expr_id: ExprId,
        scrutinee: ExprId,
        arms: MatchArmBuffer,
    ) -> Result<MirValue, MirLoweringError> {
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
            .ok_or(MirLoweringError::MissingExprType(scrutinee))?;

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
                Some(next_decision),
                &mut bindings,
            )?;
            self.set_terminator(Terminator::Jump(arm_block));
            self.switch_to_block(arm_block);
            self.publish_pattern_bindings(bindings)?;
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
        pattern: PatternId,
        value: MirValue,
        expected: &TypeId,
        fail: Option<BlockId>,
        bindings: &mut Vec<PatternBinding>,
    ) -> Result<(), MirLoweringError> {
        if fail.is_none()
            && !self
                .analyzed
                .names
                .pattern_is_irrefutable(&self.analyzed.lowered.module, pattern)
        {
            return Err(MirLoweringError::MissingBinding(
                "irrefutable pattern without failure continuation",
            ));
        }
        let semantic = self.semantic_type(expected)?;
        self.function
            .semantic
            .registers
            .entry(value.temp.index())
            .or_insert(semantic);
        if self
            .analyzed
            .typed
            .type_table
            .pattern_variant(pattern)
            .is_some()
        {
            let fail = fail.ok_or(MirLoweringError::MissingBinding(
                "enum pattern failure continuation",
            ))?;
            return self.lower_enum_pattern(pattern, value, expected, fail, bindings);
        }
        match &self.analyzed.lowered.module.pattern(pattern).kind {
            PatternKind::Wildcard => {}
            PatternKind::Or(alternatives) => {
                self.lower_or_pattern(alternatives.clone(), value, expected, fail, bindings)?;
            }
            PatternKind::Range { inclusive, .. } => {
                let fail = fail.ok_or(MirLoweringError::MissingBinding(
                    "range pattern failure continuation",
                ))?;
                let (start, end) = self
                    .analyzed
                    .typed
                    .type_table
                    .pattern_range(pattern)
                    .cloned()
                    .ok_or(MirLoweringError::MissingBinding("checked range pattern"))?;
                let start_ty = semantic_representation(&lower_type(&start.ty()));
                let end_ty = semantic_representation(&lower_type(&end.ty()));
                let start = self.lower_constant(lower_scalar(start), start_ty);
                let end = self.lower_constant(lower_scalar(end), end_ty);
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
            PatternKind::Name { local, name } => bindings.push(PatternBinding {
                local: *local,
                name: name.clone(),
                value,
            }),
            PatternKind::Literal(_) => {
                let fail = fail.ok_or(MirLoweringError::MissingBinding(
                    "literal pattern failure continuation",
                ))?;
                let scalar = self
                    .analyzed
                    .typed
                    .type_table
                    .pattern_scalar_value(pattern)
                    .cloned()
                    .ok_or(MirLoweringError::MissingBinding("checked pattern literal"))?;
                let ty = semantic_representation(&lower_type(&scalar.ty()));
                let literal = self.lower_constant(lower_scalar(scalar), ty);
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
                if elements.is_empty() && *expected == TypeId::Builtin(BuiltinType::Unit) {
                    return Ok(());
                }
                let TypeId::Tuple(types) = expected else {
                    return Err(MirLoweringError::MissingBinding("checked tuple pattern"));
                };
                let elements = elements.clone();
                let types = types.clone();
                if elements.len() != types.len() {
                    return Err(MirLoweringError::MissingBinding(
                        "checked tuple pattern arity",
                    ));
                }
                for (index, (element, ty)) in elements.into_iter().zip(types.iter()).enumerate() {
                    if !self.pattern_requires_value(element) {
                        continue;
                    }
                    let index = i32::try_from(index)
                        .map_err(|_| MirLoweringError::MissingBinding("tuple pattern index"))?;
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
                    .ok_or(MirLoweringError::MissingBinding("checked struct pattern"))?
                    .to_vec();
                if fields.len() != resolved.len() {
                    return Err(MirLoweringError::MissingBinding(
                        "checked struct pattern fields",
                    ));
                }
                for (field, declaration) in fields.into_iter().zip(resolved.iter()) {
                    if !self.pattern_requires_value(field.pattern) {
                        continue;
                    }
                    let field_ref = self.aggregate_field_ref(declaration, expected)?;
                    let signature = self.analyzed.aggregates.field(declaration).ok_or(
                        MirLoweringError::MissingBinding("struct pattern field signature"),
                    )?;
                    let TypeId::Struct(owner) = expected else {
                        return Err(MirLoweringError::MissingBinding(
                            "checked struct pattern type",
                        ));
                    };
                    let structure = self
                        .planner
                        .catalog
                        .structure(&owner.declaration)
                        .ok_or(MirLoweringError::MissingBinding("struct pattern layout"))?;
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
            PatternKind::EnumVariant { .. } => {
                return Err(MirLoweringError::MissingBinding("checked enum variant"));
            }
        }
        Ok(())
    }

    fn lower_or_pattern(
        &mut self,
        mut alternatives: Vec<PatternId>,
        value: MirValue,
        expected: &TypeId,
        fail: Option<BlockId>,
        bindings: &mut Vec<PatternBinding>,
    ) -> Result<(), MirLoweringError> {
        if alternatives.is_empty() {
            return Err(MirLoweringError::MissingBinding(
                "checked pattern alternatives",
            ));
        }
        let irrefutable = alternatives.iter().position(|alternative| {
            self.analyzed
                .names
                .pattern_is_irrefutable(&self.analyzed.lowered.module, *alternative)
        });
        if let Some(index) = irrefutable {
            alternatives.truncate(index + 1);
        } else if fail.is_none() {
            return Err(MirLoweringError::MissingBinding(
                "alternative pattern failure continuation",
            ));
        }

        let success = self.new_block();
        let mut canonical = HashMap::<String, MirValue>::new();
        for (index, alternative) in alternatives.iter().copied().enumerate() {
            let last = index + 1 == alternatives.len();
            let next = if last {
                if irrefutable.is_some() { None } else { fail }
            } else {
                Some(self.new_block())
            };
            let mut branch_bindings = Vec::new();
            self.lower_pattern_decision(alternative, value, expected, next, &mut branch_bindings)?;

            if index > 0 && branch_bindings.len() != canonical.len() {
                return Err(MirLoweringError::MissingBinding(
                    "checked alternative bindings",
                ));
            }
            for PatternBinding { local, name, value } in branch_bindings {
                let merged = if let Some(merged) = canonical.get(&name) {
                    *merged
                } else {
                    if index > 0 {
                        return Err(MirLoweringError::MissingBinding(
                            "checked alternative binding",
                        ));
                    }

                    let ty = self
                        .analyzed
                        .typed
                        .type_table
                        .local_type(local)
                        .ok_or(MirLoweringError::MissingLocalType(local))?;
                    let semantic = self.semantic_type(&ty)?;
                    let merged = self.alloc_temp(value.ty);
                    self.function
                        .semantic
                        .registers
                        .insert(merged.temp.index(), semantic);
                    canonical.insert(name.clone(), merged);
                    bindings.push(PatternBinding {
                        local,
                        name,
                        value: merged,
                    });
                    merged
                };
                self.emit(Instruction::Move {
                    dst: merged,
                    src: value,
                });
            }
            self.set_terminator(Terminator::Jump(success));
            if !last {
                self.switch_to_block(
                    next.ok_or(MirLoweringError::MissingBinding("alternative continuation"))?,
                );
            }
        }
        self.switch_to_block(success);
        Ok(())
    }

    /// Ignored children need no projection, but checked names that denote variants
    /// still require a decision. Static field/access validation remains HIR-owned.
    fn pattern_requires_value(&self, pattern: PatternId) -> bool {
        if self
            .analyzed
            .typed
            .type_table
            .pattern_variant(pattern)
            .is_some()
        {
            return true;
        }
        match &self.analyzed.lowered.module.pattern(pattern).kind {
            PatternKind::Wildcard => false,
            PatternKind::Tuple(elements) => elements
                .iter()
                .any(|element| self.pattern_requires_value(*element)),
            PatternKind::Struct { fields, .. } => fields
                .iter()
                .any(|field| self.pattern_requires_value(field.pattern)),
            PatternKind::Or(alternatives) => {
                for alternative in alternatives {
                    if self.pattern_requires_value(*alternative) {
                        return true;
                    }
                    if self
                        .analyzed
                        .names
                        .pattern_is_irrefutable(&self.analyzed.lowered.module, *alternative)
                    {
                        break;
                    }
                }
                false
            }
            PatternKind::Name { .. }
            | PatternKind::Literal(_)
            | PatternKind::Range { .. }
            | PatternKind::EnumVariant { .. } => true,
        }
    }

    pub(crate) fn publish_declaration_bindings(
        &mut self,
        pattern: PatternId,
        bindings: Vec<PatternBinding>,
    ) -> Result<(), MirLoweringError> {
        let mut values = bindings
            .into_iter()
            .map(|binding| (binding.local, binding.value))
            .collect::<HashMap<_, _>>();
        let bindings = self
            .analyzed
            .lowered
            .module
            .body
            .pattern_bindings(pattern)
            .into_iter()
            .map(|(local, name)| {
                let value = values
                    .remove(&local)
                    .ok_or(MirLoweringError::MissingBinding(
                        "checked declaration binding value",
                    ))?;
                Ok(PatternBinding {
                    local,
                    name: name.to_owned(),
                    value,
                })
            })
            .collect::<Result<Vec<_>, MirLoweringError>>()?;
        if !values.is_empty() {
            return Err(MirLoweringError::MissingBinding(
                "checked declaration bindings",
            ));
        }
        self.publish_pattern_bindings(bindings)
    }

    /// Finish extraction before allocating capture cells or making any new debug
    /// binding visible. Or alternatives have already merged their raw leaf values.
    pub(crate) fn publish_pattern_bindings(
        &mut self,
        bindings: Vec<PatternBinding>,
    ) -> Result<(), MirLoweringError> {
        let mut staged = Vec::with_capacity(bindings.len());
        for PatternBinding {
            local: hir_local,
            name,
            value,
        } in bindings
        {
            let local = self.bind_local(hir_local, name)?;
            let value = if self.cell_locals.contains(&hir_local) {
                let cell = self.alloc_temp(ValueType::HeapObject);
                self.emit(Instruction::MakeCell { dst: cell, value });
                cell
            } else {
                value
            };
            staged.push((local, value));
        }
        for (local, value) in &staged {
            self.emit(Instruction::StoreLocal {
                local: *local,
                src: *value,
            });
        }
        for (local, _) in staged {
            self.introduce_debug_local(local);
        }
        Ok(())
    }

    fn lower_enum_pattern(
        &mut self,
        pattern: PatternId,
        value: MirValue,
        expected: &TypeId,
        fail: BlockId,
        bindings: &mut Vec<PatternBinding>,
    ) -> Result<(), MirLoweringError> {
        let fields = match &self.analyzed.lowered.module.pattern(pattern).kind {
            PatternKind::EnumVariant { fields, .. } => fields.clone(),
            PatternKind::Name { .. } => Vec::new(),
            _ => return Err(MirLoweringError::MissingBinding("checked enum pattern")),
        };
        let variant = self
            .analyzed
            .typed
            .type_table
            .pattern_variant(pattern)
            .ok_or(MirLoweringError::MissingBinding("checked enum variant"))?;
        let signature = self
            .planner
            .catalog
            .variant(variant)
            .ok_or(MirLoweringError::MissingBinding("enum variant signature"))?;
        let enumeration = self
            .planner
            .catalog
            .enumeration(&signature.owner)
            .ok_or(MirLoweringError::MissingBinding("enum pattern layout"))?;
        let arguments = match expected {
            TypeId::Enum(owner) if owner.declaration == enumeration.id => &owner.arguments,
            _ => {
                return Err(MirLoweringError::MissingBinding(
                    "checked enum pattern type",
                ));
            }
        };
        if arguments.len() != enumeration.generic_params.len() {
            return Err(MirLoweringError::MissingBinding(
                "checked enum type arguments",
            ));
        }
        let substitution = enumeration
            .generic_params
            .iter()
            .cloned()
            .zip(arguments.iter().cloned())
            .collect();
        let payload = signature
            .payload
            .iter()
            .map(|ty| ty.instantiate(&substitution))
            .collect::<Vec<_>>();
        let slot = signature.slot;
        if fields.len() != payload.len() {
            return Err(MirLoweringError::MissingBinding(
                "checked enum pattern arity",
            ));
        }
        let cond = self.test_enum_variant(expected, value, slot)?;
        let next = self.new_block();
        self.set_terminator(Terminator::Branch {
            cond,
            then_block: next,
            else_block: fail,
        });
        self.switch_to_block(next);
        for (index, (field, ty)) in fields.into_iter().zip(payload.iter()).enumerate() {
            if !self.pattern_requires_value(field) {
                continue;
            }
            let member = self.read_enum_field(expected, value, slot, index)?;
            self.lower_pattern_decision(field, member, ty, Some(fail), bindings)?;
        }
        Ok(())
    }
}
