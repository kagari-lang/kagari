//! Host aggregate roots retain data and immutable layouts without executable leases.
use crate::{
    Runtime,
    error::{RuntimeError, trap::TrapReason},
    error_trace::ErrorTrace,
    frame::types::compatibility::TypeView,
    module::{EnumVariantRef, StructLayoutRef, layout_storage::StructStorageLayout},
    value::{
        EnumTag,
        aggregates::{enumerations::EnumObject, structures::StructObject},
        carriers::{RootedValue, ScopedValue},
        properties::ValueProperties,
    },
    value_check::scoped::{self, CheckError},
};
use kagari_gc::error::GcError;
use std::sync::Arc;

const CHUNK: usize = 1024;

enum Description {
    Struct(Arc<StructStorageLayout>),
    Enum(EnumVariantRef),
}

impl Description {
    fn len(&self) -> usize {
        match self {
            Self::Struct(description) => description.fields.len(),
            Self::Enum(layout) => layout.variant().payload.len(),
        }
    }

    fn field(&self, index: usize) -> Option<TypeView<'_>> {
        match self {
            Self::Struct(description) => {
                let (ty, environment) = description.semantic.field_type(index)?;
                Some(TypeView::new(
                    ty,
                    description.semantic.module(),
                    environment,
                ))
            }
            Self::Enum(layout) => {
                let (ty, environment) = layout.payload_type(index)?;
                Some(TypeView::new(ty, layout.module(), environment))
            }
        }
    }
}

impl Runtime {
    /// Construct a declared struct from genuine Host roots. Layout provenance,
    /// field count and semantic types are checked before allocation. A plain data
    /// layout needs no live executable revision; the result owns one Host root.
    pub fn alloc_struct(
        &mut self,
        layout: StructLayoutRef,
        fields: &[RootedValue],
    ) -> Result<RootedValue, RuntimeError> {
        self.validate_aggregate_sources(fields)?;
        if !layout.module().belongs_to(self.host.owner()) {
            return Err(RuntimeError::module_validation(
                "struct layout belongs to a different runtime",
            ));
        }
        let description = self.structure_layouts.prepare(layout)?;
        self.allocate_host_aggregate(Description::Struct(description), fields)
    }

    /// Construct a declared enum from genuine Host roots. Error-origin capture
    /// follows semantic payload admission and occurs outside all storage loans.
    /// The result retains its data and immutable layout, without a body lease.
    pub fn alloc_enum(
        &mut self,
        tag: EnumTag,
        fields: &[RootedValue],
    ) -> Result<RootedValue, RuntimeError> {
        self.validate_aggregate_sources(fields)?;
        let EnumTag::Declared(layout) = tag;
        if !layout.module().belongs_to(self.host.owner()) {
            return Err(RuntimeError::module_validation(
                "enum layout belongs to a different runtime",
            ));
        }
        self.allocate_host_aggregate(Description::Enum(layout), fields)
    }

    fn validate_aggregate_sources(&mut self, fields: &[RootedValue]) -> Result<(), RuntimeError> {
        self.resources.ensure_synchronous_thread()?;
        self.resources.ensure_execution_allowed()?;
        let resources = &self.resources;
        let result = self.memory.scope(&self.resources, |cx, _| {
            Ok((|| -> Result<(), CheckError> {
                for (index, field) in fields.iter().enumerate() {
                    if index.is_multiple_of(CHUNK) {
                        resources.poll_closed()?;
                    }
                    let value = field
                        .get(cx)
                        .map_err(|_| RuntimeError::from(TrapReason::InvalidOrForeignHeapPayload))?;
                    if !ValueProperties::of(cx, value)?.storable {
                        return Err(
                            RuntimeError::from(TrapReason::InvalidOrForeignHeapPayload).into()
                        );
                    }
                }
                Ok(())
            })())
        });
        result
            .map_err(|error| self.managed_memory_error(error))?
            .map_err(|error| {
                self.resources
                    .admit_execution_error(error.into_runtime(self))
            })
    }

    fn allocate_host_aggregate(
        &mut self,
        description: Description,
        fields: &[RootedValue],
    ) -> Result<RootedValue, RuntimeError> {
        if fields.len() != description.len() {
            return Err(TrapReason::InvalidHeapTargetIndexOrPayload.into());
        }
        let Self {
            memory,
            host,
            host_borrows,
            resources,
            ..
        } = self;
        let result = memory.scope(resources, |cx, _| {
            Ok((|| -> Result<(), CheckError> {
                for (index, field) in fields.iter().enumerate() {
                    if index.is_multiple_of(CHUNK) {
                        resources.poll_closed()?;
                    }
                    let value = field.get(cx)?;
                    let expected = description.field(index).ok_or(GcError::Representation)?;
                    if !scoped::matches(cx, value, expected, host, host_borrows, resources)? {
                        return Err(RuntimeError::from(
                            TrapReason::InvalidHeapTargetIndexOrPayload,
                        )
                        .into());
                    }
                }
                Ok(())
            })())
        });
        result
            .map_err(|error| self.managed_memory_error(error))?
            .map_err(|error| {
                self.resources
                    .admit_execution_error(error.into_runtime(self))
            })?;
        // The caller's genuine roots protect the unchanged inputs. Capturing only
        // detached frame metadata cannot execute script or mutate these values.
        let origin =
            matches!(&description, Description::Enum(layout) if layout.variant().reports_failure)
                .then(|| ErrorTrace::capture(&self.resources));
        let resources = &self.resources;
        let result = self.memory.scope(&self.resources, |cx, _| {
            Ok((|| -> Result<RootedValue, CheckError> {
                let mut values = Vec::new();
                values.try_reserve_exact(fields.len())?;
                for (index, field) in fields.iter().enumerate() {
                    if index.is_multiple_of(CHUNK) {
                        resources.poll_closed()?;
                    }
                    values.push(field.get(cx)?);
                }
                let value = match description {
                    Description::Struct(description) => ScopedValue::managed(StructObject::alloc(
                        cx,
                        description,
                        &values,
                        resources.candidate_origin(),
                    )?),
                    Description::Enum(layout) => {
                        ScopedValue::managed(EnumObject::alloc(cx, layout, &values, origin)?)
                    }
                };
                resources.poll_closed()?;
                Ok(value.retain(cx)?)
            })())
        });
        result
            .map_err(|error| self.managed_memory_error(error))?
            .map_err(|error| {
                self.resources
                    .admit_execution_error(error.into_runtime(self))
            })
    }
}
