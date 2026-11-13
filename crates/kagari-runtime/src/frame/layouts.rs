//! Resolve aggregate operands with the active frame's checked type arguments.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{ExecutionFrame, types::arguments::TypeArgument},
    module::{
        EnumVariantRef, StructLayoutRef, execution::aggregates::LayoutSite,
        linked_execution::layouts::AggregateLayout,
    },
    native::storage_type::StorageType,
};
use kagari_types::ty::Ty;
use std::{slice, sync::Arc};

impl ExecutionFrame {
    pub fn type_arguments(
        &self,
        runtime: &Runtime,
        types: &[Ty],
    ) -> Result<Vec<TypeArgument>, RuntimeError> {
        runtime.type_arguments(
            self.loaded(),
            self.environment()
                .map(|environment| environment.types.clone()),
            types,
        )
    }

    pub(super) fn element_contract(
        &self,
        runtime: &Runtime,
        element: &Ty,
    ) -> Result<Arc<StorageType>, RuntimeError> {
        let argument = runtime
            .type_arguments(
                self.loaded(),
                self.environment()
                    .map(|environment| environment.types.clone()),
                slice::from_ref(element),
            )?
            .pop()
            .ok_or_else(|| RuntimeError::module_validation("array element scope"))?;
        StorageType::prepare_scoped(argument, self.loaded()).map(Arc::new)
    }

    fn aggregate_layout(
        &self,
        runtime: &Runtime,
        prepared_site: Option<LayoutSite>,
    ) -> Result<&AggregateLayout, RuntimeError> {
        self.validate_runtime(runtime)?;
        let links = self
            .links
            .as_ref()
            .ok_or_else(|| RuntimeError::module_validation("missing function execution"))?;
        let layouts = links
            .layouts
            .as_deref()
            .ok_or_else(|| RuntimeError::module_validation("missing function layout operands"))?;
        // Prepared fields address the same table directly. Cold canonical aggregate
        // operations select their site from the current logical PC.
        let site = prepared_site
            .or_else(|| layouts.site(self.instruction_offset()))
            .ok_or_else(|| RuntimeError::module_validation("missing prepared layout site"))?;
        layouts.resolve(
            runtime,
            self.loaded(),
            self.environment
                .as_ref()
                .map(|environment| environment.facts()),
            links.applied_layouts.as_deref(),
            site,
        )
    }

    pub(super) fn field_layout(
        &self,
        runtime: &Runtime,
        site: LayoutSite,
    ) -> Result<StructLayoutRef, RuntimeError> {
        let AggregateLayout::Struct(layout) = self.aggregate_layout(runtime, Some(site))? else {
            return Err(RuntimeError::module_validation(
                "expected prepared struct layout",
            ));
        };
        Ok(layout.clone())
    }

    /// Resolve the checked struct operand at this frame's current program point.
    pub fn struct_layout(&self, runtime: &Runtime) -> Result<StructLayoutRef, RuntimeError> {
        let AggregateLayout::Struct(layout) = self.aggregate_layout(runtime, None)? else {
            return Err(RuntimeError::module_validation(
                "expected prepared struct layout",
            ));
        };
        Ok(layout.clone())
    }

    /// Resolve the checked enum operand at this frame's current program point.
    pub fn enum_variant(&self, runtime: &Runtime) -> Result<EnumVariantRef, RuntimeError> {
        let AggregateLayout::Enum(layout) = self.aggregate_layout(runtime, None)? else {
            return Err(RuntimeError::module_validation(
                "expected prepared enum layout",
            ));
        };
        Ok(layout.clone())
    }
}
