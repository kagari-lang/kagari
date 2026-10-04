use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_abi::representation::ValueType;
use kagari_common::identity::DefinitionPath;
use kagari_contract::operations::{BinaryOp, UnaryOp};
use kagari_hir::{
    hir::{
        expr::ops::{BinaryOp as HirBinaryOp, PrefixOp},
        ids::{ExprId, LocalId as HirLocalId, PlaceId},
        place::PlaceKind,
    },
    resolver::resolved::ResolvedName,
    typeck::scalar::ScalarValue,
    types::{
        TypeId,
        abi::{lower_nominal_type, lower_type},
    },
};
use kagari_mir::{
    ids::LocalId,
    instruction::{AggregateFieldRef, Constant, Instruction, MirValue, ValueBuffer},
};
use kagari_types::{
    language as standard_traits,
    language::Protocol,
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};
use std::{ops::ControlFlow, slice};

pub(crate) fn lower_scalar(value: ScalarValue) -> Constant {
    match value {
        ScalarValue::Unit => Constant::Unit,
        ScalarValue::Bool(value) => Constant::Bool(value),
        ScalarValue::I32(value) => Constant::I32(value),
        ScalarValue::Integer { value, ty } => match ty {
            BuiltinType::I8 | BuiltinType::I16 => Constant::I32(value as i32),
            BuiltinType::U64 | BuiltinType::USize => Constant::U64(value as u64),
            _ => Constant::I64(value as i64),
        },
        ScalarValue::F32(value) => Constant::F32(value),
        ScalarValue::F64(value) => Constant::F64(value),
        ScalarValue::String(value) => Constant::Str(value),
    }
}

impl FunctionLowerer<'_, '_> {
    /// Protocol lowering must use the checked declaration supplied by HIR. A
    /// missing contract or slot is an invalid lowering input, not a catalog fallback.
    pub(super) fn protocol_method(
        &self,
        protocol: Protocol,
        slot: usize,
    ) -> Result<DefinitionPath, MirLoweringError> {
        self.planner
            .catalog
            .trait_(&standard_traits::identity(protocol))
            .and_then(|contract| contract.methods.get(slot))
            .map(|method| method.id.clone())
            .ok_or(MirLoweringError::MissingBinding("checked protocol method"))
    }

    pub(super) fn lower_host_path_arguments(
        &mut self,
        arguments: &[(u32, ExprId)],
    ) -> Result<ControlFlow<MirValue, ValueBuffer>, MirLoweringError> {
        let mut ordered = vec![None; arguments.len()];
        for (slot, argument) in arguments {
            let value = self.lower_expr(*argument)?;
            if self.current_block_terminated() {
                return Ok(ControlFlow::Break(value));
            }
            let Some(target) = ordered.get_mut(*slot as usize) else {
                return Err(MirLoweringError::MissingBinding("host path argument slot"));
            };
            if target.replace(value).is_some() {
                return Err(MirLoweringError::MissingBinding(
                    "repeated source path argument",
                ));
            }
        }
        let values = ordered
            .into_iter()
            .map(|value| {
                value.ok_or(MirLoweringError::MissingBinding(
                    "missing host path argument",
                ))
            })
            .collect::<Result<ValueBuffer, _>>()?;
        Ok(ControlFlow::Continue(values))
    }

    pub(crate) fn bind_local(
        &mut self,
        hir_local: HirLocalId,
        name: String,
    ) -> Result<LocalId, MirLoweringError> {
        let ty = self
            .analyzed
            .typed
            .type_table
            .local_type(hir_local)
            .as_ref()
            .map(|ty| self.value_type(ty))
            .transpose()?
            .ok_or(MirLoweringError::MissingLocalType(hir_local))?;
        let ty = if self.cell_locals.contains(&hir_local) {
            ValueType::HeapObject
        } else {
            ty
        };
        let local = self.alloc_local(
            name,
            ty,
            self.analyzed.lowered.source_map.local_span(hir_local),
        );
        let semantic = self.semantic_type(
            &self
                .analyzed
                .typed
                .type_table
                .local_type(hir_local)
                .ok_or(MirLoweringError::MissingLocalType(hir_local))?,
        )?;
        self.function
            .semantic
            .locals
            .insert(local.index(), semantic);
        self.locals.insert(hir_local, local);
        Ok(local)
    }

