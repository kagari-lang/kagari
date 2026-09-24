use super::{IrLoweringError, state::FunctionLowerer};
use crate::module::{AggregateFieldRef, Instruction, IrValue, LocalId, ValueType};
use kagari_hir::{hir, types::TypeId};

pub(super) struct PreparedPlace {
    host_path: Option<crate::module::PathRef>,
    dynamic_args: crate::module::ValueBuffer,
    root: Root,
    projections: Vec<Projection>,
}

enum Root {
    Local { local: LocalId, ty: ValueType },
    Value(IrValue),
}

struct Projection {
    kind: ProjectionKind,
    ty: ValueType,
    tuple_base: bool,
}

enum ProjectionKind {
    Field(AggregateFieldRef),
    Index(IrValue),
}

impl FunctionLowerer<'_, '_> {
    /// Capture identity-bearing roots and dynamic indexes once, before RHS code.
    /// `None` means target evaluation terminated control flow; no location exists.
    pub(super) fn prepare_place(
        &mut self,
        id: hir::PlaceId,
    ) -> Result<Option<PreparedPlace>, IrLoweringError> {
        if let Some(checked) = self.analyzed.typed.type_table.host_place_path(id).cloned() {
            let Some(prepared) = self.prepare_place_inner(checked.root, true)? else {
                return Ok(None);
            };
            let mut value = match prepared.root {
                Root::Value(value) => value,
                Root::Local { local, ty } => {
                    let dst = self.alloc_temp(ty);
                    self.emit(Instruction::LoadLocal { dst, local });
                    dst
                }
            };
            for projection in &prepared.projections {
                value = self.read_projection(value, projection);
            }
            let dynamic_args = if let hir::PlaceKind::Index { index, .. } =
                self.analyzed.lowered.module.place(id).kind
            {
                let arg = self.lower_expr(index)?;
                if self.current_block_terminated() {
                    return Ok(None);
                }
                smallvec::smallvec![arg]
            } else {
                Default::default()
            };
            let path = crate::module::PathRef {
                declaration: Some(checked.declaration),
                contract_fingerprint: checked
                    .contract
                    .fingerprint()
                    .map_err(|_| IrLoweringError::MissingBinding("checked host write contract"))?,
                root_ty: value.ty,
                result_ty: self.place_type(id)?,
                read_only: false,
                debug_name: "host field write".into(),
            };
            return Ok(Some(PreparedPlace {
                host_path: Some(path),
                dynamic_args,
                root: Root::Value(value),
                projections: Vec::new(),
            }));
        }
        self.prepare_place_inner(id, false)
    }

    fn prepare_place_inner(
        &mut self,
        id: hir::PlaceId,
        projected: bool,
    ) -> Result<Option<PreparedPlace>, IrLoweringError> {
        let place = self.analyzed.lowered.module.place(id).clone();
        match place.kind {
            hir::PlaceKind::Name(_) => {
                let local = self.lookup_binding(self.place_root_resolution(id)?)?;
                let ty = self.place_type(id)?;
                let root = if projected
                    && matches!(
                        self.analyzed.typed.type_table.place_type(id),
                        Some(
                            TypeId::Struct(_)
                                | TypeId::Array(_)
                                | TypeId::Map { .. }
                                | TypeId::Set(_)
                        )
                    ) {
                    let dst = self.alloc_temp(ty);
                    self.emit(Instruction::LoadLocal { dst, local });
                    Root::Value(dst)
                } else {
                    Root::Local { local, ty }
                };
                Ok(Some(PreparedPlace {
                    host_path: None,
                    dynamic_args: Default::default(),
                    root,
                    projections: Vec::new(),
                }))
            }
            hir::PlaceKind::Expr(expr) => {
                let value = self.lower_expr(expr)?;
                if self.current_block_terminated() {
                    return Ok(None);
                }
                Ok(Some(PreparedPlace {
                    host_path: None,
                    dynamic_args: Default::default(),
                    root: Root::Value(value),
                    projections: Vec::new(),
                }))
            }
            hir::PlaceKind::Field { base, .. } => {
                let field = self
                    .analyzed
                    .typed
                    .type_table
                    .place_field(id)
                    .ok_or(IrLoweringError::MissingBinding("checked field assignment"))?;
                let Some(mut place) = self.prepare_place_inner(base, true)? else {
                    return Ok(None);
                };
                let receiver_ty = self
                    .analyzed
                    .typed
                    .type_table
                    .place_type(base)
                    .ok_or(IrLoweringError::UnresolvedPlace(base))?;
                place.projections.push(Projection {
                    kind: ProjectionKind::Field(self.aggregate_field_ref(field, &receiver_ty)?),
                    ty: self.place_type(id)?,
                    tuple_base: false,
                });
                Ok(Some(place))
            }
            hir::PlaceKind::Index { base, index } => {
                let Some(mut place) = self.prepare_place_inner(base, true)? else {
                    return Ok(None);
                };
                let index = self.lower_expr(index)?;
                if self.current_block_terminated() {
                    return Ok(None);
                }
                place.projections.push(Projection {
                    kind: ProjectionKind::Index(index),
                    ty: self.place_type(id)?,
                    tuple_base: matches!(
                        self.analyzed.typed.type_table.place_type(base),
                        Some(TypeId::Tuple(_))
                    ),
                });
                Ok(Some(place))
            }
        }
    }

