//! Closure selection is call policy; argument publication belongs to frame storage.
use crate::{
    Runtime,
    closure::{ClosureMode, ClosureRecord, calls::SelectedClosure},
    error::{RuntimeError, trap::TrapReason},
    execution::roots::ExecutionRoots,
    execution_metadata::managed::sources::MetadataSource,
    frame::{
        ExecutionFrame, FrameDispatch, FrameEntry, FrameEnvironment, FrameStack,
        arguments::FrameArguments, driver::ExecutionAction,
    },
    module::{
        execution::calls::{PreparedCall, PreparedClosureCall, PreparedClosureSignature},
        linked_execution::calls::LinkedCall,
        managed::source::RevisionSource,
    },
    native::conversion::{arguments::ArgumentValues, context::ConversionContext},
    value::{
        carriers::RootedValue, cells::CellObject, closures::ClosureObject, kind::ObjectKind,
        place::ResultPlace, view::ValueView,
    },
    value_check::{
        data::candidate_access,
        scoped::{self, CheckError},
    },
};

use kagari_bytecode::{instruction::Slot, module::CallableTarget};
use kagari_contract::{ids::FunctionRef, representation::semantic_representation};
use kagari_gc::error::GcError;
use kagari_types::ty::Ty;
use std::borrow::Cow;

impl FrameStack {
    pub(crate) fn make_closure(
        &self,
        runtime: &mut Runtime,
        function: FunctionRef,
        captures: &[Slot],
        destination: Slot,
    ) -> Result<(), RuntimeError> {
        self.validate_top(runtime)?;
        let (owner, environment, frame_slots, destination, values) = {
            let frame = self.current(runtime)?;
            let frame_slots = frame.slots();
            let destination = frame.slot_location(runtime.resources(), destination, false)?;
            let mut values = Vec::new();
            values
                .try_reserve_exact(captures.len())
                .map_err(|_| RuntimeError::resource_limit("closure captures"))?;
            for (index, register) in captures.iter().enumerate() {
                if index.is_multiple_of(1024) {
                    runtime.resources().poll_closed()?;
                }
                let location = frame.slot_location(runtime.resources(), *register, true)?;
                values.push(
                    frame_slots
                        .inspect(&runtime.memory, location)
                        .map_err(|error| runtime.managed_memory_error(error))?,
                );
            }
            (
                frame.loaded.clone(),
                frame.admitted_environment().cloned(),
                frame_slots,
                destination,
                values,
            )
        };
        {
            let mut conversion = ConversionContext::for_selected(
                runtime,
                &owner,
                frame_slots.banks.execution,
                RevisionSource::Execution(frame_slots.body),
                environment
                    .as_ref()
                    .map(MetadataSource::AdmittedEnvironment),
            )?;
            let value = conversion.allocate_script_closure(
                function,
                &values,
                environment
                    .as_ref()
                    .map(|environment| environment.facts().clone()),
                ClosureMode::Ordinary,
            )?;
            ResultPlace::frame(&frame_slots, Some(&destination))
                .publish(conversion.runtime_mut(), &value)?
                .admit_frame(&frame_slots, Some(&destination))?;
        }
        self.complete_source_write(runtime);
        Ok(())
    }

    pub fn push_closure(
        &self,
        runtime: &mut Runtime,
        value: &ValueView<'_>,
        args: ArgumentValues<'_>,
        return_dst: Option<Slot>,
    ) -> Result<(), RuntimeError> {
        self.validate_top(runtime)?;
        let selected = runtime.select_closure_call(self.session, value)?;
        self.push_selected_closure(
            runtime,
            &selected,
            FrameArguments::pack(args),
            return_dst,
            None,
        )
        .map(|_| ())
    }

    /// A genuinely retained closure supplies captures/body/environment directly.
    /// Named relationships must be selected and frozen before reaching this entry.
    pub(crate) fn push_rooted_closure(
        &self,
        runtime: &mut Runtime,
        root: &RootedValue,
        explicit: ArgumentValues<'_>,
    ) -> Result<(), RuntimeError> {
        self.validate_top(runtime)?;
        let closure = runtime
            .memory
            .scope(&runtime.resources, |cx, _| {
                let value = root.get(cx)?;
                let object =
                    cx.downcast::<ClosureObject>(value.object().ok_or(GcError::Representation)?)?;
                Ok(cx.read(object, |object| object.record().clone()))
            })
            .map_err(|error| runtime.managed_memory_error(error))?;
        if closure.call_route().is_some()
            || (matches!(closure.callable_target(), CallableTarget::Native(_))
                && !closure.capture_types.is_empty())
        {
            return Err(RuntimeError::module_validation(
                "rooted entry requires an exact closure",
            ));
        }
        let arguments = FrameArguments::pack(explicit.reborrow())
            .with_rooted_captures(root, closure.capture_types.len())
            .map_err(|error| runtime.managed_memory_error(error))?;
        closure_inputs(runtime, &closure, arguments, explicit.len())?;
        let environment = closure.environment.as_ref().map(|facts| FrameEnvironment {
            facts: facts.clone(),
            source: MetadataSource::RootedClosureEnvironment(root),
        });
        self.push_arguments(
            runtime,
            closure.implementation.clone(),
            closure.callable_target(),
            arguments,
            None,
            FrameDispatch {
                origin: RevisionSource::RootedClosure(root),
                metadata: None,
                prepared: None,
                entry: FrameEntry::Call,
                invocation: None,
                environment,
            },
        )
    }

