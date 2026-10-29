//! Prepared field operations share heap semantics across concrete and applied layouts.
use crate::{
    Runtime,
    frame::{
        ExecutionFrame, ExecutionStack,
        cursor::kernel::{RegionError, RegionExit},
    },
    gc::roots::RootSet,
    module::{
        StructLayoutRef,
        execution::{
            fields::{FieldAccess, PreparedFieldOperation},
            layout::Location,
        },
    },
    value::Value,
};
use kagari_bytecode::module::CallableTarget;
use kagari_types::reference::ReferenceAccess;

pub(super) enum FieldAction {
    Read,
    Write(Value),
}

impl FieldAction {
    fn execute_host(
        self,
        runtime: &Runtime,
        layout: &StructLayoutRef,
        slot: usize,
        base: Value,
        access: ReferenceAccess,
    ) -> Result<Option<RootSet>, RegionError> {
        let view = runtime.scoped_view(&base)?;
        match self {
            Self::Read => runtime
                .host_borrows
                .read_field(runtime, view, layout, slot, access)
                .map(Some)
                .map_err(RegionError::from),
            Self::Write(value) => {
                runtime
                    .host_borrows
                    .write_field(runtime, view, layout, slot, value)?;
                Ok(None)
            }
        }
    }

    /// Value admission precedes layout preparation and receiver access on writes.
    pub(super) fn validate(&self, runtime: &Runtime) -> Result<(), RegionError> {
        match self {
            Self::Write(value) if !value.is_default_heap_payload(runtime.gc()) => Err(
                RegionError::TypeMismatch("write_field expects default-storable value"),
            ),
            _ => Ok(()),
        }
    }

    pub(super) fn execute(
        self,
        runtime: &Runtime,
        layout: &StructLayoutRef,
        slot: usize,
        base: Value,
    ) -> Result<Option<Value>, RegionError> {
        let Value::Struct(id) = base else {
            return Err(RegionError::TypeMismatch(match self {
                Self::Read => "read_field expects struct value",
                Self::Write(_) => "write_field expects struct value",
            }));
        };
        match self {
            Self::Read => runtime
                .gc()
                .struct_get_slot(id, layout, slot)
                .map(Some)
                .ok_or(RegionError::TypeMismatch("struct layout or field mismatch")),
            Self::Write(value) => {
                runtime.gc().struct_set_slot(id, layout, slot, value)?;
                Ok(None)
            }
        }
    }
}

impl ExecutionFrame {
    fn prepare_field_storage(
        &self,
        runtime: &Runtime,
        index: usize,
        operation: &PreparedFieldOperation,
        layout: &StructLayoutRef,
    ) -> Result<(), RegionError> {
        self.links
            .as_ref()
            .ok_or_else(|| {
                runtime
                    .resources()
                    .quarantine("missing field execution links")
            })?
            .prepare_field(runtime, index, layout.clone(), operation.slot as usize)?;
        Ok(())
    }

    pub(super) fn prepared_field(&self, index: usize) -> Option<&PreparedFieldOperation> {
        let CallableTarget::Script(function) = self.target else {
            return None;
        };
        self.loaded
            .execution()
            .functions
            .get(function.index())?
            .fields
            .get(index)
    }
}

