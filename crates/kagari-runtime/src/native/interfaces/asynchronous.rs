//! Queued interface calls retain the checked dispatch selection and result scope.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{
        factory::{FactoryEntry, QueuedFactory},
        types::arguments::TypeArgument,
    },
    native::{
        binding::NativeResult,
        composition::future_output,
        conversion::{
            FromKagari, KagariType,
            arguments::{IntoKagariArguments, KagariArguments, encode_arguments},
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
};

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
        &self,
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
        runtime: &Runtime,
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
        let execution = runtime.initialize_owned_factory(session, external, || {
            let mut cx = ConversionContext::new(runtime, &member.owner)?;
            let (_arguments, mut values) =
                encode_arguments(&mut cx, &binding.binding.record.signature.params, arguments)?;
            values
                .try_reserve(1)
                .map_err(|_| RuntimeError::resource_limit("interface receiver argument"))?;
            values.insert(0, *method.receiver());
            runtime.gc().validate_async_values(&values)?;
            let roots = runtime
                .gc()
                .root_execution_values(values.clone())
                .ok_or_else(|| RuntimeError::resource_limit("async interface arguments"))?;
            cx.poll()?;
            Ok(QueuedFactory {
                owner: member.owner.clone(),
                future: binding.binding.record.signature.result.clone(),
                entry: FactoryEntry::Interface {
                    method: Box::new(method),
                    arguments: values,
                    roots,
                },
            })
        })?;
        let owner = member.owner.clone();
        let output = binding.output.clone();
        runtime.schedule_execution(execution, move |runtime, root| {
            let value = root
                .value(runtime.gc())
                .ok_or_else(|| RuntimeError::module_validation("async interface output root"))?;
            ConversionContext::new(runtime, &owner)?.decode_prepared(&output, &value)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
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
        value::Value,
    };
    use kagari_compiler::{
        bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir,
    };
    use kagari_hir::analysis::AnalysisDatabase;
    use kagari_source::source_database::{SourceDatabase, SourceLayer};
    use kagari_types::ty::Ty;
    use std::{
        slice,
        sync::Arc,
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
        let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
        VerifiedProgram::from_bytecode(
            lower_program_to_bytecode(&mir, &Default::default()).unwrap(),
        )
    }

    fn interface(runtime: &Runtime, owner: &ExecutableModule) -> Interface {
        let value = runtime.make_interface(owner, 0, Value::I32(7)).unwrap();
        let Value::Interface(id) = value else {
            unreachable!()
        };
        let ty = Ty::Trait(
            runtime
                .gc
                .interface_snapshot(id)
                .unwrap()
                .description
                .interface_type()
                .clone(),
        );
        let argument = runtime
            .resolve_type_arguments(owner, slice::from_ref(&ty))
            .unwrap()
            .remove(0);
        /* SAFETY: The freshly allocated interface has no intervening collection or user mapping callback before admission. */
        unsafe {
            ConversionContext::new(runtime, owner)
                .unwrap()
                .decode_transport(&argument, &value)
        }
        .unwrap()
    }

    fn dequeue(runtime: &Runtime, ticket: &ScheduledResult<i32>, expected: PublicationToken) {
        let context = Context::from_waker(Waker::noop());
        let turn = runtime.begin_scheduler_turn(&context, None).unwrap();
        let Some(ScheduledWork::Invocation(work)) = turn.next_work().unwrap() else {
            panic!("queued interface factory must be ready")
        };
        assert_eq!(work.execution().execution_id(), ticket.execution_id());
        let mut activation = Some(runtime.resume_owned_execution(work.execution()).unwrap());
        let stack = activation.as_ref().unwrap();
        assert_eq!(stack.poll_await(runtime).unwrap(), Poll::Ready(None));
        assert_eq!(
            stack.current().unwrap().loaded().publication_token(),
            expected,
            "dequeue must enter the publication selected at admission"
        );
        runtime.collect_garbage().unwrap();
        assert_eq!(
            stack.current().unwrap().loaded().publication_token(),
            expected
        );
        assert!(ExecutionStack::try_park_ready(&mut activation, runtime).unwrap());
        work.park();
        drop(turn);
        runtime.cancel_scheduled_execution(ticket).unwrap();
    }

    #[test]
    fn queued_interface_admission_is_exact_while_retained_method_handles_stay_live() {
        let source = r#"
            use fixture::queue::{Future, ready};
            pub trait Queue { fn run(self) -> Future<i32>; }
            impl Queue for i32 { fn run(self) -> Future<i32> { ready(1) } }
            fn main() -> Queue { 7 }
        "#;
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
        let receiver = interface(&runtime, &old);
        let other_receiver = interface(&runtime, &unrelated);
        let member = receiver.member(&runtime, "run").unwrap();
        let binding = runtime
            .bind_async_interface_method::<(), i32>(&member)
            .unwrap();
        let live_binding = runtime
            .bind_interface_method::<(), FutureOutput<i32>>(&member)
            .unwrap();
        let live = receiver.prepare_call(&runtime, &live_binding).unwrap();
        let live_clone = live.clone();
        let queued = receiver
            .schedule_async(&runtime, &binding, (), Default::default())
            .unwrap();
        let cancelled = receiver
            .schedule_async(&runtime, &binding, (), Default::default())
            .unwrap();
        let roots_before = runtime.gc.stats().method_applications;
        assert!(
            other_receiver
                .schedule_async(&runtime, &binding, (), Default::default())
                .is_err()
        );
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
            live.implementation(&runtime).unwrap().publication_token(),
            current.publication_token()
        );
        assert_eq!(
            live_clone
                .implementation(&runtime)
                .unwrap()
                .publication_token(),
            current.publication_token()
        );
        runtime.cancel_scheduled_execution(&cancelled).unwrap();
        runtime.collect_garbage().unwrap();
        assert_eq!(runtime.gc.stats().method_applications, roots_before - 1);
        let fresh = receiver
            .schedule_async(&runtime, &binding, (), Default::default())
            .unwrap();
        runtime.collect_garbage().unwrap();
        dequeue(&runtime, &queued, old.publication_token());
        dequeue(&runtime, &fresh, current.publication_token());
        runtime.collect_garbage().unwrap();
        assert_eq!(
            runtime.gc.stats().method_applications,
            0,
            "live handles must not retain completed/cancelled admitted computations"
        );
        assert_eq!(
            live.implementation(&runtime).unwrap().publication_token(),
            current.publication_token()
        );
        assert!(!runtime.is_quarantined());
    }
}
