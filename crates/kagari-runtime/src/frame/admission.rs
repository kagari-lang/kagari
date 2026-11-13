//! One transaction admits arguments and publishes exact frame-owned dependencies.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};
use crate::{
    Runtime,
    error::{RuntimeError, fault::EngineFault},
    execution::{roots::ExecutionRoots, space},
    execution_metadata::{
        applications::AdmittedApplication,
        environments::AdmittedEnvironment,
        managed::{ProtectedGraph, ProtectedMetadata, sources::MetadataError},
    },
    frame::{
        ExecutionFrame, FrameDispatch, FrameEntry, FrameStack, ReturnDestination,
        arguments::FrameArguments,
        driver::ExecutionAction,
        uncommon::{FrameUncommon, UncommonFrames},
        values::{FrameAllocationError, FrameMetadata, FrameSlots},
    },
    module::{ExecutableModule, managed::source::RevisionSource},
    value_check,
};
use kagari_bytecode::{instruction::Slot, module::CallableTarget};
use kagari_gc::error::GcError;
use std::slice;

impl FrameStack {
    /// Reuse a checked stack only across internal preparation without callbacks.
    pub(super) fn push_admitted_arguments(
        &self,
        runtime: &mut Runtime,
        loaded: ExecutableModule,
        target: CallableTarget,
        args: FrameArguments<'_, '_>,
        return_dst: Option<Slot>,
        dispatch: FrameDispatch<'_>,
    ) -> Result<ExecutionAction, RuntimeError> {
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::CallAdmission);

