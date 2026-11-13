//! Interface boxing consumes checked descriptions and protected receiver values.
use crate::{
    Runtime,
    error::{RuntimeError, trap::TrapReason},
    execution::roots::{ExecutionRoots, Slots},
    execution_metadata::managed::sources::{MetadataError, MetadataSource},
    frame::types::{TypeEnvironment, compatibility::TypeView},
    native::{binding::NativeResult, conversion::context::ConversionContext},
    objects::{
        constructions::ConstructionEvidence,
        interfaces::{InterfaceRequest, PreparedInterface},
    },
    value::{
        carriers::ScopedValue, interfaces::InterfaceObject, kind::ObjectKind,
        properties::ValueProperties, view::ValueView,
    },
    value_check::scoped::{self, CheckError},
};

use kagari_bytecode::instruction::ConstructionTarget;
use kagari_contract::callable::construction::evidence::ConstructionRequirement;
use kagari_gc::{error::GcError, object::Gc};
use kagari_types::ty::{NominalTy, Ty};

impl<'call> ConversionContext<'call> {
    /// Prepare a checked table application and publish the receiver with all
    /// actual executable edges before this context's preparation suffix retires.
    pub(crate) fn make_interface(
        &mut self,
        request: InterfaceRequest<'_>,
        data: &ValueView<'_>,
    ) -> NativeResult<ValueView<'call>> {
        let sources = self.interface_sources()?;
        self.make_interface_with_sources(request, data, &sources)
    }

    pub(crate) fn make_interface_with_sources(
        &mut self,
        request: InterfaceRequest<'_>,
        data: &ValueView<'_>,
        sources: &[MetadataSource<'_>],
    ) -> NativeResult<ValueView<'call>> {
        self.poll()?;
        self.runtime
            .validate_executable_module(request.implementation)?;
        // Original admission precedes environment/dependency/local growth.
        self.with_values(|slots| data.get(slots).map(|_| ()))?;
        let execution = self.execution_id()?;
        let prepared = self
            .runtime
            .prepare_interface_description(execution, request, sources)?;
        self.publish_interface(prepared, data, sources)
    }

    /// Bytecode has already selected the table and its witnesses. Bind their
    /// executable dependencies in the caller's lexical source before publication.
    pub(crate) fn make_declared_interface(
        &mut self,
        target: &ConstructionTarget,
        environment: Option<TypeEnvironment>,
        data: &ValueView<'_>,
    ) -> NativeResult<ValueView<'call>> {
        self.poll()?;
        self.with_values(|slots| data.get(slots).map(|_| ()))?;
        let ConstructionTarget::Declared {
            module,
            implementation,
            arguments,
            operations,
            constructions,
        } = target
        else {
            return Err(RuntimeError::module_validation(
                "declared interface construction contract",
            ));
        };
        let execution = self.execution_id()?;
        let mut sources = self.interface_sources()?;
        let environment = self.runtime.construction_dependencies(
            execution,
            self.owner,
            environment,
            ConstructionEvidence {
                operations,
                constructions,
            },
            &mut sources,
        )?;
        let arguments = self.runtime.type_arguments(
            self.owner,
            environment.as_ref().map(|env| env.types.clone()),
            arguments,
        )?;
        let owner = self
            .owner
            .member(*module)
            .ok_or_else(|| RuntimeError::module_validation("interface construction module"))?;
        let prepared = self.runtime.prepare_interface_description(
            execution,
            InterfaceRequest {
                implementation: &owner,
                table: implementation.index(),
                arguments: &arguments,
                environment: environment.as_ref(),
                view: false,
            },
            &sources,
        )?;
        self.publish_interface(prepared, data, &sources)
    }

    /// Use the caller's admitted construction capability, not a table search.
    pub(crate) fn make_bound_interface(
        &mut self,
        environment: &TypeEnvironment,
        required: &ConstructionRequirement,
        data: &ValueView<'_>,
    ) -> NativeResult<ValueView<'call>> {
        self.poll()?;
        self.with_values(|slots| data.get(slots).map(|_| ()))?;
        let invalid = || RuntimeError::module_validation("generic construction capability");
        let sources = self.interface_sources()?;
        let types = self.runtime.type_arguments(
            self.owner,
            Some(environment.types.clone()),
            &[
                required.receiver.clone(),
                Ty::Trait(required.interface.clone()),
            ],
        )?;
        let receiver = types[0].identity(self.owner).ok_or_else(invalid)?;
        let target = types[1].identity(self.owner).ok_or_else(invalid)?;
        let snapshot = self
            .runtime
            .environment_snapshot_in(&sources, environment)?;
        let binding = snapshot
            .record
            .constructions
            .selected(&receiver, &target)
            .ok_or_else(invalid)?
            .clone();
        let Ty::Trait(target) = binding.interface.ty() else {
            return Err(invalid());
        };
        if let Some(selection) = &binding.selection {
            let arguments = self.runtime.type_arguments(
                &binding.owner,
                selection.environment.as_ref().map(|env| env.types.clone()),
                &selection.arguments,
            )?;
            let execution = self.execution_id()?;
            let prepared = self.runtime.prepare_interface_description(
                execution,
                InterfaceRequest {
                    implementation: &selection.owner,
                    table: selection.table,
                    arguments: &arguments,
                    environment: selection.environment.as_ref(),
                    view: false,
                },
                &sources,
            )?;
            let source = prepared.description.interface_type().clone();
            let value = self.publish_interface(prepared, data, &sources)?;
            self.upcast_interface(&value, &source, target)
        } else {
            let Ty::Trait(source) = types[0].ty() else {
                return Err(invalid());
            };
            self.upcast_interface(data, source, target)
        }
    }

    /// A parent view uses the original object's fixed metadata graph. It never
    /// searches mutable receiver data or reselects a replacement table/body.
    pub(crate) fn upcast_interface(
        &mut self,
        original: &ValueView<'_>,
        source: &NominalTy,
        target: &NominalTy,
    ) -> NativeResult<ValueView<'call>> {
        self.poll()?;
        if original.kind() != Some(ObjectKind::Interface) {
            return Err(RuntimeError::from(TrapReason::InvalidInterfaceUpcast));
        }
        let description = self.with_values(|slots| {
            let object = interface(slots, original)?;
            Ok(slots
                .scope()
                .read(object, |object| object.description().clone()))
        })?;
        if description.interface_type() != source {
            return Err(RuntimeError::from(TrapReason::InvalidInterfaceUpcast));
        }
        let execution = self.execution_id()?;
        if source == target {
            return self.with_values(|slots| {
                let value = original.get(slots)?;
                let location = slots.push_local(execution, value)?;
                ValueView::protected(value, Some(location))
            });
        }
        let slot = description
            .parents
            .iter()
            .position(|parent| parent.interface == *target)
            .ok_or_else(|| RuntimeError::from(TrapReason::InvalidInterfaceUpcast))?;
        let (cached, value) = self.with_values(|slots| {
            let object = interface(slots, original)?;
            let parent = InterfaceObject::parent(slots.scope(), object, slot)?;
            let value = match parent {
                Some(parent) => ScopedValue::managed(parent),
                None => InterfaceObject::data(slots.scope(), object)?,
            };
            let location = slots.push_local(execution, value)?;
            Ok((
                parent.is_some(),
                ValueView::protected(value, Some(location))?,
            ))
        })?;
        if cached {
            self.validate_interface_receiver(&value)?;
            return Ok(value);
        }
        let parent = &description.parents[slot];
        let binding = &parent.binding;
        let arguments = self.runtime.type_arguments(
            &binding.owner,
            binding.environment.as_ref().map(|env| env.types.clone()),
            &binding.arguments,
        )?;
        let protected =
            [MetadataSource::Interface(original.location().ok_or_else(
                || RuntimeError::from(TrapReason::InvalidInterfaceUpcast),
            )?)];
        let prepared = self.runtime.prepare_interface_description(
            execution,
            InterfaceRequest {
                implementation: &binding.owner,
                table: binding.table,
                arguments: &arguments,
                environment: binding.environment.as_ref(),
                view: parent.view,
            },
            &protected,
        )?;
        let parent = self.publish_interface(prepared, &value, &protected)?;
        self.with_values(|slots| {
            let object = interface(slots, original)?;
            let parent = interface(slots, &parent)?;
            InterfaceObject::publish_parent(slots.scope(), object, slot, parent)
        })?;
        Ok(parent)
    }

    fn validate_interface_receiver(&mut self, value: &ValueView<'_>) -> NativeResult<()> {
        let Runtime {
            memory,
            host,
            host_borrows,
            resources,
            ..
        } = &mut *self.runtime;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                Ok((|| -> Result<(), CheckError> {
                    let object = interface(slots, value)?;
                    let description = slots
                        .scope()
                        .read(object, |object| object.description().clone());
                    let data = InterfaceObject::data(slots.scope(), object)?;
                    let expected = TypeView::prepared(
                        &description.concrete_expression,
                        &description.concrete_type,
                        &description.implementation,
                        description
                            .environment
                            .as_ref()
                            .map(|env| env.types.as_ref()),
                    );
                    if !scoped::matches(
                        slots.scope(),
                        data,
                        expected,
                        host,
                        host_borrows,
                        resources,
                    )? {
                        return Err(RuntimeError::from(TrapReason::InvalidInterfaceReceiver).into());
                    }
                    Ok(())
                })())
            })
        });
        result
            .map_err(|error| self.gc_error(error))?
            .map_err(|error| {
                self.runtime
                    .resources
                    .admit_execution_error(error.into_runtime(self.runtime))
            })
    }

    fn interface_sources(&self) -> NativeResult<Vec<MetadataSource<'call>>> {
        let mut sources = Vec::new();
        sources
            .try_reserve_exact(3)
            .map_err(|_| RuntimeError::resource_limit("interface metadata sources"))?;
        sources.push(MetadataSource::Revision {
            owner: self.owner,
            source: self.origin,
        });
        sources.extend(self.metadata);
        Ok(sources)
    }

    fn publish_interface(
        &mut self,
        prepared: PreparedInterface,
        data: &ValueView<'_>,
        protected: &[MetadataSource<'_>],
    ) -> NativeResult<ValueView<'call>> {
        let execution = self.execution_id()?;
        let description = prepared.description;
        let Runtime {
            memory,
            resources,
            modules,
            host,
            host_borrows,
            ..
        } = &mut *self.runtime;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                Ok((|| -> Result<ValueView<'call>, MetadataError> {
                    let data = data.get(slots)?;
                    let mut targets = Vec::new();
                    targets.try_reserve_exact(protected.len().checked_add(1).ok_or_else(
                        || RuntimeError::resource_limit("interface metadata sources"),
                    )?)?;
                    for (index, source) in protected.iter().enumerate() {
                        if index.is_multiple_of(1024) {
                            resources.poll_closed()?;
                        }
                        targets.push(source.resolve(modules, slots)?);
                    }
                    if let Some(environment) = &prepared.environment {
                        targets.push(
                            MetadataSource::AdmittedEnvironment(environment)
                                .resolve(modules, slots)?,
                        );
                    }
                    if data.kind() != Some(ObjectKind::HostRoot)
                        && !ValueProperties::of(slots.scope(), data)?.storable
                    {
                        return Err(
                            RuntimeError::from(TrapReason::InvalidOrForeignHeapPayload).into()
                        );
                    }
                    let expected = TypeView::prepared(
                        &description.concrete_expression,
                        &description.concrete_type,
                        &description.implementation,
                        description
                            .environment
                            .as_ref()
                            .map(|environment| environment.types.as_ref()),
                    );
                    if !scoped::matches(
                        slots.scope(),
                        data,
                        expected,
                        host,
                        host_borrows,
                        resources,
                    )
                    .map_err(|error| match error {
                        CheckError::Storage(error) => MetadataError::Memory(error),
                        CheckError::Execution(error) => MetadataError::Execution(error),
                    })? {
                        return Err(RuntimeError::from(TrapReason::InvalidInterfaceReceiver).into());
                    }
                    let object = InterfaceObject::alloc(
                        slots.scope(),
                        description,
                        data,
                        &targets,
                        resources,
                    )?;
                    let value = ScopedValue::managed(object);
                    let location = slots.push_local(execution, value)?;
                    Ok(ValueView::protected(value, Some(location))?)
                })())
            })
        });
        result
            .map_err(|error| self.gc_error(error))?
            .map_err(|error| match error {
                MetadataError::Memory(error) => self.gc_error(error),
                MetadataError::Execution(error) => {
                    self.runtime.resources.admit_execution_error(error)
                }
            })
    }
}

pub(super) fn interface<'gc>(
    slots: &mut Slots<'_, 'gc>,
    value: &ValueView<'_>,
) -> Result<Gc<'gc, InterfaceObject>, GcError> {
    let value = value.get(slots)?;
    if value.kind() != Some(ObjectKind::Interface) {
        return Err(GcError::Representation);
    }
    slots
        .scope()
        .downcast(value.object().ok_or(GcError::Representation)?)
}
