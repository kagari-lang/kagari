//! Host interface calls enter the same rooted dispatch and result-adapter path.
use crate::{
    RootedInterfaceMethod, Runtime,
    error::RuntimeError,
    execution_metadata::managed::sources::MetadataSource,
    frame::ExecutionStack,
    native::{
        binding::NativeResult,
        context::ScriptCall,
        conversion::{
            FromKagari,
            arguments::{IntoKagariArguments, PreparedArguments},
            context::ConversionContext,
        },
        interfaces::{Interface, binding::InterfaceMethod},
        typed::NativeContext,
    },
    objects::application::MethodApplicationInput,
    session::{ExecutionEntry, ExecutionOptions, ExecutionPhase, ExecutionSession},
    value_check,
};
use kagari_types::ty::Ty;

impl<S> Interface<S> {
    pub fn call<A: IntoKagariArguments, R: FromKagari>(
        &self,
        cx: &mut NativeContext<'_>,
        binding: &InterfaceMethod<A, R>,
        arguments: A,
    ) -> NativeResult<R> {
        self.call_with_options(cx, binding, arguments, cx.runtime().execution_options())
    }

    pub fn call_with_options<A: IntoKagariArguments, R: FromKagari>(
        &self,
        cx: &mut NativeContext<'_>,
        binding: &InterfaceMethod<A, R>,
        arguments: A,
        options: ExecutionOptions,
    ) -> NativeResult<R> {
        let runtime = cx.runtime_mut();
        let member = &binding.record.member;
        let method = self.prepare_call(runtime, binding)?.admit(runtime)?;
        let owner = method.implementation(runtime)?;
        let invoke = cx.invoke_script.ok_or_else(|| {
            RuntimeError::module_validation("call context has no execution backend")
        })?;
        let mut session = cx
            .runtime_mut()
            .begin_interface_execution(&method, options)?;
        let mut conversion = ConversionContext::new(session.runtime_mut(), &member.owner)?;
        let mapping =
            PreparedArguments::<A>::from_binding(&conversion, &binding.record.signature.params);
        let values = mapping.encode(&mut conversion, arguments)?;
        let (location, destination) = conversion.result_place()?;
        let published = invoke(
            conversion.runtime_mut(),
            &owner,
            ScriptCall::Interface(&method),
            values,
            destination,
        )?;
        let result = conversion.published_result(published, location)?;
        conversion.decode_prepared(&binding.record.signature.result, &result)
    }

    /// Prepare a live method handle for this receiver and checked relationship.
    /// Inspection and each new call select current code; call admission separately
    /// freezes one computation without changing this handle or its clones.
    pub fn prepare_call<A, R>(
        &self,
        runtime: &mut Runtime,
        binding: &InterfaceMethod<A, R>,
    ) -> NativeResult<RootedInterfaceMethod> {
        runtime.resources().ensure_execution_allowed()?;
        let member = &binding.record.member;
        runtime.validate_executable_module(&member.owner)?;
        if self.owner.installation_id() != member.owner.installation_id()
            || !self
                .argument
                .view(&self.owner)
                .compatible(member.source.view(&member.owner))
        {
            return Err(RuntimeError::module_validation(
                "interface binding belongs to another applied view or installation",
            ));
        }
        let Ty::Trait(interface) = member.interface.ty() else {
            unreachable!("checked interface binding")
        };
        let arguments_types = binding
            .record
            .application
            .as_ref()
            .map_or(&[][..], |application| application.arguments.as_slice());
        let application = binding.record.application.as_ref();
        let witness = application
            .map(|application| MetadataSource::HostEnvironment(&application.environment));
        let sources = witness.as_slice();
        let method = runtime.prepare_interface_method_slot(
            &self.root,
            interface,
            member.slot,
            MethodApplicationInput {
                arguments: arguments_types,
                operations: application.map_or_else(Default::default, |application| {
                    application.operations.clone()
                }),
                constructions: application.map_or_else(Default::default, |application| {
                    application.constructions.clone()
                }),
                sources,
            },
        )?;
        Ok(method)
    }
}

impl Runtime {
    pub(crate) fn begin_interface_execution(
        &mut self,
        method: &RootedInterfaceMethod,
        options: ExecutionOptions,
    ) -> NativeResult<ExecutionSession<'_>> {
        let owner = method.implementation(self)?;
        self.validate_executable_module(&owner)?;
        {
            let mut conversion = ConversionContext::new(self, &owner)?;
            let value = conversion.import(&method._root)?;
            value_check::candidate_protected(conversion.runtime_mut(), &[value])?;
        }
        if self.resources().active_session().is_some_and(|session| {
            session.options.phase == ExecutionPhase::CandidateInitialization
                && session.root.program_root().key() != owner.program_root().key()
        }) {
            return Err(RuntimeError::execution_phase_violation(
                "external interface in candidate execution",
            ));
        }
        let origin = method.body_origin()?;
        let root_entry = self.resources().active_session().is_none();
        let mut session =
            self.begin_execution_inner(&owner, options, ExecutionEntry::RetainedFunction, origin)?;
        if root_entry {
            session.runtime_mut().attach_execution_observer()?;
        }
        Ok(session)
    }

    /// Backend entry for an already rooted, generation-checked dispatch selection.
    pub fn enter_interface_execution_stack(
        &mut self,
        method: &RootedInterfaceMethod,
    ) -> NativeResult<ExecutionStack<'_>> {
        ExecutionStack::new(self.begin_interface_execution(method, self.execution_options())?)
    }
}
