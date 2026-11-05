use crate::source::elaborate::model::IndexRead;
use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_abi::representation::ValueType;
use kagari_common::identity::table::DefinitionId;
use kagari_hir::{hir::ids::ExprId, typeck::table::ResolvedHostPath, types::TypeId};
use kagari_mir::instruction::{Instruction, MirValue, PathRef, StructFieldInit};
use smallvec::SmallVec;
use std::ops::ControlFlow;

impl FunctionLowerer<'_, '_> {
    pub(super) fn lower_tuple(
        &mut self,
        expr_id: ExprId,
        elements: Vec<ExprId>,
    ) -> Result<MirValue, MirLoweringError> {
        let elements = match self.lower_values(&elements)? {
            ControlFlow::Continue(elements) => elements,
            ControlFlow::Break(value) => return Ok(value),
        };
        let dst = self.alloc_temp(self.expr_type(expr_id)?);
        self.emit(Instruction::MakeTuple { dst, elements });
        Ok(dst)
    }

    pub(super) fn lower_array(
        &mut self,
        expr_id: ExprId,
        elements: Vec<ExprId>,
    ) -> Result<MirValue, MirLoweringError> {
        let elements = match self.lower_values(&elements)? {
            ControlFlow::Continue(elements) => elements,
            ControlFlow::Break(value) => return Ok(value),
        };
        let dst = self.alloc_temp(self.expr_type(expr_id)?);
        self.emit(Instruction::MakeArray {
            dst,
            element: self.array_element_type(expr_id)?,
            elements,
        });
        Ok(dst)
    }

    pub(super) fn lower_range(
        &mut self,
        expr_id: ExprId,
        start: Option<ExprId>,
        end: Option<ExprId>,
        _inclusive: bool,
    ) -> Result<MirValue, MirLoweringError> {
        let mut values = Vec::new();
        for expr in start.iter().chain(&end) {
            let value = self.lower_expr(*expr)?;
            if self.current_block_terminated() {
                return Ok(value);
            }
            values.push(value);
        }
        let source = self
            .expression_type(expr_id)
            .ok_or(MirLoweringError::MissingExprType(expr_id))?;
        let ty = self
            .planner
            .arguments(
                &[source],
                &self.instance.substitution,
                self.function.debug.source_span,
            )?
            .remove(0);
        let ty = self.planner.encode_type(&ty);
        let mut values = values.into_iter();
        let first = start.and_then(|_| values.next());
        let last = end.and_then(|_| values.next());
        let dst = self.alloc_temp(ValueType::HeapObject);
        self.emit(Instruction::MakeRange {
            dst,
            start: first,
            end: last,
            ty,
        });
        Ok(dst)
    }