    /// Admission consumes one selected meaning and the original capture source.
    /// It neither reselects named code nor borrows a conversion cleanup mark.
    pub(crate) fn push_selected_closure(
        &self,
        runtime: &mut Runtime,
        selected: &SelectedClosure<'_>,
        explicit: FrameArguments<'_, '_>,
        return_dst: Option<Slot>,
        prepared: Option<&PreparedCall>,
    ) -> Result<ExecutionAction, RuntimeError> {
        self.validate_top(runtime)?;
        let closure = selected.record();
        if matches!(selected.target(), CallableTarget::Native(_))
            && !closure.capture_types.is_empty()
        {
            return Err(RuntimeError::module_validation("native closure arguments"));
        }
        let arguments = explicit
            .with_captures(selected.value().reborrow(), closure.capture_types.len())
            .map_err(|error| runtime.managed_memory_error(error))?;
        closure_inputs(runtime, closure, arguments, explicit.len())?;
        let environment = selected
            .environment()
            .map(|(facts, source)| FrameEnvironment { facts, source });
        self.push_admitted_arguments(
            runtime,
            selected.owner().clone(),
            selected.target(),
            arguments,
            return_dst,
            FrameDispatch {
                origin: selected.origin(),
                metadata: None,
                prepared,
                entry: FrameEntry::Call,
                invocation: None,
                environment,
            },
        )
    }
}

impl ExecutionFrame {
    pub(super) fn validate_closure_call(
        &self,
        closure: &ClosureRecord,
        contract: &PreparedClosureCall,
        linked: Option<&LinkedCall>,
    ) -> Result<(), RuntimeError> {
        let matches = match &contract.signature {
            PreparedClosureSignature::Physical { params, result } => {
                closure.matches_physical_signature(params.iter().copied(), *result)
            }
            PreparedClosureSignature::Scoped => {
                let Some(LinkedCall::Closure(semantic)) = linked else {
                    return Err(RuntimeError::module_validation(
                        "missing linked closure contract",
                    ));
                };
                let semantic = semantic.as_ref().ok_or_else(|| {
                    RuntimeError::module_validation("shared closure call signature")
                })?;
                // Physical closure validation needs shape, not installation-key
                // adoption. Preserve the actual closure's checked scope below.
                let resolved: Cow<'_, Ty> = if semantic.is_concrete() {
                    Cow::Borrowed(semantic)
                } else {
                    Cow::Owned(
                        self.environment
                            .as_ref()
                            .ok_or_else(|| {
                                RuntimeError::module_validation("missing closure call environment")
                            })?
                            .facts()
                            .types
                            .resolve(semantic)?,
                    )
                };
                let Ty::Function { params, result } = resolved.as_ref() else {
                    return Err(RuntimeError::module_validation("shared closure call type"));
                };
                closure.matches_physical_signature(
                    params.iter().map(semantic_representation),
                    semantic_representation(result),
                )
            }
        };
        if !matches {
            return Err(RuntimeError::from(TrapReason::ClosureCallContract));
        }
        Ok(())
    }
}

/// Captured cells are storage handles while their type describes the contained
/// variable. Keep that rule here, outside physical frame copying and GC tracing.
pub(super) fn closure_inputs(
    runtime: &mut Runtime,
    closure: &ClosureRecord,
    arguments: FrameArguments<'_, '_>,
    explicit_count: usize,
) -> Result<(), RuntimeError> {
    if explicit_count != closure.signature().params.len() {
        return Err(match closure.callable_target() {
            CallableTarget::Script(_) => TrapReason::ClosureCallContractMismatch.into(),
            CallableTarget::Native(_) => {
                RuntimeError::module_validation("native closure arguments")
            }
        });
    }
    let Runtime {
        memory,
        host,
        host_borrows,
        resources,
        ..
    } = runtime;
    let checked = memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            Ok((|| -> Result<(), CheckError> {
                arguments.check_bounds(slots)?;
                let parameters = match closure.callable_target() {
                    CallableTarget::Script(function) => Some(
                        &closure.implementation.bytecode.functions[function.index()]
                            .metadata
                            .params,
                    ),
                    CallableTarget::Native(_) => None,
                };
                if parameters.is_some_and(|parameters| parameters.len() != arguments.len()) {
                    return Err(RuntimeError::from(TrapReason::ClosureCallContractMismatch).into());
                }
                for index in 0..arguments.len() {
                    if index.is_multiple_of(1024) {
                        resources.poll_closed()?;
                    }
                    let value = arguments.get(slots, index)?;
                    candidate_access(slots.scope(), value, resources)?;
                    if parameters
                        .is_some_and(|parameters| !value.has_representation(parameters[index]))
                    {
                        return Err(
                            RuntimeError::from(TrapReason::ClosureCallContractMismatch).into()
                        );
                    }
                    let captured = index < closure.capture_types.len();
                    let expected = if captured {
                        closure.capture_types[index].as_ref()
                    } else {
                        Some(&closure.signature().params[index - closure.capture_types.len()])
                    };
                    if let Some(expected) = expected {
                        let value = if captured && value.kind() == Some(ObjectKind::Cell) {
                            let cell = slots.scope().downcast::<CellObject>(
                                value.object().ok_or(GcError::Representation)?,
                            )?;
                            CellObject::get(
                                slots.scope(),
                                cell,
                                semantic_representation(expected.ty()),
                            )?
                        } else {
                            value
                        };
                        if !scoped::matches(
                            slots.scope(),
                            value,
                            expected.view(&closure.implementation),
                            host,
                            host_borrows,
                            resources,
                        )? {
                            return Err(RuntimeError::module_validation(
                                "closure semantic argument mismatch",
                            )
                            .into());
                        }
                    }
                }
                Ok(())
            })())
        })
    });
    checked
        .map_err(|error| runtime.managed_memory_error(error))?
        .map_err(|error| {
            let error = error.into_runtime(runtime);
            runtime.resources.admit_execution_error(error)
        })
}