impl ExecutionStack<'_> {
    pub(super) fn execute_scoped_field(
        &self,
        runtime: &Runtime,
        index: usize,
    ) -> Result<RegionExit, RegionError> {
        // The closed cursor has ended. Layout application can allocate metadata,
        // but no operand-bank borrow survives it. The frame roots its environment.
        let frame = self.current()?;
        let operation = frame.prepared_field(index).ok_or_else(|| {
            runtime
                .resources()
                .quarantine("invalid prepared field ordinal")
        })?;
        let access = operation.access;
        let read = |location: Location| {
            runtime
                .resources()
                .frame_values
                .try_borrow()
                .map_err(|_| {
                    runtime
                        .resources()
                        .quarantine("field operands borrowed across transition")
                })?
                .with_location(frame.slots, location, |value| *value)
                .ok_or_else(|| runtime.resources().quarantine("invalid field operand"))
        };
        if operation.reference.is_none()
            && let Some(ty) = operation.scalar
        {
            let bits = if let FieldAccess::Write { value } = access {
                Some(
                    runtime
                        .resources()
                        .frame_values
                        .try_borrow()
                        .map_err(|_| {
                            runtime
                                .resources()
                                .quarantine("field operands borrowed across transition")
                        })?
                        .scalar_location(frame.slots, value)
                        .ok_or_else(|| {
                            runtime
                                .resources()
                                .quarantine("invalid scalar field operand")
                        })?
                        .1,
                )
            } else {
                None
            };
            let layout = frame.field_layout(runtime, operation.layout_site)?;
            frame.prepare_field_storage(runtime, index, operation, &layout)?;
            let Value::Struct(id) = read(operation.base)? else {
                return Err(RegionError::TypeMismatch(match access {
                    FieldAccess::Read { .. } => "read_field expects struct value",
                    FieldAccess::Write { .. } => "write_field expects struct value",
                }));
            };
            let slot = operation.slot;
            drop(frame);
            let mut frame = self.current_mut()?;
            match access {
                FieldAccess::Read { dst } => {
                    let bits = runtime
                        .gc()
                        .struct_scalar(id, &layout, slot as usize, ty)
                        .ok_or(RegionError::TypeMismatch("struct layout or field mismatch"))?;
                    runtime
                        .resources()
                        .frame_values
                        .try_borrow_mut()
                        .map_err(|_| {
                            runtime
                                .resources()
                                .quarantine("field operands borrowed during publication")
                        })?
                        .set_scalar_location(
                            &runtime.gc,
                            frame.slots,
                            dst,
                            dst.representation,
                            bits,
                        )
                        .ok_or_else(|| {
                            runtime.resources().quarantine("invalid field destination")
                        })?;
                }
                FieldAccess::Write { .. } => runtime.gc().set_struct_scalar(
                    id,
                    &layout,
                    slot as usize,
                    ty,
                    bits.expect("prepared scalar write"),
                )?,
            }
            frame.completed = frame.executing;
            return Ok(RegionExit::Safepoint);
        }
        let action = match access {
            FieldAccess::Read { .. } => FieldAction::Read,
            FieldAccess::Write { value } => FieldAction::Write(read(value)?),
        };
        action.validate(runtime)?;
        let layout = frame.field_layout(runtime, operation.layout_site)?;
        if operation.reference.is_none() {
            frame.prepare_field_storage(runtime, index, operation, &layout)?;
        }
        let base = read(operation.base)?;
        let mut result_root = None;
        let result = if let Some(access) = operation.reference {
            result_root =
                action.execute_host(runtime, &layout, operation.slot as usize, base, access)?;
            result_root
                .as_ref()
                .map(|root| {
                    root.get(runtime.gc(), 0)
                        .ok_or(RegionError::TypeMismatch("invalid inline field result"))
                })
                .transpose()?
        } else {
            action.execute(runtime, &layout, operation.slot as usize, base)?
        };
        drop(frame);
        let mut frame = self.current_mut()?;
        if let Some(value) = result {
            let FieldAccess::Read { dst } = access else {
                return Err(runtime
                    .resources()
                    .quarantine("field access contract mismatch")
                    .into());
            };

            runtime
                .resources()
                .frame_values
                .try_borrow_mut()
                .map_err(|_| {
                    runtime
                        .resources()
                        .quarantine("field operands borrowed during publication")
                })?
                .set_location(&runtime.gc, frame.slots, dst, value)
                .ok_or_else(|| runtime.resources().quarantine("invalid field destination"))?;
        }
        drop(result_root);
        frame.completed = frame.executing;
        Ok(RegionExit::Safepoint)
    }
}
