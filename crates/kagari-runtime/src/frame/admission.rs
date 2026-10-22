//! One transaction admits arguments, enters depth and publishes the selected frame.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};

use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::MetadataRoot,
    frame::{
        ExecutionFrame, ExecutionStack, FrameDispatch, FrameEntry, NativeEntryState,
        ReturnDestination, arguments::FrameArguments, driver::ExecutionAction,
        values::FrameMetadata,
    },
    module::LoadedModule,
};
use kagari_bytecode::{instruction::Slot, module::CallableTarget};

impl ExecutionStack<'_> {
    /// Reuse a checked stack only across internal preparation without callbacks.
    pub(super) fn push_admitted_arguments(
        &self,
        runtime: &Runtime,
        loaded: LoadedModule,
        target: CallableTarget,
        args: FrameArguments<'_>,
        return_dst: Option<Slot>,
        dispatch: FrameDispatch<'_>,
    ) -> Result<ExecutionAction, RuntimeError> {
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::CallAdmission);
        runtime.admit_static_execution(&loaded)?;
        if let CallableTarget::Script(function) = target
            && loaded.is_static_initializer(function)
        {
            return Err(RuntimeError::module_validation(
                "static initializer requires runtime-owned transition authority",
            ));
        }
        let heap = runtime.gc();
        let resources = runtime.resources();
        let mut heap_arguments_valid = true;
        if !args.all_managed(runtime, |value| {
            if !heap.validate_candidate_value(value) {
                return false;
            }
            // Heap rejection is deferred. Even after it fails, finish checking
            // candidate membership and source windows in their original order.
            heap_arguments_valid = heap_arguments_valid && heap.validate_value(value);
            true
        })? {
            return Err(RuntimeError::execution_phase_violation(
                "external object in candidate call arguments",
            ));
        }
        if loaded
            .execution()
            .callee(target)
            .is_some_and(|callee| callee.may_suspend)
            && (self.session.state().owner.borrow().is_none()
                || (!self.frames()?.is_empty() && !matches!(dispatch.entry, FrameEntry::Await)))
        {
            return Err(RuntimeError::module_validation(
                "resume body requires an owned driver entry",
            ));
        }
        let mut frames = resources
            .sessions
            .frames_mut(self.session.id)
            .ok_or_else(|| resources.quarantine("frame stack is borrowed across execution"))?;
        if frames.try_reserve(1).is_err() {
            drop(frames);
            return Err(resources.limit("frame capacity"));
        }
        // Depth polling must be able to capture the suspended caller frames.
        drop(frames);
        resources.enter_call()?;
        let mut frames = resources
            .sessions
            .frames_mut(self.session.id)
            .expect("frame preparation cannot reenter execution");
        // Keep fallible preparation inside the depth rollback boundary. Nothing
        // here can collect, invoke user code or mutate the inspected arguments.
        let prepare = || {
            let FrameDispatch {
                prepared,
                entry: _,
                invocation,
                environment,
            } = dispatch;
            let callee = loaded
                .execution()
                .callee(target)
                .ok_or_else(|| resources.quarantine("invalid prepared callee layout"))?;
            let (native_entry, action) = match target {
                CallableTarget::Script(function) => {
                    if args.len() != callee.parameter_count {
                        return Err(RuntimeError::module_validation(
                            "frame argument count does not match the linked function",
                        ));
                    }
                    if callee.generic
                        && loaded.bytecode.functions[function.index()]
                            .metadata
                            .semantic
                            .generic
                            .as_ref()
                            .is_some_and(|body| {
                                environment
                                    .as_ref()
                                    .is_none_or(|environment| !environment.types.matches(body))
                            })
                    {
                        return Err(RuntimeError::module_validation(
                            "shared generic entry requires a call environment",
                        ));
                    }
                    (NativeEntryState::Script, ExecutionAction::Script)
                }
                CallableTarget::Native(import) => {
                    if callee.generic
                        && loaded.bytecode.native_imports[import.index()]
                            .generic
                            .as_ref()
                            .is_some_and(|body| {
                                environment
                                    .as_ref()
                                    .is_none_or(|environment| !environment.types.matches(body))
                            })
                    {
                        return Err(RuntimeError::module_validation(
                            "shared native frame environment",
                        ));
                    }
                    if args.len() != callee.parameter_count {
                        return Err(RuntimeError::module_validation("native frame arguments"));
                    }
                    (NativeEntryState::Pending, ExecutionAction::NativeEntry)
                }
            };
            let slot_count = callee.slot_layout.locations.len();
            let parameter_slots = &callee.parameter_slots;
            let slot_layout = Some(callee.slot_layout.clone());

            heap.ensure_execution_allowed()?;
            if !heap_arguments_valid {
                return Err(RuntimeError::module_validation("invalid heap argument"));
            }
            runtime.validate_metadata(MetadataRoot::Program(loaded.clone()).edge())?;
            if let Some(environment) = &environment {
                runtime.validate_environment(environment.id)?;
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
                                environment.as_ref().map(|scope| scope.id),
                            )
                            .ok_or_else(|| {
                                RuntimeError::module_validation("missing linked function execution")
                            })?,
                    )
                }
                _ => None,
            };
            let slots = resources
                .frame_values
                .try_borrow_mut()
                .map_err(|_| resources.quarantine("execution slots borrowed during frame entry"))?
                .allocate(
                    slot_count,
                    parameter_slots,
                    &args,
                    FrameMetadata {
                        program: loaded.clone(),
                        environment: environment.clone(),
                        invocation,
                    },
                    slot_layout.clone(),
                )?;
            Ok((
                ExecutionFrame {
                    links,
                    environment,
                    loaded,
                    target,
                    native_entry,
                    ip: 0,
                    executing: None,
                    completed: None,
                    native_origin: None,
                    slots,
                    slot_count,
                    slot_layout,
                    return_to: prepared.map_or(ReturnDestination::Slot(return_dst), |call| {
                        ReturnDestination::Prepared(call.destination)
                    }),
                    invocation,
                    mutations: Vec::new(),
                },
                action,
            ))
        };
        match prepare() {
            Ok((frame, action)) => {
                frames.push(frame);
                Ok(action)
            }
            Err(error) => {
                drop(frames);
                resources.leave_call();
                Err(resources.admit_execution_error(error))
            }
        }
    }
}
