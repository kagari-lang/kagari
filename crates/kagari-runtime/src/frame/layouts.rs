//! Resolve aggregate operands with the active frame's checked type arguments.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{ExecutionFrame, types::arguments::TypeArgument},
    gc::HeapObjectId,
    module::{
        EnumVariantRef, StructLayoutRef, execution::aggregates::LayoutSite,
        linked_execution::layouts::AggregateLayout,
    },
    native::storage_type::StorageType,
    value::Value,
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

    fn element_contract(
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

    /// Access backend execution transport through its original semantic owner.
    ///
    /// # Safety
    /// Keep every managed input in this runtime's roots or active traced storage
    /// until publication. Protect extracted results before collection, callbacks
    /// or mutation can remove their last owner. Returned loans must end before
    /// reentry; scoped host resources also require their original host lifetime.
    /// This entry preserves validation but cannot revive expired direct references.
    #[doc(hidden)]
    pub unsafe fn alloc_array_transport(
        &self,
        runtime: &Runtime,
        element: &Ty,
        elements: Vec<Value>,
    ) -> Result<HeapObjectId, RuntimeError> {
        self.alloc_array(runtime, element, elements)
    }

    pub(crate) fn alloc_array(
        &self,
        runtime: &Runtime,
        element: &Ty,
        elements: Vec<Value>,
    ) -> Result<HeapObjectId, RuntimeError> {
        runtime.validate_heap_payloads(&elements)?;
        runtime
            .gc
            .alloc_array_with_contract(self.element_contract(runtime, element)?, elements)
    }

    /// Access backend execution transport through its original semantic owner.
    ///
    /// # Safety
    /// Keep every managed input in this runtime's roots or active traced storage
    /// until publication. Protect extracted results before collection, callbacks
    /// or mutation can remove their last owner. Returned loans must end before
    /// reentry; scoped host resources also require their original host lifetime.
    /// This entry preserves validation but cannot revive expired direct references.
    #[doc(hidden)]
    pub unsafe fn alloc_array_repeat_transport(
        &self,
        runtime: &Runtime,
        element: &Ty,
        value: Value,
        count: u64,
    ) -> Result<HeapObjectId, RuntimeError> {
        self.alloc_array_repeat(runtime, element, value, count)
    }

    pub(crate) fn alloc_array_repeat(
        &self,
        runtime: &Runtime,
        element: &Ty,
        value: Value,
        count: u64,
    ) -> Result<HeapObjectId, RuntimeError> {
        runtime.validate_heap_payloads(slice::from_ref(&value))?;
        runtime.gc.alloc_array_repeat_with_contract(
            self.element_contract(runtime, element)?,
            value,
            count,
        )
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
            self.environment.as_ref(),
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
