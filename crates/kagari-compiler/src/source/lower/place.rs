use crate::source::elaborate::model::{CheckedPlaceKind, IndexWrite, ValueBinding};
use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_abi::representation::ValueType;
use kagari_common::identity::table::DefinitionId;
use kagari_contract::{numeric::NumericOperation, operations::BinaryOp};
use kagari_hir::{
    compile_input::body::bindings::BindingId,
    hir::{expr::ops::BinaryOp as HirBinaryOp, ids::PlaceId},
    types::{NominalType, TypeId},
};
use kagari_mir::{
    ids::{LocalId, ModuleSlotId},
    instruction::{AggregateFieldRef, Instruction, MirValue, PathRef, ValueBuffer},
};
use kagari_types::{language::Protocol, ty::Ty};
use std::ops::ControlFlow;
use std::slice;

pub(super) struct PreparedPlace {
    host_path: Option<PathRef>,
    dynamic_args: ValueBuffer,
    root: Root,
    projections: Vec<Projection>,
}

enum Root {
    Local { local: LocalId, ty: ValueType },
    Cell { local: LocalId, ty: ValueType },
    Static { slot: ModuleSlotId, value: MirValue },
    Value(MirValue),
}

struct Projection {
    kind: ProjectionKind,
    ty: ValueType,
    tuple_base: bool,
}

enum ProjectionKind {
    Field(AggregateFieldRef),
    Index(MirValue),
    AppliedIndex {
        index: MirValue,
        receiver: TypeId,
        read: NominalType,
        write: (NominalType, DefinitionId),
    },
}

