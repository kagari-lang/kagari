use kagari_hir::{hir, resolver::ResolvedName};

use crate::lower::IrLoweringError;
use crate::lower::state::FunctionLowerer;
use crate::module::ids::{LocalId, TempId};
use crate::module::instruction::{BinaryOp, CallTarget, Constant, Instruction, UnaryOp};
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
            ResolvedName::Function(_) | ResolvedName::Struct(_) | ResolvedName::Enum(_) => Err(
                IrLoweringError::UnsupportedExpr("non-local binding used as local value"),
            ),
        }
    }

    pub(crate) fn expr_type(&self, expr_id: hir::ExprId) -> Result<ValueType, IrLoweringError> {
        self.analyzed
            .typed
            .type_table
            .expr_type(expr_id)
            .map(ValueType::from_type_id)
            .ok_or(IrLoweringError::MissingExprType(expr_id))
    }

    pub(crate) fn lower_constant(&mut self, constant: Constant, ty: ValueType) -> TempId {
        let dst = self.alloc_temp(ty);
        self.emit(Instruction::LoadConst { dst, constant });
        dst
    }

    pub(crate) fn lower_unit(&mut self) -> TempId {
        self.lower_constant(Constant::Unit, ValueType::Unit)
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
            ResolvedName::Param(_)
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