    pub(super) fn lower_struct_init(
        &mut self,
        expr_id: ExprId,
        declaration: DefinitionId,
        fields: Vec<(DefinitionId, ExprId)>,
        base: Option<ExprId>,
        base_fields: Vec<DefinitionId>,
    ) -> Result<MirValue, MirLoweringError> {
        let mut lowered_fields = SmallVec::new();
        for (target, expression) in fields {
            let value = self.lower_expr(expression)?;
            if self.current_block_terminated() {
                return Ok(value);
            }
            lowered_fields.push(StructFieldInit {
                slot: self
                    .planner
                    .catalog
                    .field(&target)
                    .ok_or(MirLoweringError::MissingBinding("checked field contract"))?
                    .slot,
                value,
            });
        }
        if let Some(base_id) = base {
            let base = self.lower_expr(base_id)?;
            if self.current_block_terminated() {
                return Ok(base);
            }
            let receiver = self
                .expression_type(base_id)
                .ok_or(MirLoweringError::MissingExprType(base_id))?;
            let receiver = self
                .planner
                .arguments(
                    &[receiver],
                    &self.instance.substitution,
                    self.expression_span(base_id),
                )?
                .remove(0);
            let TypeId::Struct(owner) = &receiver else {
                return Err(MirLoweringError::MissingBinding(
                    "checked struct update base",
                ));
            };
            let structure = self.planner.catalog.structure(&declaration).ok_or(
                MirLoweringError::MissingBinding("checked struct update layout"),
            )?;
            let substitution = structure
                .generic_params
                .iter()
                .cloned()
                .zip(owner.arguments.iter().cloned())
                .collect();
            for declaration in base_fields {
                let field = self.aggregate_field_ref(&declaration, &receiver)?;
                let signature = self.environment.aggregates().field(&declaration).ok_or(
                    MirLoweringError::MissingBinding("checked struct update field"),
                )?;
                let ty = signature.ty.instantiate(&substitution);
                let value = self.alloc_temp(self.value_type(&ty)?);
                let semantic = self.semantic_type(&ty)?;
                self.function
                    .semantic
                    .registers
                    .insert(value.temp.index(), semantic);
                let slot = field.slot;
                self.emit(Instruction::ReadAggregateField {
                    dst: value,
                    base,
                    field,
                });
                lowered_fields.push(StructFieldInit { slot, value });
            }
        }
        let dst = self.alloc_temp(self.expr_type(expr_id)?);
        self.emit(Instruction::MakeStruct {
            dst,
            structure: self.expr_nominal_instance(expr_id)?,
            fields: lowered_fields,
        });
        Ok(dst)
    }

    pub(super) fn lower_host_path_read(
        &mut self,
        expr_id: ExprId,
        checked: ResolvedHostPath,
    ) -> Result<MirValue, MirLoweringError> {
        let root_or_view = self.lower_expr(checked.root)?;
        if self.current_block_terminated() {
            return Ok(root_or_view);
        }
        let dynamic_args = match self.lower_host_path_arguments(&checked.dynamic_arguments)? {
            ControlFlow::Break(value) => return Ok(value),
            ControlFlow::Continue(values) => values,
        };
        let dst = self.alloc_temp(self.expr_type(expr_id)?);
        let fingerprint = checked
            .contract
            .fingerprint(&self.planner.definitions())
            .map_err(|_| MirLoweringError::MissingBinding("checked host path contract"))?;
        self.emit(Instruction::ReadPath {
            dst,
            root_or_view,
            dynamic_args,
            path: PathRef {
                declaration: Some(checked.declaration),
                contract_fingerprint: fingerprint,
                root_ty: root_or_view.ty,
                result_ty: dst.ty,
                read_only: true,
                debug_name: "host path read".into(),
            },
        });
        Ok(dst)
    }

    pub(super) fn lower_field(
        &mut self,
        expr_id: ExprId,
        receiver: ExprId,
        field: DefinitionId,
    ) -> Result<MirValue, MirLoweringError> {
        let base = self.lower_expr(receiver)?;
        if self.current_block_terminated() {
            return Ok(base);
        }
        let receiver_ty = self
            .expression_type(receiver)
            .ok_or(MirLoweringError::MissingExprType(receiver))?;
        let field = self.aggregate_field_ref(&field, &receiver_ty)?;
        let dst = self.alloc_temp(self.expr_type(expr_id)?);
        self.emit(Instruction::ReadAggregateField { dst, base, field });
        Ok(dst)
    }

    pub(super) fn lower_index(
        &mut self,
        expr_id: ExprId,
        receiver: ExprId,
        index: ExprId,
        operation: IndexRead,
    ) -> Result<MirValue, MirLoweringError> {
        let base = self.lower_expr(receiver)?;
        if self.current_block_terminated() {
            return Ok(base);
        }
        let index = self.lower_expr(index)?;
        if self.current_block_terminated() {
            return Ok(index);
        }
        if let IndexRead::Method(call) = operation {
            return self.lower_selected_operator(&call, &[base, index]);
        }
        let dst = self.alloc_temp(self.expr_type(expr_id)?);
        self.emit(Instruction::ReadAggregateIndex { dst, base, index });
        Ok(dst)
    }
}
