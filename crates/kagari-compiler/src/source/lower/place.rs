use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_hir::{
    hir::{expr::ops::BinaryOp as HirBinaryOp, ids::PlaceId, place::PlaceKind},
    language::semantics::ProtocolSemantics,
    resolver::resolved::ResolvedName,
    types::{TypeId, abi::lower_nominal_type},
};

use kagari_abi::{
    language::Protocol, numeric::NumericOperation, operations::BinaryOp, representation::ValueType,
    scalar::BuiltinType, types::NominalAbiType,
};
use kagari_common::identity;

use kagari_mir::{
    ids::LocalId,
    instruction::{
        AggregateFieldRef, CallTarget, Instruction, InterfaceCallContract, MirValue, PathRef,
        ValueBuffer,
    },
};
use std::ops::ControlFlow;

pub(super) struct PreparedPlace {
    host_path: Option<PathRef>,
    dynamic_args: ValueBuffer,
    root: Root,
    projections: Vec<Projection>,
}

enum Root {
    Local { local: LocalId, ty: ValueType },
    Cell { local: LocalId, ty: ValueType },
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
    InterfaceIndex {
        index: MirValue,
        read: NominalAbiType,
        write: Option<NominalAbiType>,
    },
}

impl FunctionLowerer<'_, '_> {
    /// Capture identity-bearing roots and dynamic indexes once, before RHS code.
    /// `None` means target evaluation terminated control flow; no location exists.
    pub(super) fn prepare_place(
        &mut self,
        id: PlaceId,
    ) -> Result<Option<PreparedPlace>, MirLoweringError> {
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
                Root::Cell { local, ty } => {
                    let cell = self.alloc_temp(ValueType::HeapObject);
                    self.emit(Instruction::LoadLocal { dst: cell, local });
                    let dst = self.alloc_temp(ty);
                    self.emit(Instruction::ReadCell { dst, cell });
                    dst
                }
            };
            for projection in &prepared.projections {
                value = self.read_projection(value, projection);
            }
            let dynamic_args = match self.lower_host_path_arguments(&checked.dynamic_arguments)? {
                ControlFlow::Break(_) => return Ok(None),
                ControlFlow::Continue(values) => values,
            };
            let path = PathRef {
                declaration: Some(checked.declaration),
                contract_fingerprint: checked
                    .contract
                    .fingerprint()
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
        let place = self.analyzed.lowered.module.place(id).clone();
        match place.kind {
            PlaceKind::Name(_) => {
                let local = self.lookup_binding(self.place_root_resolution(id)?)?;
                let ty = self.place_type(id)?;
                let is_cell = matches!(self.place_root_resolution(id)?, ResolvedName::Local(local_id) if self.cell_locals.contains(&local_id));
                let root = if projected
                    && matches!(
                        self.analyzed.typed.type_table.place_type(id),
                        Some(
                            TypeId::Trait(_)
                                | TypeId::Struct(_)
                                | TypeId::Array(_, _)
                                | TypeId::Map { .. }
                                | TypeId::Set(_, _)
                        )
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
            PlaceKind::Expr(expr) => {
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
            PlaceKind::Field { base, .. } => {
                let field = self
                    .analyzed
                    .typed
                    .type_table
                    .place_field(id)
                    .ok_or(MirLoweringError::MissingBinding("checked field assignment"))?;
                let Some(mut place) = self.prepare_place_inner(base, true)? else {
                    return Ok(None);
                };
                let receiver_ty = self
                    .analyzed
                    .typed
                    .type_table
                    .place_type(base)
                    .ok_or(MirLoweringError::UnresolvedPlace(base))?;
                place.projections.push(Projection {
                    kind: ProjectionKind::Field(self.aggregate_field_ref(field, &receiver_ty)?),
                    ty: self.place_type(id)?,
                    tuple_base: false,
                });
                Ok(Some(place))
            }
            PlaceKind::Index { base, index } => {
                let Some(mut place) = self.prepare_place_inner(base, true)? else {
                    return Ok(None);
                };
                let receiver = self
                    .analyzed
                    .typed
                    .type_table
                    .place_type(base)
                    .ok_or(MirLoweringError::UnresolvedPlace(base))?;
                let receiver = self
                    .planner
                    .arguments(
                        &[receiver],
                        &self.instance.substitution,
                        self.function.debug.source_span,
                    )?
                    .remove(0);
                if let Some(item) = receiver.list_item() {
                    let mut read = Protocol::Index.nominal();
                    read.arguments.push(TypeId::Builtin(BuiltinType::USize));
                    read.associated_types.insert(
                        identity::associated_type_id(&read.declaration, "Output"),
                        item.clone(),
                    );
                    let write = if receiver.writable_list() {
                        let mut write = Protocol::MutableList.nominal();
                        write.arguments.push(item.clone());
                        Some(lower_nominal_type(&write))
                    } else {
                        None
                    };
                    let index = self.lower_expr(index)?;
                    if self.current_block_terminated() {
                        return Ok(None);
                    }
                    place.projections.push(Projection {
                        kind: ProjectionKind::InterfaceIndex {
                            index,
                            read: lower_nominal_type(&read),
                            write,
                        },
                        ty: self.place_type(id)?,
                        tuple_base: false,
                    });
                    return Ok(Some(place));
                }
                if let Some(interface) = self.analyzed.typed.type_table.place_index(id).cloned() {
                    let receiver_ty = self
                        .analyzed
                        .typed
                        .type_table
                        .place_type(base)
                        .ok_or(MirLoweringError::UnresolvedPlace(base))?;
                    let mut value = match place.root {
                        Root::Value(value) => value,
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
                        value = self.read_projection(value, projection);
                    }
                    let index = self.lower_expr(index)?;
                    if self.current_block_terminated() {
                        return Ok(None);
                    }
                    let method = self.protocol_method(Protocol::Index, 0)?;
                    let value = self.lower_applied_operator(
                        interface,
                        receiver_ty,
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
                ProjectionKind::InterfaceIndex { index, write, .. } => {
                    let interface =
                        write.ok_or(MirLoweringError::MissingBinding("writable list interface"))?;
                    let slot = self
                        .planner
                        .catalog
                        .trait_(&interface.declaration)
                        .and_then(|contract| {
                            contract.methods.iter().find(|method| method.name == "set")
                        })
                        .ok_or(MirLoweringError::MissingBinding("checked list setter"))?
                        .slot;
                    self.emit(Instruction::Call {
                        dst: None,
                        callee: CallTarget::InterfaceMethod(Box::new(InterfaceCallContract {
                            interface,
                            method_slot: slot as u32,
                        })),
                        args: [base, index, result].into_iter().collect(),
                    });
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
            Root::Value(_) => Err(MirLoweringError::UnsupportedStatement(
                "temporary value is not an assignment slot",
            )),
        }
    }

    fn read_projection(&mut self, base: MirValue, projection: &Projection) -> MirValue {
        let dst = self.alloc_temp(projection.ty);
        match &projection.kind {
            ProjectionKind::InterfaceIndex { index, read, .. } => self.emit(Instruction::Call {
                dst: Some(dst),
                callee: CallTarget::InterfaceMethod(Box::new(InterfaceCallContract {
                    interface: read.clone(),
                    method_slot: 0,
                })),
                args: [base, *index].into_iter().collect(),
            }),

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
