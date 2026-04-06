use kagari_hir::{hir, resolver::ResolvedName};

use crate::lower::IrLoweringError;
use crate::lower::state::FunctionLowerer;
use crate::lower::{EvaluatedConst, EvaluatedConstField};
use crate::module::ids::{LocalId, ModuleSlotId, TempId};
use crate::module::instruction::{
    BinaryOp, CallTarget, Constant, Instruction, StructFieldInit, UnaryOp,
};
use crate::module::types::ValueType;

impl FunctionLowerer<'_> {
    pub(crate) fn bind_local(
        &mut self,
        hir_local: hir::LocalId,
        name: String,
    ) -> Result<LocalId, IrLoweringError> {
        let ty = self
            .analyzed
            .typed
            .type_table
            .local_type(hir_local)
            .as_ref()
            .map(ValueType::from_type_id)
            .ok_or(IrLoweringError::MissingLocalType(hir_local))?;
        let local = self.alloc_local(name, ty);
        self.locals.insert(hir_local, local);
        Ok(local)
    }

    pub(crate) fn lookup_binding(
        &self,
        resolved: ResolvedName,
    ) -> Result<LocalId, IrLoweringError> {
        match resolved {
            ResolvedName::Param(id) => self
                .params
                .get(&id)
                .copied()
                .ok_or(IrLoweringError::MissingBinding("parameter")),
            ResolvedName::Local(id) => self
                .locals
                .get(&id)
                .copied()
                .ok_or(IrLoweringError::MissingBinding("local")),
            ResolvedName::Const(_)
            | ResolvedName::Static(_)
            | ResolvedName::Function(_)
            | ResolvedName::Struct(_)
            | ResolvedName::Enum(_) => Err(IrLoweringError::UnsupportedExpr(
                "non-local binding used as local value",
            )),
        }
    }

    pub(crate) fn lookup_module_slot(
        &self,
        resolved: ResolvedName,
    ) -> Result<ModuleSlotId, IrLoweringError> {
        match resolved {
            ResolvedName::Static(id) => self
                .static_slots
                .get(&id)
                .copied()
                .ok_or(IrLoweringError::MissingBinding("static")),
            ResolvedName::Const(_)
            | ResolvedName::Param(_)
            | ResolvedName::Local(_)
            | ResolvedName::Function(_)
            | ResolvedName::Struct(_)
            | ResolvedName::Enum(_) => Err(IrLoweringError::UnsupportedExpr(
                "non-module binding used as module slot",
            )),
        }
    }

    pub(crate) fn expr_type(&self, expr_id: hir::ExprId) -> Result<ValueType, IrLoweringError> {
        self.analyzed
            .typed
            .type_table
            .expr_type(expr_id)
            .as_ref()
            .map(ValueType::from_type_id)
            .ok_or(IrLoweringError::MissingExprType(expr_id))
    }

    pub(crate) fn place_type(&self, place_id: hir::PlaceId) -> Result<ValueType, IrLoweringError> {
        self.analyzed
            .typed
            .type_table
            .place_type(place_id)
            .as_ref()
            .map(ValueType::from_type_id)
            .ok_or(IrLoweringError::UnresolvedPlace(place_id))
    }

    pub(crate) fn place_root(&self, place_id: hir::PlaceId) -> hir::PlaceId {
        match &self.analyzed.lowered.module.place(place_id).kind {
            hir::PlaceKind::Name(_) => place_id,
            hir::PlaceKind::Field { base, .. } | hir::PlaceKind::Index { base, .. } => {
                self.place_root(*base)
            }
        }
    }

    pub(crate) fn place_root_resolution(
        &self,
        place_id: hir::PlaceId,
    ) -> Result<ResolvedName, IrLoweringError> {
        let root = self.place_root(place_id);
        self.analyzed
            .names
            .place_resolution(root)
            .ok_or(IrLoweringError::UnresolvedPlace(root))
    }

    pub(crate) fn lower_constant(&mut self, constant: Constant, ty: ValueType) -> TempId {
        let dst = self.alloc_temp(ty);
        self.emit(Instruction::LoadConst { dst, constant });
        dst
    }

    pub(crate) fn lower_unit(&mut self) -> TempId {
        self.lower_constant(Constant::Unit, ValueType::Unit)
    }

    pub(crate) fn lower_evaluated_const(
        &mut self,
        value: &EvaluatedConst,
        ty: ValueType,
    ) -> Result<TempId, IrLoweringError> {
        match value {
            EvaluatedConst::Scalar(constant) => Ok(self.lower_constant(constant.clone(), ty)),
            EvaluatedConst::Tuple(elements) => {
                let elements = elements
                    .iter()
                    .map(|element| self.lower_evaluated_const(element, ValueType::HeapObject))
                    .collect::<Result<_, _>>()?;
                let dst = self.alloc_temp(ty);
                self.emit(Instruction::MakeTuple { dst, elements });
                Ok(dst)
            }
            EvaluatedConst::Array(elements) => {
                let elements = elements
                    .iter()
                    .map(|element| self.lower_evaluated_const(element, ValueType::HeapObject))
                    .collect::<Result<_, _>>()?;
                let dst = self.alloc_temp(ty);
                self.emit(Instruction::MakeArray { dst, elements });
                Ok(dst)
            }
            EvaluatedConst::Struct { name, fields } => {
                let fields = fields
                    .iter()
                    .map(|field| self.lower_const_field(field))
                    .collect::<Result<_, _>>()?;
                let dst = self.alloc_temp(ty);
                self.emit(Instruction::MakeStruct {
                    dst,
                    name: name.clone(),
                    fields,
                });
                Ok(dst)
            }
        }
    }

    fn lower_const_field(
        &mut self,
        field: &EvaluatedConstField,
    ) -> Result<StructFieldInit, IrLoweringError> {
        Ok(StructFieldInit {
            name: field.name.clone(),
            value: self.lower_evaluated_const(&field.value, ValueType::HeapObject)?,
        })
    }

    pub(crate) fn lower_name_expr(
        &mut self,
        expr_id: hir::ExprId,
    ) -> Result<TempId, IrLoweringError> {
        let resolved = self
            .analyzed
            .names
            .expr_resolution(expr_id)
            .ok_or(IrLoweringError::UnresolvedExpr(expr_id))?;

        match resolved {
            ResolvedName::Param(_) | ResolvedName::Local(_) => {
                let local = self.lookup_binding(resolved)?;
                let dst = self.alloc_temp(self.expr_type(expr_id)?);
                self.emit(Instruction::LoadLocal { dst, local });
                Ok(dst)
            }
            ResolvedName::Const(id) => {
                let constant = self
                    .const_values
                    .get(&id)
                    .cloned()
                    .ok_or(IrLoweringError::MissingBinding("const value"))?;
                self.lower_evaluated_const(&constant, self.expr_type(expr_id)?)
            }
            ResolvedName::Static(_) => {
                let slot = self.lookup_module_slot(resolved)?;
                let dst = self.alloc_temp(self.expr_type(expr_id)?);
                self.emit(Instruction::LoadModule { dst, slot });
                Ok(dst)
            }
            ResolvedName::Function(_) => Err(IrLoweringError::UnsupportedExpr(
                "bare function values are not lowered yet",
            )),
            ResolvedName::Struct(_) | ResolvedName::Enum(_) => Err(
                IrLoweringError::UnsupportedExpr("type-level names are not value expressions"),
            ),
        }
    }

    pub(crate) fn lower_direct_callee(
        &self,
        expr_id: hir::ExprId,
    ) -> Result<Option<CallTarget>, IrLoweringError> {
        let Some(resolved) = self.analyzed.names.expr_resolution(expr_id) else {
            return Ok(None);
        };

        match resolved {
            ResolvedName::Function(id) => Ok(Some(CallTarget::Function(id))),
            ResolvedName::Const(_)
            | ResolvedName::Static(_)
            | ResolvedName::Param(_)
            | ResolvedName::Local(_)
            | ResolvedName::Struct(_)
            | ResolvedName::Enum(_) => Ok(None),
        }
    }

    pub(crate) fn lower_unary_op(op: hir::PrefixOp) -> UnaryOp {
        match op {
            hir::PrefixOp::Neg => UnaryOp::Neg,
            hir::PrefixOp::Not => UnaryOp::Not,
        }
    }

    pub(crate) fn lower_binary_op(op: hir::BinaryOp) -> BinaryOp {
        match op {
            hir::BinaryOp::Add => BinaryOp::Add,
            hir::BinaryOp::Sub => BinaryOp::Sub,
            hir::BinaryOp::Mul => BinaryOp::Mul,
            hir::BinaryOp::Div => BinaryOp::Div,
            hir::BinaryOp::Eq => BinaryOp::Eq,
            hir::BinaryOp::NotEq => BinaryOp::NotEq,
            hir::BinaryOp::Lt => BinaryOp::Lt,
            hir::BinaryOp::Gt => BinaryOp::Gt,
            hir::BinaryOp::Le => BinaryOp::Le,
            hir::BinaryOp::Ge => BinaryOp::Ge,
            hir::BinaryOp::AndAnd => BinaryOp::AndAnd,
            hir::BinaryOp::OrOr => BinaryOp::OrOr,
        }
    }
}