impl FunctionLowerer<'_, '_> {
    /// Capture identity-bearing roots and dynamic indexes once, before RHS code.
    /// `None` means target evaluation terminated control flow; no location exists.
    pub(super) fn prepare_place(
        &mut self,
        id: PlaceId,
    ) -> Result<Option<PreparedPlace>, MirLoweringError> {
        if let CheckedPlaceKind::Host(checked) = self.source_body().places[&id].kind.clone() {
            let Some(prepared) = self.prepare_place_inner(checked.root, true)? else {
                return Ok(None);
            };

            let value = self.read_prepared_place(&prepared)?;
            let dynamic_args = match self.lower_host_path_arguments(&checked.dynamic_arguments)? {
                ControlFlow::Break(_) => return Ok(None),
                ControlFlow::Continue(values) => values,
            };
            let path = PathRef {
                declaration: Some(checked.declaration),
                contract_fingerprint: checked
                    .contract
                    .fingerprint(&self.planner.definitions())
                    .map_err(|_| MirLoweringError::MissingBinding("checked host write contract"))?,
                root_ty: value.ty,
                result_ty: self.place_type(id)?,
                read_only: false,
                debug_name: "host path write".into(),
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
        id: PlaceId,
        projected: bool,
    ) -> Result<Option<PreparedPlace>, MirLoweringError> {
        let place = self.source_body().places[&id].clone();
        match place.kind {
            CheckedPlaceKind::Binding(binding) => {
                let binding = match binding {
                    ValueBinding::Static(declaration) => {
                        let slot = self.planner.static_slot(declaration)?;
                        // Initialization occurs before RHS evaluation, even for plain assignment.
                        let value = self.load_static(slot);
                        return Ok(Some(PreparedPlace {
                            host_path: None,
                            dynamic_args: Default::default(),
                            root: Root::Static { slot, value },
                            projections: Vec::new(),
                        }));
                    }
                    ValueBinding::Lexical(binding) => binding,
                };
                let local = self.lookup_binding(binding)?;
                let ty = self.place_type(id)?;
                let is_cell =
                    matches!(binding, BindingId::Local(id) if self.cell_locals.contains(&id));
                let root_type = place.ty;
                // Identity semantics belong to the instantiated receiver, including
                // roots declared as generic parameters or associated projections.
                let root = if projected
                    && matches!(
                        self.semantic_type(&root_type)?,
                        Ty::Trait(_)
                            | Ty::Reference { .. }
                            | Ty::NativeObject(_)
                            | Ty::Struct(_)
                            | Ty::Array(_)
                            | Ty::Map { .. }
                            | Ty::Set(_, _)
                    ) {
                    let dst = self.alloc_temp(ty);
                    if is_cell {
                        let cell = self.alloc_temp(ValueType::HeapObject);
                        self.emit(Instruction::LoadLocal { dst: cell, local });
                        self.emit(Instruction::ReadCell { dst, cell });
                    } else {
                        self.emit(Instruction::LoadLocal { dst, local });
                    }
                    Root::Value(dst)
                } else if is_cell {
                    Root::Cell { local, ty }
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
            CheckedPlaceKind::Value(expr) => {
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
            CheckedPlaceKind::Field { base, field } => {
                let Some(mut place) = self.prepare_place_inner(base, true)? else {
                    return Ok(None);
                };
                let receiver_ty = self.source_body().places[&base].ty.clone();
                place.projections.push(Projection {
                    kind: ProjectionKind::Field(self.aggregate_field_ref(&field, &receiver_ty)?),
                    ty: self.place_type(id)?,
                    tuple_base: false,
                });
                Ok(Some(place))
            }
            CheckedPlaceKind::Index {
                base,
                index,
                operation,
            } => {
                let Some(mut place) = self.prepare_place_inner(base, true)? else {
                    return Ok(None);
                };
                let receiver_type = self.source_body().places[&base].ty.clone();
                let receiver = self
                    .planner
                    .arguments(
                        slice::from_ref(&receiver_type),
                        &self.instance.substitution,
                        self.function.debug.source_span,
                    )?
                    .remove(0);
                if let IndexWrite::Method {
                    read,
                    write,
                    setter,
                } = operation
                {
                    let write = (write, setter);
                    let index = self.lower_expr(index)?;
                    if self.current_block_terminated() {
                        return Ok(None);
                    }
                    place.projections.push(Projection {
                        kind: ProjectionKind::AppliedIndex {
                            index,
                            receiver,
                            read,
                            write,
                        },
                        ty: self.place_type(id)?,
                        tuple_base: false,
                    });
                    return Ok(Some(place));
                }
                if let IndexWrite::ReadMethod(interface) = operation {
                    let value = self.read_prepared_place(&place)?;
                    let index = self.lower_expr(index)?;
                    if self.current_block_terminated() {
                        return Ok(None);
                    }
                    let method = self.protocol_method(Protocol::Index, 0)?;
                    let value = self.lower_applied_operator(
                        interface,
                        receiver_type,
                        &method,
                        &[value, index],
                    )?;
                    return Ok(Some(PreparedPlace {
                        host_path: None,
                        dynamic_args: Default::default(),
                        root: Root::Value(value),
                        projections: vec![],
                    }));
                }
                let index = self.lower_expr(index)?;
                if self.current_block_terminated() {
                    return Ok(None);
                }
                place.projections.push(Projection {
                    kind: ProjectionKind::Index(index),
                    ty: self.place_type(id)?,
                    tuple_base: matches!(receiver_type, TypeId::Tuple(_)),
                });
                Ok(Some(place))
            }
            CheckedPlaceKind::Host(_) => self.prepare_place(id),
            CheckedPlaceKind::Diverging {
                base,
                read_base,
                values,
            } => {
                if let Some(base) = base {
                    let Some(prepared) = self.prepare_place_inner(base, true)? else {
                        return Ok(None);
                    };
                    if read_base {
                        self.read_prepared_place(&prepared)?;
                    }
                }
                for value in values {
                    self.lower_expr(value)?;
                    if self.current_block_terminated() {
                        return Ok(None);
                    }
                }
                Err(MirLoweringError::MissingBinding(
                    "diverging place preparation completed",
                ))
            }
        }
    }

    /// Read an already prepared receiver before a host/index getter's operands.
    /// Deferred writable projections are left untouched until their normal commit path.
    fn read_prepared_place(&mut self, place: &PreparedPlace) -> Result<MirValue, MirLoweringError> {
        let mut value = match place.root {
            Root::Static { value, .. } | Root::Value(value) => value,
            Root::Local { local, ty } => {
                let dst = self.alloc_temp(ty);
                self.emit(Instruction::LoadLocal { dst, local });
                dst
            }
            Root::Cell { local, ty } => {
                let cell = self.alloc_temp(ValueType::HeapObject);
                self.emit(Instruction::LoadLocal { dst: cell, local });
                let dst = self.alloc_temp(ty);
                self.emit(Instruction::ReadCell { dst, cell });
                dst
            }
        };
        for projection in &place.projections {
            value = self.read_projection(value, projection)?;
        }
        if let Some(path) = &place.host_path {
            let mut path = path.clone();
            path.read_only = true;
            let dst = self.alloc_temp(path.result_ty);
            self.emit(Instruction::ReadPath {
                dst,
                root_or_view: value,
                dynamic_args: place.dynamic_args.clone(),
                path,
            });
            value = dst;
        }
        Ok(value)
    }

    pub(super) fn commit_place(
        &mut self,
        place: PreparedPlace,
        op: Option<HirBinaryOp>,
        numeric: Option<NumericOperation>,
        rhs: MirValue,
    ) -> Result<(), MirLoweringError> {
        if let Some(path) = place.host_path {
            let Root::Value(root_or_view) = place.root else {
                return Err(MirLoweringError::MissingBinding("prepared host root"));
            };
            if let Some(op) = op {
                self.emit(Instruction::ModifyPath {
                    dst: None,
                    root_or_view,
                    path,
                    dynamic_args: place.dynamic_args,
                    op: numeric
                        .map(BinaryOp::Numeric)
                        .unwrap_or_else(|| Self::lower_binary_op(op)),
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
            Root::Cell { local, ty } => {
                let cell = self.alloc_temp(ValueType::HeapObject);
                self.emit(Instruction::LoadLocal { dst: cell, local });
                if place.projections.is_empty() && op.is_none() {
                    self.emit(Instruction::WriteCell { cell, value: rhs });
                    return Ok(());
                }
                let dst = self.alloc_temp(ty);
                self.emit(Instruction::ReadCell { dst, cell });
                dst
            }
            Root::Static { value, .. } | Root::Value(value) => value,
        };
        let mut base = root;
        let mut path = Vec::new();
        let count = place.projections.len();
        for (index, projection) in place.projections.into_iter().enumerate() {
            let next = if index + 1 < count {
                Some(self.read_projection(base, &projection)?)
            } else {
                None
            };
            path.push((base, projection));
            if let Some(next) = next {
                base = next;
            }
        }
        let mut result = if let Some(op) = op {
            let current = match path.last() {
                Some((base, projection)) => self.read_projection(*base, projection)?,
                None => root,
            };
            let dst = self.alloc_temp(current.ty);
            // This synthesized expression has no HIR expression finalization. Preserve
            // the checked left-hand type for semantic stores such as StoreModule.
            if let Some(ty) = self
                .function
                .semantic
                .registers
                .get(&current.temp.index())
                .cloned()
            {
                self.function
                    .semantic
                    .registers
                    .insert(dst.temp.index(), ty);
            }
            self.emit(Instruction::Binary {
                dst,
                op: numeric
                    .map(BinaryOp::Numeric)
                    .unwrap_or_else(|| Self::lower_binary_op(op)),
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
                ProjectionKind::AppliedIndex {
                    index,
                    receiver,
                    write,
                    ..
                } => {
                    let (interface, method) = write;
                    self.lower_applied_operator(
                        interface,
                        receiver,
                        &method,
                        &[base, index, result],
                    )?;
                }
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
            Root::Cell { local, .. } => {
                let cell = self.alloc_temp(ValueType::HeapObject);
                self.emit(Instruction::LoadLocal { dst: cell, local });
                self.emit(Instruction::WriteCell {
                    cell,
                    value: result,
                });
                Ok(())
            }
            Root::Static { slot, .. } => {
                self.emit(Instruction::StoreModule { slot, src: result });
                Ok(())
            }
            Root::Value(_) => Err(MirLoweringError::UnsupportedStatement(
                "temporary value is not an assignment slot",
            )),
        }
    }

    fn read_projection(
        &mut self,
        base: MirValue,
        projection: &Projection,
    ) -> Result<MirValue, MirLoweringError> {
        if let ProjectionKind::AppliedIndex {
            index,
            receiver,
            read,
            ..
        } = &projection.kind
        {
            let method = self.protocol_method(Protocol::Index, 0)?;
            return self.lower_applied_operator(
                read.clone(),
                receiver.clone(),
                &method,
                &[base, *index],
            );
        }
        let dst = self.alloc_temp(projection.ty);
        match &projection.kind {
            ProjectionKind::AppliedIndex { .. } => unreachable!("handled applied projection"),
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
        Ok(dst)
    }
}