    pub(crate) fn lookup_binding(
        &self,
        resolved: ResolvedName,
    ) -> Result<LocalId, MirLoweringError> {
        match resolved {
            ResolvedName::Param(id) => self
                .params
                .get(&id)
                .copied()
                .ok_or(MirLoweringError::MissingBinding("parameter")),
            ResolvedName::Local(id) => self
                .locals
                .get(&id)
                .copied()
                .ok_or(MirLoweringError::MissingBinding("local")),
            ResolvedName::Const(_)
            | ResolvedName::Function(_)
            | ResolvedName::Module(_)
            | ResolvedName::HostFunction(_)
            | ResolvedName::SourceItem { .. }
            | ResolvedName::SourceImport(_)
            | ResolvedName::HostType(_)
            | ResolvedName::HostModule(_)
            | ResolvedName::RuntimeHelper(_)
            | ResolvedName::OpaqueType(_)
            | ResolvedName::Struct(_)
            | ResolvedName::Enum(_)
            | ResolvedName::Trait(_) => Err(MirLoweringError::UnsupportedExpr(
                "non-local binding used as local value",
            )),
        }
    }

    pub(crate) fn array_element_type(&self, expr_id: ExprId) -> Result<Ty, MirLoweringError> {
        let ty = self
            .analyzed
            .typed
            .type_table
            .expr_type(expr_id)
            .ok_or(MirLoweringError::MissingExprType(expr_id))?;
        let TypeId::Array(item, _) = ty else {
            return Err(MirLoweringError::MissingBinding(
                "checked array literal element type",
            ));
        };
        self.semantic_type(&item)
    }

    pub(crate) fn expr_type(&self, expr_id: ExprId) -> Result<ValueType, MirLoweringError> {
        self.analyzed
            .typed
            .type_table
            .expr_type(expr_id)
            .as_ref()
            .map(|ty| self.value_type(ty))
            .transpose()?
            .ok_or(MirLoweringError::MissingExprType(expr_id))
    }

    pub(crate) fn place_type(&self, place_id: PlaceId) -> Result<ValueType, MirLoweringError> {
        self.analyzed
            .typed
            .type_table
            .place_type(place_id)
            .as_ref()
            .map(|ty| self.value_type(ty))
            .transpose()?
            .ok_or(MirLoweringError::UnresolvedPlace(place_id))
    }

    pub(crate) fn aggregate_field_ref(
        &self,
        field: &DefinitionPath,
        receiver: &TypeId,
    ) -> Result<AggregateFieldRef, MirLoweringError> {
        let field = self
            .analyzed
            .aggregates
            .field(field)
            .expect("checked field contract");
        let owner = self.nominal_instance(receiver)?;
        if owner.declaration != field.owner {
            return Err(MirLoweringError::MissingBinding("field instance owner"));
        }
        Ok(AggregateFieldRef {
            owner,
            slot: field.slot,
        })
    }

    pub(crate) fn nominal_instance(&self, ty: &TypeId) -> Result<NominalTy, MirLoweringError> {
        let types = self.planner.arguments(
            slice::from_ref(ty),
            &self.instance.substitution,
            self.function.debug.source_span,
        )?;
        let ty = &types[0];
        if !ty.is_concrete()
            && !self.function.semantic.generic.as_ref().is_some_and(|body| {
                body.types_valid([&lower_type(ty)], &self.planner.options.cancel)
            })
        {
            return Err(MirLoweringError::MissingBinding(
                "concrete nominal instance",
            ));
        }
        let (TypeId::NativeObject(ty) | TypeId::Struct(ty) | TypeId::Enum(ty) | TypeId::Trait(ty)) =
            ty
        else {
            return Err(MirLoweringError::MissingBinding("nominal instance"));
        };
        Ok(lower_nominal_type(ty))
    }

    pub(crate) fn expr_nominal_instance(&self, id: ExprId) -> Result<NominalTy, MirLoweringError> {
        let ty = self
            .analyzed
            .typed
            .type_table
            .expr_type(id)
            .ok_or(MirLoweringError::MissingExprType(id))?;
        self.nominal_instance(&ty)
    }

    pub(crate) fn place_root(&self, place_id: PlaceId) -> PlaceId {
        match &self.analyzed.lowered.module.place(place_id).kind {
            PlaceKind::Name(_) | PlaceKind::Expr(_) => place_id,
            PlaceKind::Field { base, .. } | PlaceKind::Index { base, .. } => self.place_root(*base),
        }
    }

