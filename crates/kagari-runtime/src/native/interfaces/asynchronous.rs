//! Queued interface calls retain the checked dispatch selection and result scope.
use crate::{
    Runtime,
    error::RuntimeError,
    execution::roots::ExecutionRoots,
    execution_metadata::{environments::AdmittedEnvironment, managed::sources::MetadataSource},
    frame::{
        factory::{FactoryEntry, QueuedFactory},
        types::arguments::TypeArgument,
    },
    module::managed::source::RevisionSource,
    native::{
        binding::NativeResult,
        composition::future_output,
        conversion::{
            FromKagari, KagariType,
            arguments::{IntoKagariArguments, KagariArguments, PreparedArguments},
            context::ConversionContext,
        },
        function_handle::asynchronous::FutureOutput,
        interfaces::{
            Interface,
            binding::{InterfaceMember, InterfaceMethod},
        },
    },
    scheduler::result::ScheduledResult,
    session::{ExecutionOptions, ExecutionPhase},
    source::FunctionSourceTarget,
    value_check::data::async_protected,
};

use std::slice;

/// A checked interface member returning one Future layer. The descriptor retains
/// dispatch type evidence and the lexical output scope, not a particular receiver.
#[derive(Debug)]
pub struct AsyncInterfaceMethod<A, R> {
    binding: InterfaceMethod<A, FutureOutput<R>>,
    output: TypeArgument,
}

impl<A, R> AsyncInterfaceMethod<A, R> {
    /// Preserve the declared member identity without claiming a receiver's body.
    pub fn source_target(&self) -> FunctionSourceTarget {
        self.binding.source_target()
    }

    pub fn member(&self) -> &InterfaceMember {
        self.binding.member()
    }
}

impl<A, R> Clone for AsyncInterfaceMethod<A, R> {
    fn clone(&self) -> Self {
        Self {
            binding: self.binding.clone(),
            output: self.output.clone(),
        }
    }
}

impl Runtime {
    pub fn bind_async_interface_method<A: KagariArguments + 'static, R: KagariType + 'static>(
        &mut self,
        member: &InterfaceMember,
    ) -> NativeResult<AsyncInterfaceMethod<A, R>> {
        let binding = self.bind_interface_method(member)?;
        let output = future_output(
            self,
            &binding.record.member.owner,
            &binding.record.signature.result,
        )?;
        Ok(AsyncInterfaceMethod { binding, output })
    }
}