    pub(super) fn commit_place(
        &mut self,
        place: PreparedPlace,
        op: Option<hir::BinaryOp>,
        rhs: IrValue,
    ) -> Result<(), IrLoweringError> {
        if let Some(path) = place.host_path {
            let Root::Value(root_or_view) = place.root else {
                return Err(IrLoweringError::MissingBinding("prepared host root"));
            };
            if let Some(op) = op {
                self.emit(Instruction::ModifyPath {
                    dst: None,
                    root_or_view,
                    path,
                    dynamic_args: place.dynamic_args,
                    op: Self::lower_binary_op(op),
                    value: rhs,
                });
            } else {
                self.emit(Instruction::SetPath {
                    root_or_view,
                    path,
                    dynamic_args: place.dynamic_args,
                    value: rhs,
                });
            }
            return Ok(());
        }
        // Rebinding an object variable still targets the slot, not its old object.
        let root = match place.root {
            Root::Local { local, ty } => {
                if place.projections.is_empty() && op.is_none() {
                    self.emit(Instruction::StoreLocal { local, src: rhs });
                    return Ok(());
                }
                let dst = self.alloc_temp(ty);
                self.emit(Instruction::LoadLocal { dst, local });
                dst
            }
            Root::Value(value) => value,
        };
        let mut base = root;
        let mut path = Vec::new();
        let count = place.projections.len();
        for (index, projection) in place.projections.into_iter().enumerate() {
            let next = if index + 1 < count {
                Some(self.read_projection(base, &projection))
            } else {
                None
            };
            path.push((base, projection));
            if let Some(next) = next {
                base = next;
            }
        }
        let mut result = if let Some(op) = op {
            let current = path.last().map_or(root, |(base, projection)| {
                self.read_projection(*base, projection)
            });
            let dst = self.alloc_temp(current.ty);
            self.emit(Instruction::Binary {
                dst,
                op: Self::lower_binary_op(op),
                lhs: current,
                rhs,
            });
            dst
        } else {
            rhs
        };
        // Tuple changes are prepared in temporary values. Only the first enclosing
        // mutable object or local slot commits; identity-bearing ancestors need no rewrite.
        for (base, projection) in path.into_iter().rev() {
            match projection.kind {
                ProjectionKind::Field(field) => self.emit(Instruction::WriteAggregateField {
                    base,
                    field,
                    value: result,
                }),
                ProjectionKind::Index(index) => self.emit(Instruction::WriteAggregateIndex {
                    base,
                    index,
                    value: result,
                }),
            }
            if !projection.tuple_base {
                return Ok(());
            }
            result = base;
        }
        match place.root {
            Root::Local { local, .. } => {
                self.emit(Instruction::StoreLocal { local, src: result });
                Ok(())
            }
            Root::Value(_) => Err(IrLoweringError::UnsupportedStatement(
                "temporary value is not an assignment slot",
            )),
        }
    }

    fn read_projection(&mut self, base: IrValue, projection: &Projection) -> IrValue {
        let dst = self.alloc_temp(projection.ty);
        match &projection.kind {
            ProjectionKind::Field(field) => self.emit(Instruction::ReadAggregateField {
                dst,
                base,
                field: field.clone(),
            }),
            ProjectionKind::Index(index) => self.emit(Instruction::ReadAggregateIndex {
                dst,
                base,
                index: *index,
            }),
        }
        dst
    }
}