    pub(crate) fn place_root_resolution(
        &self,
        place_id: PlaceId,
    ) -> Result<ResolvedName, MirLoweringError> {
        let root = self.place_root(place_id);
        self.analyzed
            .names
            .place_resolution(root)
            .ok_or(MirLoweringError::UnresolvedPlace(root))
    }

    pub(crate) fn lower_constant(&mut self, constant: Constant, ty: ValueType) -> MirValue {
        let dst = self.alloc_temp(ty);
        self.emit(Instruction::LoadConst { dst, constant });
        dst
    }

    pub(crate) fn lower_unit(&mut self) -> MirValue {
        self.lower_constant(Constant::Unit, ValueType::Unit)
    }

    pub(crate) fn lower_name_expr(
        &mut self,
        expr_id: ExprId,
    ) -> Result<MirValue, MirLoweringError> {
        let resolved = self
            .analyzed
            .names
            .expr_resolution(expr_id)
            .ok_or(MirLoweringError::UnresolvedExpr(expr_id))?;

        match resolved {
            ResolvedName::Param(_) | ResolvedName::Local(_) => {
                let local = self.lookup_binding(resolved)?;
                let is_cell =
                    matches!(resolved, ResolvedName::Local(id) if self.cell_locals.contains(&id));
                let physical = if is_cell {
                    ValueType::HeapObject
                } else {
                    self.expr_type(expr_id)?
                };
                let loaded = self.alloc_temp(physical);
                self.emit(Instruction::LoadLocal { dst: loaded, local });
                if is_cell {
                    let dst = self.alloc_temp(self.expr_type(expr_id)?);
                    self.emit(Instruction::ReadCell { dst, cell: loaded });
                    return Ok(dst);
                }
                let dst = loaded;
                Ok(dst)
            }
            ResolvedName::Const(id) => {
                let constant = self
                    .analyzed
                    .typed
                    .const_values
                    .get(&id)
                    .cloned()
                    .ok_or(MirLoweringError::MissingBinding("const value"))?;
                Ok(self.lower_constant(lower_scalar(constant), self.expr_type(expr_id)?))
            }
            ResolvedName::Function(_) => Err(MirLoweringError::UnsupportedExpr(
                "bare function values are not lowered yet",
            )),
            ResolvedName::HostFunction(_)
            | ResolvedName::SourceItem { .. }
            | ResolvedName::SourceImport(_)
            | ResolvedName::HostType(_)
            | ResolvedName::HostModule(_)
            | ResolvedName::RuntimeHelper(_) => Err(MirLoweringError::UnsupportedExpr(
                "bare standard functions are not lowered yet",
            )),
            ResolvedName::Module(_)
            | ResolvedName::OpaqueType(_)
            | ResolvedName::Struct(_)
            | ResolvedName::Enum(_)
            | ResolvedName::Trait(_) => Err(MirLoweringError::UnsupportedExpr(
                "type-level names are not value expressions",
            )),
        }
    }

    pub(crate) fn lower_unary_op(op: PrefixOp) -> UnaryOp {
        match op {
            PrefixOp::Neg => UnaryOp::Neg,
            PrefixOp::Not => UnaryOp::Not,
        }
    }

    pub(crate) fn lower_binary_op(op: HirBinaryOp) -> BinaryOp {
        match op {
            HirBinaryOp::Add => BinaryOp::Add,
            HirBinaryOp::Sub => BinaryOp::Sub,
            HirBinaryOp::Mul => BinaryOp::Mul,
            HirBinaryOp::Div => BinaryOp::Div,
            HirBinaryOp::Rem => BinaryOp::Rem,
            HirBinaryOp::BitAnd
            | HirBinaryOp::BitOr
            | HirBinaryOp::BitXor
            | HirBinaryOp::Shl
            | HirBinaryOp::Shr => unreachable!("bit operations retain numeric contracts"),
            HirBinaryOp::Eq => BinaryOp::Eq,
            HirBinaryOp::NotEq => BinaryOp::NotEq,
            HirBinaryOp::IdentityEq => BinaryOp::IdentityEq,
            HirBinaryOp::IdentityNotEq => BinaryOp::IdentityNotEq,
            HirBinaryOp::Lt => BinaryOp::Lt,
            HirBinaryOp::Gt => BinaryOp::Gt,
            HirBinaryOp::Le => BinaryOp::Le,
            HirBinaryOp::Ge => BinaryOp::Ge,
            HirBinaryOp::AndAnd => BinaryOp::AndAnd,
            HirBinaryOp::OrOr => BinaryOp::OrOr,
        }
    }
}
