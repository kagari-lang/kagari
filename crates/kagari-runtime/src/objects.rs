use crate::{
    RootedInterfaceMethod, Runtime,
    error::{RuntimeError, trap::TrapReason},
    execution_metadata::managed::{ProtectedMetadata, sources::MetadataSource},
    frame::types::arguments::TypeArgument,
    module::ExecutableModule,
    native::{
        conversion::context::{ConversionContext, inputs::ValueInput},
        storage_type::StorageType,
    },
    objects::{
        application::MethodApplicationInput,
        interfaces::{InterfaceRequest, description::InterfaceDescription},
    },
    value::{carriers::RootedValue, interfaces::InterfaceObject},
};
use kagari_common::identity::table::DefinitionId;
use kagari_gc::error::GcError;
use kagari_types::ty::{NominalTy, Ty};
use std::sync::Arc;
mod adapters;
mod aggregates;
pub(crate) mod application;
mod calls;
pub(crate) mod constructions;
pub(crate) mod interfaces;
pub(crate) mod invocation;
pub(crate) mod method;
pub(crate) mod method_view;
mod operations;

impl Runtime {
    /// Construct a fixed array from genuine Host roots and return independent
    /// Host ownership. Internal calls use source views through ConversionContext.
    pub fn alloc_array(
        &mut self,
        owner: &ExecutableModule,
        element: Ty,
        elements: &[RootedValue],
    ) -> Result<RootedValue, RuntimeError> {
        let mut cx = ConversionContext::new(self, owner)?;
        let contract = Arc::new(StorageType::prepare(element, owner)?);
        let value = cx.allocate_array_inputs(contract, elements.iter().map(ValueInput::Root))?;
        cx.retain(&value)
    }

    pub fn alloc_array_repeat(
        &mut self,
        owner: &ExecutableModule,
        element: Ty,
        value: &RootedValue,
        count: usize,
    ) -> Result<RootedValue, RuntimeError> {
        let mut cx = ConversionContext::new(self, owner)?;
        let contract = Arc::new(StorageType::prepare(element, owner)?);
        let count =
            u64::try_from(count).map_err(|_| RuntimeError::resource_limit("array length"))?;
        let value = cx.allocate_array_repeat_input(contract, ValueInput::Root(value), count)?;
        cx.retain(&value)
    }

    /// Box a concrete script value or durable host root behind a verified table.
    /// The resulting heap object retains the table's entire execution version.
    pub fn make_interface(
        &mut self,
        implementation: &ExecutableModule,
        table_index: usize,
        data: &RootedValue,
    ) -> Result<RootedValue, RuntimeError> {
        let arguments = implementation
            .bytecode
            .interface_tables
            .get(table_index)
            .ok_or_else(|| RuntimeError::module_validation("invalid interface table"))?
            .arguments
            .clone();
        let arguments = self.resolve_type_arguments(implementation, &arguments)?;
        let mut conversion = ConversionContext::new(self, implementation)?;
        let data = conversion.import(data)?;
        let value = conversion.make_interface(
            InterfaceRequest {
                implementation,
                table: table_index,
                arguments: &arguments,
                environment: None,
                view: false,
            },
            &data,
        )?;
        conversion.retain(&value)
    }

    /// Resolve a declared method in the actual interface's retained relationship.
    /// The ID belongs to the receiver's retained product definition table; foreign
    /// IDs and unavailable methods return ModuleValidation. Parent relationships
    /// use checked identity admission, without importing or specializing a table.
    /// The method body remains a live ordinary selection until call admission.
    pub fn resolve_interface_method(
        &mut self,
        value: &RootedValue,
        method: &DefinitionId,
    ) -> Result<RootedInterfaceMethod, RuntimeError> {
        let description = self.interface_description(value)?;
        let method = *method;
        description
            .receiver_table
            .owner
            .definitions()
            .resolve(method)
            .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
        let slot = description.methods.iter().position(|binding| {
            binding
                .as_ref()
                .is_some_and(|binding| binding.method == method)
        });
        if let Some(slot) = slot {
            return self.prepare_interface_method_slot(
                value,
                description.interface_type(),
                slot,
                MethodApplicationInput::plain(&[]),
            );
        }
        let declaration = description
            .receiver_table
            .owner
            .definitions()
            .parent(method)
            .map_err(|error| RuntimeError::module_validation(error.to_string()))?
            .ok_or_else(|| RuntimeError::module_validation("interface method parent"))?;
        // Parent types are runtime-canonical; the requested method and its
        // validated lexical parent still belong to the retained product table.
        let declaration = description
            .receiver_table
            .owner
            .identity_admission()
            .runtime_id(declaration)?;
        let parent = description
            .parents
            .iter()
            .find(|parent| parent.interface.declaration == declaration)
            .ok_or_else(|| RuntimeError::module_validation("interface method unavailable"))?;
        let parent =
            self.upcast_interface(value, description.interface_type(), &parent.interface)?;
        let description = self.interface_description(&parent)?;
        let slot = description
            .methods
            .iter()
            .position(|binding| {
                binding
                    .as_ref()
                    .is_some_and(|binding| binding.method == method)
            })
            .ok_or_else(|| RuntimeError::module_validation("interface method unavailable"))?;
        self.prepare_interface_method_slot(
            &parent,
            description.interface_type(),
            slot,
            MethodApplicationInput::plain(&[]),
        )
    }

