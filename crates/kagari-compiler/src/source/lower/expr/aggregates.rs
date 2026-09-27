use crate::source::lower::MirLoweringError;
use crate::source::lower::state::FunctionLowerer;
use crate::source::types::lower_type;
use kagari_abi::representation::ValueType;
use kagari_hir::hir;
use kagari_hir::typeck::ResolvedHostPath;
use kagari_mir::PathRef;
use kagari_mir::instruction::Instruction;
use kagari_mir::instruction::MirValue;
use kagari_mir::instruction::StructFieldInit;
use smallvec::SmallVec;
use std::ops::ControlFlow;

impl FunctionLowerer<'_, '_> {
    pub(super) fn lower_tuple(
        &mut self,
        expr_id: hir::ExprId,
        elements: hir::ExprBuffer,
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
        expr_id: hir::ExprId,
        elements: hir::ExprBuffer,
    ) -> Result<MirValue, MirLoweringError> {
        let elements = match self.lower_values(&elements)? {
            ControlFlow::Continue(elements) => elements,
            ControlFlow::Break(value) => return Ok(value),
        };
        let dst = self.alloc_temp(self.expr_type(expr_id)?);
        self.emit(Instruction::MakeArray { dst, elements });
        Ok(dst)
    }

    pub(super) fn lower_range(
        &mut self,
        expr_id: hir::ExprId,
        start: Option<hir::ExprId>,
        end: Option<hir::ExprId>,
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
            .analyzed
            .typed
            .type_table
            .expr_type(expr_id)
            .ok_or(MirLoweringError::MissingExprType(expr_id))?;
        let ty = self
            .planner
            .arguments(
                &[source],
                &self.instance.substitution,
                self.function.debug.source_span,
            )?
            .remove(0);
        let ty = lower_type(&ty);
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
        expr_id: hir::ExprId,
        fields: hir::FieldInitBuffer,
    ) -> Result<MirValue, MirLoweringError> {
        let target = self
            .analyzed
            .typed
            .type_table
            .struct_init(expr_id)
            .cloned()
            .ok_or(MirLoweringError::MissingBinding(
                "checked struct initializer",
            ))?;
        if fields.len() != target.fields.len() {
            return Err(MirLoweringError::MissingBinding(
                "checked initializer field count",
            ));
        }
        let mut lowered_fields = SmallVec::new();
        for (field, target) in fields.iter().zip(target.fields) {
            let target = target.ok_or(MirLoweringError::MissingBinding(
                "checked initializer field",
            ))?;
            let value = self.lower_expr(field.value)?;
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
        expr_id: hir::ExprId,
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
            .fingerprint()
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
        expr_id: hir::ExprId,
        receiver: hir::ExprId,
    ) -> Result<MirValue, MirLoweringError> {
        if let Some(checked) = self.analyzed.typed.type_table.host_path(expr_id).cloned() {
            return self.lower_host_path_read(expr_id, checked);
        }
        let base = self.lower_expr(receiver)?;
        if self.current_block_terminated() {
            return Ok(base);
        }
        let field = self
            .analyzed
            .typed
            .type_table
            .expr_field(expr_id)
            .ok_or(MirLoweringError::MissingBinding("checked field read"))?;
        let receiver_ty = self
            .analyzed
            .typed
            .type_table
            .expr_type(receiver)
            .ok_or(MirLoweringError::MissingExprType(receiver))?;
        let field = self.aggregate_field_ref(field, &receiver_ty)?;
        let dst = self.alloc_temp(self.expr_type(expr_id)?);
        self.emit(Instruction::ReadAggregateField { dst, base, field });
        Ok(dst)
    }

    pub(super) fn lower_index(
        &mut self,
        expr_id: hir::ExprId,
        receiver: hir::ExprId,
        index: hir::ExprId,
    ) -> Result<MirValue, MirLoweringError> {
        if let Some(checked) = self.analyzed.typed.type_table.host_path(expr_id).cloned() {
            return self.lower_host_path_read(expr_id, checked);
        }
        let base = self.lower_expr(receiver)?;
        if self.current_block_terminated() {
            return Ok(base);
        }
        let index = self.lower_expr(index)?;
        if self.current_block_terminated() {
            return Ok(index);
        }
        if self
            .analyzed
            .typed
            .type_table
            .call_resolution(expr_id)
            .is_some()
        {
            return self.lower_selected_operator(expr_id, &[base, index]);
        }
        let dst = self.alloc_temp(self.expr_type(expr_id)?);
        self.emit(Instruction::ReadAggregateIndex { dst, base, index });
        Ok(dst)
    }
}
