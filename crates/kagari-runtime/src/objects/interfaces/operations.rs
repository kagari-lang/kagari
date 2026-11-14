//! Receiver dictionaries are cached by actual interface providers, never by IDs.
use crate::{
    Runtime,
    error::RuntimeError,
    execution::roots::{SessionId, Slots},
    execution_metadata::{
        groups::AdmittedOperationGroup,
        managed::sources::{MetadataError, MetadataSource},
    },
    objects::interfaces::description::InterfaceDescription,
    resource::ResourceState,
    value::interfaces::InterfaceObject,
};
use kagari_gc::object::Gc;
use std::sync::Arc;

impl Runtime {
    pub(crate) fn interface_receiver_operations(
        &mut self,
        execution: SessionId,
        description: &Arc<InterfaceDescription>,
        slot: usize,
        protected: &[MetadataSource<'_>],
    ) -> Result<Option<AdmittedOperationGroup>, RuntimeError> {
        let method = description
            .methods
            .get(slot)
            .and_then(Option::as_ref)
            .ok_or_else(|| RuntimeError::module_validation("invalid interface method slot"))?;
        let required = method.requires_receiver_operations;
        // Even a method that needs no dictionary authenticates its original
        // relationship. Pure facts alone never authorize a call.
        let cached = with_interface(self, description, protected, |slots, object, _| {
            if !required {
                return Ok(None);
            }
            InterfaceObject::receiver_operations(slots.scope(), object)
                .map(|dictionary| AdmittedOperationGroup::publish(slots, execution, dictionary))
                .transpose()
                .map_err(MetadataError::Memory)
        })?;
        if !required || cached.is_some() {
            return Ok(cached);
        }
        // All environment/body dependencies come from this receiver's retained
        // relationship. There is no current-publication or semantic-ID fallback.
        let dictionary =
            self.bind_table_operations(execution, &description.receiver_table, protected)?;
        with_interface(self, description, protected, |slots, object, _| {
            let dictionary = dictionary.resolve(slots)?;
            InterfaceObject::publish_receiver_operations(
                slots.scope(),
                object,
                description,
                dictionary,
            )?;
            Ok(())
        })?;
        Ok(Some(dictionary))
    }
}

fn with_interface<R>(
    runtime: &mut Runtime,
    description: &Arc<InterfaceDescription>,
    protected: &[MetadataSource<'_>],
    read: impl for<'gc> FnOnce(
        &mut Slots<'_, 'gc>,
        Gc<'gc, InterfaceObject>,
        &ResourceState,
    ) -> Result<R, MetadataError>,
) -> Result<R, RuntimeError> {
    runtime.validate_executable_module(&description.implementation)?;
    runtime.with_metadata_slots(protected, |slots, graph, resources| {
        let object = graph
            .interface(slots.scope(), description, resources)?
            .ok_or_else(|| {
                RuntimeError::module_validation("interface relationship is not protected")
            })?;
        read(slots, object, resources)
    })
}