impl<S> Interface<S> {
    pub fn schedule_async<A: IntoKagariArguments, R: FromKagari + Send + 'static>(
        &self,
        runtime: &mut Runtime,
        binding: &AsyncInterfaceMethod<A, R>,
        arguments: A,
        mut options: ExecutionOptions,
    ) -> NativeResult<ScheduledResult<R>> {
        runtime.require_idle_driver()?;
        runtime.drain_retired_executions()?;
        if options.phase != ExecutionPhase::Ordinary {
            return Err(RuntimeError::execution_phase_violation(
                "owned candidate factory",
            ));
        }
        let method = self
            .prepare_call(runtime, &binding.binding)?
            .admit(runtime)?;
        let member = &binding.binding.record.member;
        let external = options.cancellation.clone();
        options.cancellation = Default::default();
        let session = runtime.begin_interface_execution(&method, options)?;
        let mut completion = None;
        let execution = Runtime::initialize_owned_factory(session, external, |runtime| {
            let invocation = runtime
                .resources()
                .active_session()
                .ok_or_else(|| RuntimeError::module_validation("interface factory execution"))?
                .id;
            let body =
                runtime.with_revision_origin(&member.owner, member.origin(), |slots, body| {
                    slots.push_dependency(invocation, body)
                })?;
            let environment = binding
                .binding
                .record
                .application
                .as_ref()
                .map(|application| {
                    runtime
                        .memory
                        .scope(&runtime.resources, |cx, roots| {
                            ExecutionRoots::with(roots, cx, |slots| {
                                let value = slots.scope().get(&application.environment)?;
                                AdmittedEnvironment::publish(slots, invocation, value)
                            })
                        })
                        .map_err(|error| runtime.managed_memory_error(error))
                })
                .transpose()?;
            let mut cx = ConversionContext::for_selected(
                runtime,
                &member.owner,
                invocation,
                RevisionSource::Execution(body),
                environment
                    .as_ref()
                    .map(MetadataSource::AdmittedEnvironment),
            )?;
            let receiver = cx.import(&method._root)?;
            async_protected(cx.runtime_mut(), slice::from_ref(&receiver))?;
            let mapping =
                PreparedArguments::<A>::from_binding(&cx, &binding.binding.record.signature.params);
            let values = mapping.encode(&mut cx, arguments)?;
            let values = cx.queue_arguments(values)?;
            cx.poll()?;
            drop(cx);
            completion = Some((invocation, body, environment));
            Ok(QueuedFactory {
                owner: member.owner.clone(),
                future: binding.binding.record.signature.result.clone(),
                entry: FactoryEntry::Interface {
                    method: Box::new(method),
                    arguments: values,
                },
            })
        })?;
        let owner = member.owner.clone();
        let output = binding.output.clone();
        let (invocation, body, environment) = completion
            .ok_or_else(|| RuntimeError::module_validation("interface completion sources"))?;
        runtime.schedule_execution(execution, move |runtime, root| {
            if runtime
                .resources()
                .active_session()
                .map(|session| session.id)
                != Some(invocation)
            {
                return Err(RuntimeError::module_validation(
                    "interface completion execution",
                ));
            }
            let mut conversion = ConversionContext::for_selected(
                runtime,
                &owner,
                invocation,
                RevisionSource::Execution(body),
                environment
                    .as_ref()
                    .map(MetadataSource::AdmittedEnvironment),
            )?;
            let value = conversion.import(root)?;
            conversion.decode_prepared(&output, &value)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        execution_metadata::applications::ApplicationRecord,
        frame::ExecutionStack,
        module::{ExecutableModule, VerifiedProgram, dispatch::PublicationToken},
        native::{
            binding::{Codec, NativeBinding},
            builder::ModuleBuilder,
            declarations::FunctionDecl,
            function_handle::asynchronous::FutureOutput,
            module::NativeModule,
            storage::NativeStorage,
            types::Type,
        },
        scheduler::ScheduledWork,
        value::carriers::RootedValue,
    };
    use kagari_compiler::{
        bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir,
    };
    use kagari_hir::{analysis::AnalysisDatabase, compile_input::program::CheckedCompilation};
    use kagari_source::source_database::{SourceDatabase, SourceLayer};
    use std::{
        slice,
        sync::{Arc, Weak},
        task::{Context, Poll, Waker},
    };

    fn provider() -> NativeModule {
        // Only the Future storage role is needed for queue admission. Do not pull
        // unrelated protected foundation dependency graphs into this contract test.
        let mut builder = ModuleBuilder::new("fixture::queue", &Default::default()).unwrap();
        let mut future = builder.define_type("Future").unwrap();
        future.type_parameter("T").unwrap();
        future.native_storage(NativeStorage::future()).unwrap();
        let future = future.finish().unwrap();
        let ready = builder
            .define_function(
                FunctionDecl::new("ready")
                    .parameter("value", Type::i32())
                    .returns(future.apply([Type::i32()]).unwrap()),
            )
            .unwrap();
        builder
            .bind(
                ready,
                NativeBinding::new(
                    vec![Codec::Exact(Type::i32())],
                    Codec::Exact(future.apply([Type::i32()]).unwrap()),
                    |_| panic!("this test stops at factory admission, before executing its body"),
                ),
            )
            .unwrap();
        builder.finish().unwrap()
    }

    fn compile(source: &str, provider: &NativeModule) -> VerifiedProgram {
        let mut sources = SourceDatabase::default();
        let root = sources
            .set("queued-interface.kgr", source.into(), SourceLayer::Base)
            .unwrap();
        let mut analysis = AnalysisDatabase::default();
        analysis.set_native_modules(vec![Arc::new(provider.to_declaration())]);
        let snapshot = analysis
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let checked = snapshot.check_program(root, &Default::default()).unwrap();
        let mir = lower_program_to_mir(
            &CheckedCompilation::from_program(&checked, &Default::default()).unwrap(),
            &Default::default(),
        )
        .unwrap();
        VerifiedProgram::from_bytecode(
            lower_program_to_bytecode(&mir, &Default::default()).unwrap(),
        )
    }

    fn interface(runtime: &mut Runtime, owner: &ExecutableModule) -> Interface {
        let value = runtime
            .make_interface(owner, 0, &RootedValue::from(7_i32))
            .unwrap();
        let description = runtime.interface_description(&value).unwrap();
        let argument = runtime
            .type_arguments(
                owner,
                description
                    .environment
                    .as_ref()
                    .map(|environment| environment.types.clone()),
                slice::from_ref(&description.interface_expression),
            )
            .unwrap()
            .remove(0);
        let mut conversion = ConversionContext::new(runtime, owner).unwrap();
        let value = conversion.import(&value).unwrap();
        conversion.decode_value(&argument, &value).unwrap()
    }

    fn computation(runtime: &Runtime, ticket: &ScheduledResult<i32>) -> Weak<ApplicationRecord> {
        let state = runtime
            .resources()
            .sessions
            .get(ticket.execution_id().0)
            .unwrap();
        let factory = state.queued_factory.borrow();
        let FactoryEntry::Interface { method, .. } = &factory.as_ref().unwrap().entry else {
            panic!("interface factory")
        };
        Arc::downgrade(method.invocation.computation().unwrap().record())
    }

    fn dequeue(runtime: &mut Runtime, ticket: &ScheduledResult<i32>, expected: PublicationToken) {
        let context = Context::from_waker(Waker::noop());
        let mut turn = runtime.begin_scheduler_turn(&context, None).unwrap();
        let Some(ScheduledWork::Invocation(mut work)) = turn.next_work().unwrap() else {
            panic!("queued interface factory must be ready")
        };
        assert_eq!(work.execution().execution_id(), ticket.execution_id());
        work.with_execution(|runtime, owner| {
            let mut activation = Some(runtime.resume_owned_execution(owner)?);
            activation.as_mut().unwrap().with(|runtime, stack| {
                assert!(matches!(
                    stack.poll_await(runtime).unwrap(),
                    Poll::Ready(None)
                ));
                assert_eq!(
                    stack.current(runtime).unwrap().loaded().publication_token(),
                    expected,
                    "dequeue must enter the publication selected at admission"
                );
                runtime.collect_garbage().unwrap();
                assert_eq!(
                    stack.current(runtime).unwrap().loaded().publication_token(),
                    expected
                );
                Ok(())
            })?;
            assert!(ExecutionStack::try_park_ready(&mut activation)?);
            Ok(())
        })
        .unwrap();
        work.park();
        drop(turn);
        runtime.cancel_scheduled_execution(ticket).unwrap();
    }

    #[test]
    fn queued_interface_admission_is_exact_while_retained_method_handles_stay_live() {
        let source = include_str!("../../../tests/fixtures/metadata/interface-queue.kgr");
        let provider = provider();
        let prepared = compile(source, &provider);
        let mut runtime = Runtime::default();
        provider.install(&mut runtime).unwrap();
        let program = runtime
            .load_verified_program("queue", prepared.clone())
            .unwrap();
        let unrelated_program = runtime
            .load_verified_program("other-installation", prepared)
            .unwrap();
        let old = runtime.select_program(&program).unwrap();
        let unrelated = runtime.select_program(&unrelated_program).unwrap();
        let receiver = interface(&mut runtime, &old);
        let other_receiver = interface(&mut runtime, &unrelated);
        let member = receiver.member(&mut runtime, "run").unwrap();
        let binding = runtime
            .bind_async_interface_method::<(), i32>(&member)
            .unwrap();
        let live_binding = runtime
            .bind_interface_method::<(), FutureOutput<i32>>(&member)
            .unwrap();
        let live = receiver.prepare_call(&mut runtime, &live_binding).unwrap();
        let live_clone = live.clone();
        let queued = receiver
            .schedule_async(&mut runtime, &binding, (), Default::default())
            .unwrap();
        let cancelled = receiver
            .schedule_async(&mut runtime, &binding, (), Default::default())
            .unwrap();
        let queued_computation = computation(&runtime, &queued);
        let cancelled_computation = computation(&runtime, &cancelled);
        let roots_before = runtime.gc_stats().root_registrations;
        assert!(
            other_receiver
                .schedule_async(&mut runtime, &binding, (), Default::default())
                .is_err()
        );
        assert_eq!(runtime.gc_stats().root_registrations, roots_before);
        let candidate = runtime
            .stage_reload_verified_program(
                &program,
                None,
                compile(&source.replace("ready(1)", "ready(2)"), &provider),
            )
            .unwrap();
        runtime.publish_staged_reload(candidate).unwrap();
        let current = runtime.select_program(&program).unwrap();
        assert_ne!(old.publication_token(), current.publication_token());
        // The ordinary handle and a pre-admission clone must both stay live.
        assert_eq!(
            live.implementation(&mut runtime)
                .unwrap()
                .publication_token(),
            current.publication_token()
        );
        assert_eq!(
            live_clone
                .implementation(&mut runtime)
                .unwrap()
                .publication_token(),
            current.publication_token()
        );
        runtime.cancel_scheduled_execution(&cancelled).unwrap();
        runtime.collect_garbage().unwrap();
        assert!(cancelled_computation.upgrade().is_none());
        assert!(queued_computation.upgrade().is_some());
        let fresh = receiver
            .schedule_async(&mut runtime, &binding, (), Default::default())
            .unwrap();
        let fresh_computation = computation(&runtime, &fresh);
        runtime.collect_garbage().unwrap();
        dequeue(&mut runtime, &queued, old.publication_token());
        dequeue(&mut runtime, &fresh, current.publication_token());
        runtime.collect_garbage().unwrap();
        assert!(queued_computation.upgrade().is_none());
        assert!(fresh_computation.upgrade().is_none());
        assert_eq!(
            live.implementation(&mut runtime)
                .unwrap()
                .publication_token(),
            current.publication_token()
        );
        assert!(!runtime.is_quarantined());
    }
}
