use crate::source::elaborate::model::{CheckedLocal, CheckedMatchArm, CheckedPatternKind};
use crate::source::lower::{MirLoweringError, state::FunctionLowerer, support::lower_scalar};
use kagari_abi::representation::ValueType;
use kagari_common::identity::table::DefinitionId;
use kagari_contract::{operations::BinaryOp, representation::semantic_representation};
use kagari_hir::{
    hir::ids::{ExprId, PatternId},
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
    binding: CheckedLocal,
    value: MirValue,
}

impl FunctionLowerer<'_, '_> {
    pub(super) fn lower_match(
        &mut self,
        expr_id: ExprId,
        scrutinee: ExprId,
        arms: Vec<CheckedMatchArm>,
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
            .expression_type(scrutinee)
            .ok_or(MirLoweringError::MissingExprType(scrutinee))?;

        for arm in arms {
            let outer_scope = self.current_scope;
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
            let arm_value = self.lower_expr(arm.value.ok_or(
                MirLoweringError::MissingBinding("reachable match arm value"),
            )?)?;
            if !self.current_block_terminated() {
                self.emit(Instruction::Move {
                    dst: result,
                    src: arm_value,
                });
                self.set_terminator(Terminator::Jump(exit_block));
            }

            self.current_scope = outer_scope;

            decision_block = next_decision;
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
        if fail.is_none() && !self.source_body().patterns[&pattern].irrefutable {
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
        match self.source_body().patterns[&pattern].kind.clone() {
            CheckedPatternKind::Variant {
                declaration,
                fields,
            } => {
                let fail = fail.ok_or(MirLoweringError::MissingBinding(
                    "enum pattern failure continuation",
                ))?;
                return self.lower_enum_pattern(
                    declaration,
                    fields,
                    value,
                    expected,
                    fail,
                    bindings,
                );
            }
            CheckedPatternKind::Wildcard => {}
            CheckedPatternKind::Or(alternatives) => {
                self.lower_or_pattern(alternatives.clone(), value, expected, fail, bindings)?;
            }
            CheckedPatternKind::Range {
                start,
                end,
                inclusive,
            } => {
                let fail = fail.ok_or(MirLoweringError::MissingBinding(
                    "range pattern failure continuation",
                ))?;
                let start_ty = semantic_representation(&lower_type(&start.ty()));
                let end_ty = semantic_representation(&lower_type(&end.ty()));
                let start = self.lower_constant(lower_scalar(start), start_ty);
                let end = self.lower_constant(lower_scalar(end), end_ty);
                for (op, bound) in [
                    (BinaryOp::Ge, start),
                    (
                        if inclusive {
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
            CheckedPatternKind::Binding(binding) => {
                bindings.push(PatternBinding { binding, value })
            }
            CheckedPatternKind::Constant(scalar) => {
                let fail = fail.ok_or(MirLoweringError::MissingBinding(
                    "literal pattern failure continuation",
                ))?;
                let semantic = self.semantic_type(&scalar.ty())?;
                let literal =
                    self.lower_constant(lower_scalar(scalar), semantic_representation(&semantic));
                // Pattern constants bypass expression lowering, but still need the
                // checked domain: physical I64 alone cannot distinguish u8 from i64.
                self.function
                    .semantic
                    .registers
                    .insert(literal.temp.index(), semantic);
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
            CheckedPatternKind::Tuple(elements) => {
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
            CheckedPatternKind::Struct(fields) => {
                for (declaration, pattern) in fields {
                    if !self.pattern_requires_value(pattern) {
                        continue;
                    }
                    let field_ref = self.aggregate_field_ref(&declaration, expected)?;
                    let signature = self.environment.aggregates().field(&declaration).ok_or(
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
                    self.lower_pattern_decision(pattern, member, &ty, fail, bindings)?;
                }
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
        let irrefutable = alternatives
            .iter()
            .position(|alternative| self.source_body().patterns[alternative].irrefutable);
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
            for PatternBinding { binding, value } in branch_bindings {
                let merged = if let Some(merged) = canonical.get(&binding.name) {
                    *merged
                } else {
                    if index > 0 {
                        return Err(MirLoweringError::MissingBinding(
                            "checked alternative binding",
                        ));
                    }

                    let semantic = self.semantic_type(&binding.ty)?;
                    let merged = self.alloc_temp(value.ty);
                    self.function
                        .semantic
                        .registers
                        .insert(merged.temp.index(), semantic);
                    canonical.insert(binding.name.clone(), merged);
                    bindings.push(PatternBinding {
                        binding,
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
        match &self.source_body().patterns[&pattern].kind {
            CheckedPatternKind::Wildcard => false,
            CheckedPatternKind::Tuple(elements) => elements
                .iter()
                .any(|element| self.pattern_requires_value(*element)),
            CheckedPatternKind::Struct(fields) => fields
                .iter()
                .any(|(_, pattern)| self.pattern_requires_value(*pattern)),
            CheckedPatternKind::Or(alternatives) => {
                for alternative in alternatives {
                    if self.pattern_requires_value(*alternative) {
                        return true;
                    }
                    if self.source_body().patterns[alternative].irrefutable {
                        break;
                    }
                }
                false
            }
            CheckedPatternKind::Binding(_)
            | CheckedPatternKind::Constant(_)
            | CheckedPatternKind::Range { .. }
            | CheckedPatternKind::Variant { .. } => true,
        }
    }

    pub(crate) fn publish_declaration_bindings(
        &mut self,
        pattern: PatternId,
        bindings: Vec<PatternBinding>,
    ) -> Result<(), MirLoweringError> {
        let mut values = bindings
            .into_iter()
            .map(|binding| (binding.binding.id, binding.value))
            .collect::<HashMap<_, _>>();
        let mut ordered = Vec::new();
        let mut pending = vec![pattern];
        while let Some(pattern) = pending.pop() {
            match &self.source_body().patterns[&pattern].kind {
                CheckedPatternKind::Binding(binding) => {
                    let value =
                        values
                            .remove(&binding.id)
                            .ok_or(MirLoweringError::MissingBinding(
                                "checked declaration binding value",
                            ))?;
                    ordered.push(PatternBinding {
                        binding: binding.clone(),
                        value,
                    });
                }
                CheckedPatternKind::Or(alternatives) => {
                    pending.extend(alternatives.first().copied())
                }
                CheckedPatternKind::Tuple(fields) | CheckedPatternKind::Variant { fields, .. } => {
                    pending.extend(fields.iter().rev().copied())
                }
                CheckedPatternKind::Struct(fields) => {
                    pending.extend(fields.iter().rev().map(|(_, pattern)| *pattern))
                }
                CheckedPatternKind::Wildcard
                | CheckedPatternKind::Constant(_)
                | CheckedPatternKind::Range { .. } => {}
            }
        }
        if !values.is_empty() {
            return Err(MirLoweringError::MissingBinding(
                "checked declaration bindings",
            ));
        }
        self.publish_pattern_bindings(ordered)
    }

    /// Finish extraction before allocating capture cells or making any new debug
    /// binding visible. Or alternatives have already merged their raw leaf values.
    pub(crate) fn publish_pattern_bindings(
        &mut self,
        bindings: Vec<PatternBinding>,
    ) -> Result<(), MirLoweringError> {
        let mut staged = Vec::with_capacity(bindings.len());
        for PatternBinding { binding, value } in bindings {
            let local = self.bind_local(&binding)?;
            let value = if self.cell_locals.contains(&binding.id) {
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
        declaration: DefinitionId,
        fields: Vec<PatternId>,
        value: MirValue,
        expected: &TypeId,
        fail: BlockId,
        bindings: &mut Vec<PatternBinding>,
    ) -> Result<(), MirLoweringError> {
        let signature = self
            .planner
            .catalog
            .variant(&declaration)
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
