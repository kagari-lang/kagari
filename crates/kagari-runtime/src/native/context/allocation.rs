//! Native results publish concrete providers and stored relationship edges.
use crate::{
    error::RuntimeError,
    execution_metadata::{
        applications::{AdmittedApplication, ApplicationRecord},
        managed::sources::MetadataSource,
    },
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        context::NativeCall,
        conversion::context::{ConversionContext, native::NativeContract},
        future::ColdFuture,
        payload::data::NativeData,
        storage::{NativeStorage, StorageContext, initializer::NativeInit},
        stored_selection::StoredSelection,
    },
    value::view::ValueView,
};
use kagari_gc::{
    error::GcError,
    object::{Gc, GcObject},
};
use kagari_types::ty::Ty;
use std::{fmt::Debug, slice, sync::Arc};

impl<'call> NativeCall<'call> {
    /// Allocate the checked factory of this call's declared nominal Native result.
    /// The resulting view belongs to this call's Native suffix, without a Host root.
    pub fn allocate_result(&mut self) -> NativeResult<ValueView<'call>> {
        let storage = self.result_storage()?;
        let owner = self.owner;
        let function = self.function;
        let applied = &function.type_signature()?.result;
        self.convert_value(|conversion| {
            storage.create(&mut StorageContext {
                conversion,
                owner,
                applied,
                selected: &function.selected,
            })
        })
    }

    /// Construct fixed Rust data and retain the requested declared callbacks as
    /// actual metadata edges. Traced providers use branded construction instead.
    pub fn allocate_result_payload<S: NativeData>(
        &mut self,
        payload: S,
        slots: &[usize],
    ) -> NativeResult<ValueView<'call>> {
        let storage = self.result_storage()?;
        let owner = self.owner;
        let applied = &self.function.type_signature()?.result;
        let selections = self.result_selections(slots)?;
        self.convert_value(|conversion| {
            let application = prepare_selections(conversion, owner, selections)?;
            conversion.allocate_native_data(
                NativeContract {
                    storage: &storage,
                    owner,
                    applied,
                },
                payload,
                application.as_ref(),
            )
        })
    }

    /// Initialize a concrete traced result inside the GC brand. The provider's
    /// reviewed scanner owns its script edges; requested callbacks belong to the
    /// Native header. No detached provider or Host root crosses this boundary.
    pub fn allocate_result_with<S: GcObject + Debug>(
        &mut self,
        slots: &[usize],
        initialize: impl for<'loan, 'source, 'gc> FnOnce(
            &mut NativeInit<'loan, 'source, 'gc>,
        ) -> Result<Gc<'gc, S>, GcError>,
    ) -> NativeResult<ValueView<'call>> {
        let storage = self.result_storage()?;
        let owner = self.owner;
        let applied = &self.function.type_signature()?.result;
        let selections = self.result_selections(slots)?;
        self.convert_value(|conversion| {
            let application = prepare_selections(conversion, owner, selections)?;
            conversion.allocate_native_with(
                NativeContract {
                    storage: &storage,
                    owner,
                    applied,
                },
                application.as_ref(),
                initialize,
            )
        })
    }

    /// Build the nominal Future result through this Native call's ordinary
    /// conversion/result handoff. Captures remain in their original source until
    /// the cold computation's actual edges are completely initialized.
    pub(in crate::native) fn allocate_future_result(
        &mut self,
        recipe: ColdFuture,
        captures: &[ValueView<'_>],
    ) -> NativeResult<ValueView<'call>> {
        let applied = &self.function.type_signature()?.result;
        self.convert_value(move |cx| cx.allocate_future(applied, recipe, captures))
    }

    fn result_selections(&self, slots: &[usize]) -> NativeResult<Vec<StoredSelection>> {
        let mut selections = Vec::new();
        selections
            .try_reserve_exact(slots.len())
            .map_err(|_| RuntimeError::resource_limit("stored Native selection capacity"))?;
        for (index, slot) in slots.iter().enumerate() {
            if index.is_multiple_of(1024) {
                self.poll()?;
            }
            selections.push(StoredSelection::new(self.owner, self.selected_at(*slot)?)?);
        }
        Ok(selections)
    }

    fn result_storage(&self) -> NativeResult<NativeStorage> {
        self.runtime.resources.ensure_synchronous_thread()?;
        self.runtime.resources.ensure_execution_allowed()?;
        let Ty::NativeObject(nominal) = self.function.type_signature()?.result.ty() else {
            return Err(RuntimeError::module_validation(
                "Native allocation requires a nominal result type",
            ));
        };
        self.runtime
            .native_entries
            .storage
            .get_id(nominal.declaration)
            .cloned()
            .ok_or_else(|| {
                RuntimeError::module_validation("native result storage is not installed")
            })
    }
}

fn prepare_selections(
    conversion: &mut ConversionContext<'_>,
    owner: &ExecutableModule,
    selections: Vec<StoredSelection>,
) -> NativeResult<Option<AdmittedApplication>> {
    if selections.is_empty() {
        return Ok(None);
    }
    let execution = conversion.execution_id()?;
    let revision = MetadataSource::Revision {
        owner,
        source: conversion.owner_origin(),
    };
    let pair = conversion
        .owner_metadata()
        .map(|metadata| [revision, metadata]);
    let sources = pair
        .as_ref()
        .map_or(slice::from_ref(&revision), |pair| pair.as_slice());
    conversion
        .runtime_mut()
        .alloc_application(
            execution,
            ApplicationRecord::StoredSelections(Arc::from(selections)),
            sources,
        )
        .map(Some)
}
