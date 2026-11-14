//! Host method preparation publishes genuine leases before scratch retirement.
use super::interfaces::interface;
use crate::{
    error::{RuntimeError, trap::TrapReason},
    execution::roots::ExecutionRoots,
    execution_metadata::{
        application_key::ApplicationArguments,
        applications::AdmittedApplication,
        call_contracts::ScopedInterfaceCall,
        managed::{applications::ApplicationObject, sources::MetadataSource},
    },
    frame::arguments::FrameArguments,
    frame::types::{
        arguments::TypeArgument, construction::ConstructionScope, operations::OperationBindings,
    },
    native::{binding::NativeResult, conversion::context::ConversionContext},
    objects::{
        interfaces::{InterfaceRequest, description::InterfaceResultBinding},
        invocation::MethodInvocation,
        method_view::MethodView,
    },
    value::{carriers::RootedValue, view::ValueView},
};
use kagari_gc::root::Root;

type PreparedHostMethod = (MethodInvocation, Option<Root<ApplicationObject>>);

impl<'call> ConversionContext<'call> {
    pub(crate) fn prepare_interface_invocation(
        &mut self,
        call: &ScopedInterfaceCall,
        receiver: Option<&ValueView<'_>>,
        mut protected: Vec<MetadataSource<'_>>,
    ) -> NativeResult<MethodInvocation> {
        let execution = self.execution_id()?;
        let invocation = if let Some(operation) = call.operation {
            MethodInvocation::from_operation(self.runtime, execution, operation, &protected)?
        } else {
            let receiver = receiver
                .ok_or_else(|| RuntimeError::module_validation("missing interface receiver"))?;
            let description = self.with_values(|slots| {
                let object = interface(slots, receiver)?;
                Ok(slots
                    .scope()
                    .read(object, |object| object.description().clone()))
            })?;
            let parent = self.upcast_interface(
                receiver,
                description.interface_type(),
                call.interface_type(),
            )?;
            let description = self.with_values(|slots| {
                let object = interface(slots, &parent)?;
                Ok(slots
                    .scope()
                    .read(object, |object| object.description().clone()))
            })?;
            if !description
                .interface_view()
                .compatible(call.interface.view(self.owner))
            {
                return Err(RuntimeError::module_validation(
                    "generic call interface view",
                ));
            }
            protected
                .try_reserve(1)
                .map_err(|_| RuntimeError::resource_limit("interface invocation sources"))?;
            protected.push(MetadataSource::Interface(parent.location().ok_or_else(
                || RuntimeError::from(TrapReason::InvalidInterfaceHandle),
            )?));
            MethodInvocation::from_interface(self.runtime, description, call.slot, &protected)?
        };
        let invocation = self.runtime.apply_method_invocation(
            execution,
            invocation,
            &call.arguments,
            call.operations.clone(),
            &mut protected,
        )?;
        if let Some(application) = &invocation.application {
            protected
                .try_reserve(1)
                .map_err(|_| RuntimeError::resource_limit("interface application sources"))?;
            protected.push(MetadataSource::Application(application.source()));
        }
        invocation.admit(self.runtime, execution, &mut protected)
    }

    /// Transfer just the admitted graph to the enclosing call preparation. Its
    /// provider owns receiver/application/body; all other child scratch retires.
    pub(crate) fn into_method_invocation(
        mut self,
        mut invocation: MethodInvocation,
    ) -> NativeResult<MethodInvocation> {
        let expected = invocation
            .computation()
            .ok_or_else(|| RuntimeError::module_validation("method computation is not admitted"))?;
        let mark = self.locals.as_mut().expect("method conversion suffix");
        let result = self
            .runtime
            .memory
            .scope(&self.runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    AdmittedApplication::preserve(slots, mark, expected)
                })
            });
        let computation = result.map_err(|error| self.gc_error(error))?;
        invocation.rebind_computation(computation)?;
        Ok(invocation)
    }

    pub(crate) fn validate_interface_arguments(
        &mut self,
        view: &MethodView,
        arguments: &[RootedValue],
    ) -> NativeResult<()> {
        let mut values = Vec::new();
        values
            .try_reserve_exact(arguments.len())
            .map_err(|_| RuntimeError::resource_limit("Host method argument validation"))?;
        for (index, argument) in arguments.iter().enumerate() {
            if index.is_multiple_of(1024) {
                self.poll()?;
            }
            values.push(self.import(argument)?.reborrow());
        }
        view.validate_arguments(self.runtime, FrameArguments::plain(&values))
    }

    /// Adapt a validated return while its original source remains protected.
    /// The caller publishes this local result before conversion cleanup.
    pub(crate) fn adapt_method_result(
        &mut self,
        adapter: &InterfaceResultBinding,
        result: &ValueView<'_>,
        protected: &[MetadataSource<'_>],
    ) -> NativeResult<ValueView<'call>> {
        let arguments = self.runtime.type_arguments(
            &adapter.owner,
            adapter
                .environment
                .as_ref()
                .map(|environment| environment.types.clone()),
            &adapter.arguments,
        )?;
        self.make_interface_with_sources(
            InterfaceRequest {
                implementation: &adapter.owner,
                table: adapter.table,
                arguments: &arguments,
                environment: adapter.environment.as_ref(),
                view: false,
            },
            result,
            protected,
        )
    }

    pub(crate) fn apply_interface_method(
        &mut self,
        invocation: MethodInvocation,
        arguments: &[TypeArgument],
        operations: OperationBindings,
        constructions: ConstructionScope,
        mut protected: Vec<MetadataSource<'_>>,
    ) -> NativeResult<PreparedHostMethod> {
        let execution = self.execution_id()?;
        let arguments = ApplicationArguments::new(self.owner, arguments.to_vec(), constructions)?;
        let invocation = self.runtime.apply_method_invocation(
            execution,
            invocation,
            &arguments,
            operations,
            &mut protected,
        )?;
        let root = invocation
            .application
            .as_ref()
            .map(|application| self.runtime.retain_application(application))
            .transpose()?;
        Ok((invocation, root))
    }

    pub(crate) fn admit_interface_method(
        &mut self,
        invocation: MethodInvocation,
        mut protected: Vec<MetadataSource<'_>>,
    ) -> NativeResult<(MethodInvocation, Root<ApplicationObject>)> {
        let execution = self.execution_id()?;
        let invocation = invocation.admit(self.runtime, execution, &mut protected)?;
        let computation = invocation.computation().ok_or_else(|| {
            RuntimeError::module_validation("method computation was not admitted")
        })?;
        let root = self.runtime.retain_application(computation)?;
        Ok((invocation, root))
    }
}