    pub(crate) fn interface_description(
        &mut self,
        root: &RootedValue,
    ) -> Result<Arc<InterfaceDescription>, RuntimeError> {
        self.with_metadata_graph(&[MetadataSource::RootedInterface(root)], |cx, graph, _| {
            let Some(ProtectedMetadata::Interface(object)) = graph.single_root() else {
                return Err(GcError::Representation.into());
            };
            Ok(cx.read(object, |object| object.description().clone()))
        })
    }

    /// Creates a parent interface view using already compiled tables from the
    /// receiver's retained execution version. It never specializes at runtime.
    pub fn upcast_interface(
        &mut self,
        root: &RootedValue,
        source: &NominalTy,
        target: &NominalTy,
    ) -> Result<RootedValue, RuntimeError> {
        let owner = self
            .memory
            .scope(&self.resources, |cx, _| {
                let value = root.get(cx)?;
                let object =
                    cx.downcast::<InterfaceObject>(value.object().ok_or(GcError::Representation)?)?;
                Ok(cx.read(object, |object| object.description().implementation.clone()))
            })
            .map_err(|error| match error {
                GcError::ForeignHeap
                | GcError::InvalidRoot
                | GcError::Index
                | GcError::Representation => RuntimeError::from(TrapReason::InvalidInterfaceUpcast),
                error => self.managed_memory_error(error),
            })?;
        let mut conversion = ConversionContext::new(self, &owner)?;
        let value = conversion.import(root)?;
        let parent = conversion.upcast_interface(&value, source, target)?;
        if source == target {
            return Ok(root.clone());
        }
        conversion.retain(&parent)
    }

    /// Resolves a trait declaration's verified method ordinal without a
    /// name or declaration search during script dispatch.
    pub fn resolve_interface_method_slot(
        &mut self,
        value: &RootedValue,
        interface: &NominalTy,
        slot: usize,
        arguments: &[TypeArgument],
    ) -> Result<RootedInterfaceMethod, RuntimeError> {
        self.prepare_interface_method_slot(
            value,
            interface,
            slot,
            MethodApplicationInput::plain(arguments),
        )
    }

    pub(crate) fn prepare_interface_method_slot(
        &mut self,
        value: &RootedValue,
        interface: &NominalTy,
        slot: usize,
        application: MethodApplicationInput<'_>,
    ) -> Result<RootedInterfaceMethod, RuntimeError> {
        let description = self.interface_description(value)?;
        let parent = self.upcast_interface(value, description.interface_type(), interface)?;
        let method = RootedInterfaceMethod::from_interface(self, parent, slot)?;
        self.apply_interface_method(method, application)
    }

    pub fn validate_interface_method_arguments(
        &mut self,
        method: &RootedInterfaceMethod,
        arguments: &[RootedValue],
    ) -> Result<(), RuntimeError> {
        let view = method.view(self)?;
        let owner = view.proof_implementation().clone();
        let mut conversion = ConversionContext::new(self, &owner)?;
        conversion.validate_interface_arguments(&view, arguments)
    }

    pub fn validate_interface_method_result(
        &mut self,
        method: &RootedInterfaceMethod,
        result: &RootedValue,
    ) -> Result<(), RuntimeError> {
        let view = method.view(self)?;
        let owner = view.proof_implementation().clone();
        let mut conversion = ConversionContext::new(self, &owner)?;
        let result = conversion.import(result)?;
        view.validate_result(conversion.runtime_mut(), &result)
    }
}
