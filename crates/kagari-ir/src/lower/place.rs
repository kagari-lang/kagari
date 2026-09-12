use super::{IrLoweringError, state::FunctionLowerer};
use crate::module::{AggregateFieldRef, Instruction, IrValue, LocalId, ValueType};
use kagari_hir::{hir, types::TypeId};

pub(super) struct PreparedPlace {
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

impl FunctionLowerer<'_> {
    /// Capture identity-bearing roots and dynamic indexes once, before RHS code.
    pub(super) fn prepare_place(
        &mut self,
        id: hir::PlaceId,
    ) -> Result<PreparedPlace, IrLoweringError> {
        self.prepare_place_inner(id, false)
    }

    fn prepare_place_inner(
        &mut self,
        id: hir::PlaceId,
        projected: bool,
    ) -> Result<PreparedPlace, IrLoweringError> {
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
                Ok(PreparedPlace {
                    root,
                    projections: Vec::new(),
                })
            }
            hir::PlaceKind::Expr(expr) => Ok(PreparedPlace {
                root: Root::Value(self.lower_expr(expr)?),
                projections: Vec::new(),
            }),
            hir::PlaceKind::Field { base, .. } => {
                let field = self
                    .analyzed
                    .typed
                    .type_table
                    .place_field(id)
                    .ok_or(IrLoweringError::MissingBinding("checked field assignment"))?;
                let mut place = self.prepare_place_inner(base, true)?;
                place.projections.push(Projection {
                    kind: ProjectionKind::Field(self.aggregate_field_ref(field)),
                    ty: self.place_type(id)?,
                    tuple_base: false,
                });
                Ok(place)
            }
            hir::PlaceKind::Index { base, index } => {
                let mut place = self.prepare_place_inner(base, true)?;
                let index = self.lower_expr(index)?;
                place.projections.push(Projection {
                    kind: ProjectionKind::Index(index),
                    ty: self.place_type(id)?,
                    tuple_base: matches!(
                        self.analyzed.typed.type_table.place_type(base),
                        Some(TypeId::Tuple(_))
                    ),
                });
                Ok(place)
            }
        }
    }

    pub(super) fn commit_place(
        &mut self,
        place: PreparedPlace,
        op: Option<hir::BinaryOp>,
        rhs: IrValue,
    ) -> Result<(), IrLoweringError> {
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
