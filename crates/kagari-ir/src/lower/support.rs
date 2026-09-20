use kagari_hir::{hir, resolver::ResolvedName};

use crate::lower::IrLoweringError;
use crate::lower::state::FunctionLowerer;
use crate::module::ids::LocalId;
use crate::module::instruction::{
    AggregateFieldRef, BinaryOp, Constant, Instruction, IrValue, UnaryOp,
};
use crate::module::types::ValueType;
use kagari_hir::typeck::ScalarValue;

impl From<ScalarValue> for Constant {
    fn from(value: ScalarValue) -> Self {
        match value {
            ScalarValue::Unit => Self::Unit,
            ScalarValue::Bool(value) => Self::Bool(value),
            ScalarValue::I32(value) => Self::I32(value),
            ScalarValue::F32(value) => Self::F32(value),
            ScalarValue::String(value) => Self::Str(value),
        }
    }
}

impl FunctionLowerer<'_, '_> {
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
            .map(|ty| self.value_type(ty))
            .transpose()?
            .ok_or(IrLoweringError::MissingLocalType(hir_local))?;
        let local = self.alloc_local(
            name,
            ty,
            self.analyzed.lowered.source_map.local_span(hir_local),
        );
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
            | ResolvedName::Function(_)
            | ResolvedName::Module(_)
            | ResolvedName::StandardModule(_)
            | ResolvedName::HostFunction(_)
            | ResolvedName::SourceItem { .. }
            | ResolvedName::SourceImport(_)
            | ResolvedName::HostType(_)
            | ResolvedName::HostModule(_)
            | ResolvedName::StandardFunction(_)
            | ResolvedName::RuntimeHelper(_)
            | ResolvedName::Struct(_)
            | ResolvedName::Enum(_)
            | ResolvedName::Trait(_) => Err(IrLoweringError::UnsupportedExpr(
                "non-local binding used as local value",
            )),
        }
    }

    pub(crate) fn expr_type(&self, expr_id: hir::ExprId) -> Result<ValueType, IrLoweringError> {
        self.analyzed
            .typed
            .type_table
            .expr_type(expr_id)
            .as_ref()
            .map(|ty| self.value_type(ty))
            .transpose()?
            .ok_or(IrLoweringError::MissingExprType(expr_id))
    }

    pub(crate) fn place_type(&self, place_id: hir::PlaceId) -> Result<ValueType, IrLoweringError> {
        self.analyzed
            .typed
            .type_table
            .place_type(place_id)
            .as_ref()
            .map(|ty| self.value_type(ty))
            .transpose()?
            .ok_or(IrLoweringError::UnresolvedPlace(place_id))
    }

    pub(crate) fn aggregate_field_ref(
        &self,
        field: &kagari_common::identity::DefinitionId,
        receiver: &kagari_hir::types::TypeId,
    ) -> Result<AggregateFieldRef, IrLoweringError> {
        let field = self
            .analyzed
            .aggregates
            .field(field)
            .expect("checked field contract");
        let owner = self.nominal_instance(receiver)?;
        if owner.declaration != field.owner {
            return Err(IrLoweringError::MissingBinding("field instance owner"));
        }
        Ok(AggregateFieldRef {
            owner,
            slot: field.slot,
        })
    }

    pub(crate) fn nominal_instance(
        &self,
        ty: &kagari_hir::types::TypeId,
    ) -> Result<crate::module::abi::NominalAbiType, IrLoweringError> {
        let types = self.planner.arguments(
            std::slice::from_ref(ty),
            &self.instance.substitution,
            self.function.debug.source_span,
        )?;
        let ty = &types[0];
        if !ty.is_concrete() {
            return Err(IrLoweringError::MissingBinding("concrete nominal instance"));
        }
        let (kagari_hir::types::TypeId::Struct(ty)
        | kagari_hir::types::TypeId::Enum(ty)
        | kagari_hir::types::TypeId::Trait(ty)) = ty
        else {
            return Err(IrLoweringError::MissingBinding("nominal instance"));
        };
        Ok(crate::module::abi::NominalAbiType::from_checked_type(ty))
    }

    pub(crate) fn expr_nominal_instance(
        &self,
        id: hir::ExprId,
    ) -> Result<crate::module::abi::NominalAbiType, IrLoweringError> {
        let ty = self
            .analyzed
            .typed
            .type_table
            .expr_type(id)
            .ok_or(IrLoweringError::MissingExprType(id))?;
        self.nominal_instance(&ty)
    }
    pub(crate) fn place_root(&self, place_id: hir::PlaceId) -> hir::PlaceId {
        match &self.analyzed.lowered.module.place(place_id).kind {
            hir::PlaceKind::Name(_) | hir::PlaceKind::Expr(_) => place_id,
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

    pub(crate) fn lower_constant(&mut self, constant: Constant, ty: ValueType) -> IrValue {
        let dst = self.alloc_temp(ty);
        self.emit(Instruction::LoadConst { dst, constant });
        dst
    }

    pub(crate) fn lower_unit(&mut self) -> IrValue {
        self.lower_constant(Constant::Unit, ValueType::Unit)
    }

    pub(crate) fn lower_name_expr(
        &mut self,
        expr_id: hir::ExprId,
    ) -> Result<IrValue, IrLoweringError> {
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
                    .analyzed
                    .typed
                    .const_values
                    .get(&id)
                    .cloned()
                    .ok_or(IrLoweringError::MissingBinding("const value"))?;
                Ok(self.lower_constant(constant.into(), self.expr_type(expr_id)?))
            }
            ResolvedName::Function(_) => Err(IrLoweringError::UnsupportedExpr(
                "bare function values are not lowered yet",
            )),
            ResolvedName::HostFunction(_)
            | ResolvedName::SourceItem { .. }
            | ResolvedName::SourceImport(_)
            | ResolvedName::HostType(_)
            | ResolvedName::HostModule(_)
            | ResolvedName::StandardFunction(_)
            | ResolvedName::RuntimeHelper(_) => Err(IrLoweringError::UnsupportedExpr(
                "bare standard functions are not lowered yet",
            )),
            ResolvedName::Module(_)
            | ResolvedName::StandardModule(_)
            | ResolvedName::Struct(_)
            | ResolvedName::Enum(_)
            | ResolvedName::Trait(_) => Err(IrLoweringError::UnsupportedExpr(
                "type-level names are not value expressions",
            )),
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