        if let CallableTarget::Script(function) = target
            && loaded.is_static_initializer(function)
            && !matches!(&dispatch.entry, FrameEntry::StaticInitializer(attempt)
                if attempt.recipe().module.key() == loaded.key() && attempt.recipe().function == function)
        {
            return Err(RuntimeError::module_validation(
                "static initializer requires runtime-owned transition authority",
            ));
        }
        if loaded
            .execution()
            .callee(target)
            .is_some_and(|callee| callee.may_suspend)
        {
            runtime.reject_static_initialization("suspending frame entry")?;
        }
        value_check::candidate_arguments(runtime, args)?;
        if loaded
            .execution()
            .callee(target)
            .is_some_and(|callee| callee.may_suspend)
            && (self.session_state(runtime).owner.borrow().is_none()
                || (!self.frames(runtime)?.is_empty()
                    && !matches!(dispatch.entry, FrameEntry::Await)))
        {
            return Err(RuntimeError::module_validation(
                "resume body requires an owned driver entry",
            ));
        }
        {
            let resources = &runtime.resources;
            let mut frames = resources.sessions.frames_mut(self.session).ok_or_else(|| {
                resources.quarantine(EngineFault::FrameStackIsBorrowedAcrossExecution)
            })?;
            let reserve = space::reserve(&mut frames, 1, &self.session_state(runtime).space);
            drop(frames);
            reserve.map_err(|error| runtime.managed_memory_error(error))?;
        }
        // Depth polling/diagnostics run with no source or frame-control loan.
        let uncommon_space = if dispatch.invocation.is_some()
            || matches!(&dispatch.entry, FrameEntry::StaticInitializer(_))
        {
            runtime
                .resources
                .uncommon_frames
                .reserve()
                .map_err(|error| runtime.managed_memory_error(error))?;
            Some(
                self.session_state(runtime)
                    .space
                    .lease(UncommonFrames::entry_bytes())
                    .map_err(|error| runtime.managed_memory_error(error))?,
            )
        } else {
            None
        };
        runtime.resources.enter_call()?;
        let prepare = || -> Result<_, RuntimeError> {
            let FrameDispatch {
                origin,
                metadata,
                prepared,
                entry,
                mut invocation,
                environment,
            } = dispatch;
            let callee = loaded.execution().callee(target).ok_or_else(|| {
                runtime
                    .resources
                    .quarantine(EngineFault::InvalidPreparedCalleeLayout)
            })?;
            if args.len() != callee.parameter_count {
                return Err(RuntimeError::module_validation(
                    "frame argument count does not match the linked function",
                ));
            }
            let (generic, action) = match target {
                CallableTarget::Script(function) => (
                    loaded.bytecode.functions[function.index()]
                        .metadata
                        .semantic
                        .generic
                        .as_ref(),
                    ExecutionAction::Script,
                ),
                CallableTarget::Native(import) => (
                    loaded.bytecode.native_imports[import.index()]
                        .generic
                        .as_ref(),
                    ExecutionAction::NativeEntry,
                ),
            };
            if callee.generic
                && generic.is_some_and(|body| {
                    environment
                        .as_ref()
                        .is_none_or(|environment| !environment.facts.types.matches(body))
                })
            {
                return Err(RuntimeError::module_validation(
                    "shared entry requires a call environment",
                ));
            }
            runtime.resources.ensure_execution_allowed()?;
            runtime.validate_executable_module(&loaded)?;
            if let Some(environment) = &environment {
                runtime.validate_environment_in(&[environment.source], &environment.facts)?;
            }
            if invocation.is_some() && metadata.is_none() {
                return Err(RuntimeError::module_validation(
                    "method invocation has no actual metadata source",
                ));
            }
            if let Some(source) = &metadata {
                if let Some(method) = &invocation {
                    let view = method.view(runtime, slice::from_ref(source))?;
                    if !method.is_admitted()
                        || view.implementation().program_identity() != loaded.program_identity()
                        || view.target() != target
                    {
                        return Err(RuntimeError::module_validation(
                            "frame method selection mismatch",
                        ));
                    }
                } else {
                    runtime.with_metadata_graph(slice::from_ref(source), |_, _, _| Ok(()))?;
                }
            }
            let links = match target {
                CallableTarget::Script(function)
                    if loaded.execution().functions[function.index()].needs_runtime_links() =>
                {
                    Some(
                        runtime
                            .modules
                            .linked_function(
                                &loaded,
                                function,
                                environment.as_ref().map(|scope| scope.facts.id),
                            )
                            .ok_or_else(|| {
                                RuntimeError::module_validation("missing linked function execution")
                            })?,
                    )
                }
                _ => None,
            };
            let Runtime {
                memory,
                modules,
                resources,
                ..
            } = &mut *runtime;
            let allocation = memory.scope(resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    Ok((|| -> Result<_, FrameAllocationError> {
                        // Admit every original source before frame growth can
                        // reuse a stale coordinate. No code-ID recovery/fallback.
                        let body = origin.resolve(modules, slots, &loaded)?;
                        let environment = environment
                            .as_ref()
                            .map(|environment| {
                                let mut resolve = || -> Result<_, MetadataError> {
                                    let source = environment.source.resolve(modules, slots)?;
                                    let graph = ProtectedGraph::new(
                                        slots.scope(),
                                        slice::from_ref(&source),
                                        resources,
                                    )?;
                                    graph
                                        .environment(slots.scope(), &environment.facts, resources)?
                                        .ok_or_else(|| GcError::Representation.into())
                                };
                                resolve().map_err(|error| match error {
                                    MetadataError::Memory(error) => {
                                        FrameAllocationError::Storage(error)
                                    }
                                    MetadataError::Execution(error) => {
                                        FrameAllocationError::Execution(error)
                                    }
                                })
                            })
                            .transpose()?;
                        let dependency = metadata
                            .map(|source| source.resolve(modules, slots))
                            .transpose()?;
                        let computation = if invocation.is_some() {
                            match dependency {
                                Some(ProtectedMetadata::Application(value)) => Some(value),
                                _ => return Err(GcError::Representation.into()),
                            }
                        } else {
                            None
                        };
                        let dependency = dependency
                            .filter(|_| computation.is_none())
                            .map(ProtectedMetadata::erase)
                            .filter(|dependency| {
                                !environment
                                    .is_some_and(|environment| dependency.ptr_eq(environment))
                            });
                        let dependencies = dependency.as_slice();
                        let storage = FrameSlots::allocate(
                            slots,
                            self.session,
                            &callee.slot_layout,
                            &callee.parameter_slots,
                            args,
                            FrameMetadata {
                                body,
                                inherited_body: match origin {
                                    RevisionSource::Execution(body) => Some(body),
                                    RevisionSource::Current
                                    | RevisionSource::Host(_)
                                    | RevisionSource::Application(_)
                                    | RevisionSource::HostApplication(_)
                                    | RevisionSource::RootedClosure(_)
                                    | RevisionSource::Cold(_)
                                    | RevisionSource::Closure(_)
                                    | RevisionSource::StaticRecipe { .. } => None,
                                },
                                dependencies,
                            },
                            resources,
                        )?;
                        // The frame mark now owns all following dependency cells.
                        // Its environment must not keep a preparation coordinate
                        // that will disappear when the outer conversion retires.
                        let finish = (|| -> Result<_, FrameAllocationError> {
                            let environment = environment
                                .map(|value| {
                                    AdmittedEnvironment::publish(slots, self.session, value)
                                })
                                .transpose()?;
                            if let (Some(method), Some(value)) = (&mut invocation, computation) {
                                let copied =
                                    AdmittedApplication::publish(slots, self.session, value)?;
                                method.rebind_computation(copied)?;
                            }
                            Ok((environment, invocation))
                        })();
                        match finish {
                            Ok((environment, invocation)) => Ok((storage, environment, invocation)),
                            Err(error) => {
                                storage.retire(slots)?;
                                Err(error)
                            }
                        }
                    })())
                })
            });
            let (storage, environment, invocation) = allocation
                .map_err(|error| runtime.managed_memory_error(error))?
                .map_err(|error| match error {
                    FrameAllocationError::Storage(error) => runtime.managed_memory_error(error),
                    FrameAllocationError::Execution(error) => error,
                })?;
            let mut frame = ExecutionFrame::from_storage(
                loaded,
                target,
                storage,
                prepared.map_or(ReturnDestination::Slot(return_dst), |call| {
                    ReturnDestination::Prepared(call.destination)
                }),
            );
            frame.links = links;
            frame.environment = environment;
            Ok((
                frame,
                action,
                FrameUncommon::new(
                    invocation,
                    match entry {
                        FrameEntry::StaticInitializer(attempt) => Some(attempt),
                        FrameEntry::Call | FrameEntry::Await => None,
                    },
                    uncommon_space,
                ),
            ))
        };
        match prepare() {
            Ok((mut frame, action, mut uncommon)) => {
                if let Some(attempt) = uncommon
                    .as_mut()
                    .and_then(|state| state.initializer.as_mut())
                    && let Err(error) = attempt.begin(runtime)
                {
                    let error = runtime.resources.admit_execution_error(error);
                    frame.release_values(&mut runtime.memory, &runtime.resources);
                    runtime.resources.leave_call();
                    return Err(error);
                }
                frame.uncommon =
                    uncommon.map(|state| runtime.resources.uncommon_frames.insert(state));
                // All fallible operations have ended. No callback can invalidate
                // the capacity/stack identity checked before the source transaction.
                runtime
                    .resources
                    .sessions
                    .frames_mut(self.session)
                    .expect("frame preparation cannot reenter execution")
                    .push(frame);
                Ok(action)
            }
            Err(error) => {
                let error = runtime.resources.admit_execution_error(error);
                runtime.resources.leave_call();
                Err(error)
            }
        }
    }
}
