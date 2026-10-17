//! Host interface calls enter the same rooted dispatch and result-adapter path.
use crate::{
    RootedInterfaceMethod, Runtime,
    error::RuntimeError,
    frame::ExecutionStack,
    native::{
        binding::NativeResult,
        context::ScriptCall,
        conversion::{
            FromKagari,
            arguments::{IntoKagariArguments, encode_arguments},
            context::ConversionContext,
        },
        interfaces::{Interface, binding::InterfaceMethod},
        typed::NativeContext,
    },
    session::{ExecutionEntry, ExecutionOptions, ExecutionPhase, ExecutionSession},
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
        let runtime = cx.runtime();
        runtime.gc().ensure_no_native_borrow()?;
        runtime.resources().ensure_execution_allowed()?;
        let member = &binding.record.member;
        runtime.validate_loaded_module(&member.owner)?;
        if self.owner.program_root().key() != member.owner.program_root().key()
            || !self
                .argument
                .view(&self.owner)
                .compatible(member.source.view(&member.owner))
        {
            return Err(RuntimeError::module_validation(
                "interface binding belongs to another applied view or generation",
            ));
        }
        let value = self.root.value(runtime.gc()).ok_or_else(|| {
            RuntimeError::module_validation("foreign or expired interface handle")
        })?;
        let Ty::Trait(interface) = member.interface.ty() else {
            unreachable!("checked interface binding")
        };
        let arguments_types = binding
            .record
            .application
            .as_ref()
            .map_or(&[][..], |application| application.arguments.as_slice());
        let method = runtime.prepare_interface_method_slot(
            &value,
            interface,
            member.slot,
            arguments_types,
            binding
                .record
                .application
                .as_ref()
                .map_or_else(Default::default, |application| {
                    application.operations.clone()
                }),
        )?;
        let owner = method.implementation(runtime)?;
        let invoke = cx.invoke_script.ok_or_else(|| {
            RuntimeError::module_validation("call context has no execution backend")
        })?;
        let _session = runtime.begin_interface_execution(&method, options)?;
        let mut conversion = ConversionContext::new(runtime, &member.owner)?;
        let (_roots, mut values) =
            encode_arguments(&mut conversion, &binding.record.signature.params, arguments)?;
        values
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("interface receiver argument"))?;
        values.insert(0, *method.receiver());
        let result = invoke(runtime, &owner, ScriptCall::Interface(&method), &values)?;
        conversion.decode_prepared(&binding.record.signature.result, &result)
    }
}

impl Runtime {
    fn begin_interface_execution(
        &self,
        method: &RootedInterfaceMethod,
        options: ExecutionOptions,
    ) -> NativeResult<ExecutionSession<'_>> {
        let owner = method.implementation(self)?;
        self.validate_loaded_module(&owner)?;
        if !self.gc().validate_candidate_value(method.receiver())
            || self.resources().active_session().is_some_and(|session| {
                session.options.phase == ExecutionPhase::CandidateInitialization
                    && session.root.program_root().key() != owner.program_root().key()
            })
        {
            return Err(RuntimeError::execution_phase_violation(
                "external interface in candidate execution",
            ));
        }
        let root_entry = self.resources().active_session().is_none();
        let session =
            self.begin_execution_inner(&owner, options, ExecutionEntry::RetainedFunction)?;
        if root_entry {
            self.attach_execution_observer()?;
        }
        Ok(session)
    }

    /// Backend entry for an already rooted, generation-checked dispatch selection.
    pub fn enter_interface_execution_stack(
        &self,
        method: &RootedInterfaceMethod,
    ) -> NativeResult<ExecutionStack<'_>> {
        ExecutionStack::new(self.begin_interface_execution(method, self.execution_options())?)
    }
}
