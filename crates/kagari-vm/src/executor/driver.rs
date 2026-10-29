//! Frame transitions carry established actions; activation and waits admit runtime state.
use crate::{
    error::VmError,
    executor::{DriveOutcome, Executor, dispatch::InstructionProgress, native},
};
use kagari_bytecode::module::CallableTarget;
use kagari_runtime::{
    frame::{
        ExecutionStack,
        cursor::kernel::RegionExit,
        driver::{ExecutionAction, ReturnAction},
    },
    session::ExecutionEvent,
};
use std::task::Poll;

impl Executor<'_> {
    pub(super) fn run_inner(&mut self, remaining: Option<usize>) -> Result<DriveOutcome, VmError> {
        self.drive_inner(remaining).inspect_err(|error| {
            let cause = match error.cause() {
                VmError::RuntimeError(error) => error.to_string(),
                error => format!("{error:?}"),
            };
            self.stack().fail_static_initializers(&cause);
        })
    }

    fn drive_inner(&mut self, mut remaining: Option<usize>) -> Result<DriveOutcome, VmError> {
        // Initial/resumed activations and waits reclassify; internal frame
        // transitions provide their established action without another lookup.
        let mut action = None;
        loop {
            self.runtime.resources().poll_execution()?;
            if action.is_none() {
                action = Some(self.stack().next_action(self.runtime)?);
            }
            if action == Some(ExecutionAction::Await) {
                let resumed = unsafe { self.stack().poll_await_transport(self.runtime) }
                    .map_err(VmError::RuntimeError);
                match self.report_operation(resumed)? {
                    Poll::Pending => return Ok(DriveOutcome::Waiting),
                    Poll::Ready(Some(value)) => return Ok(DriveOutcome::Complete(value)),
                    Poll::Ready(None) => {
                        action = Some(self.stack().next_action(self.runtime)?);
                    }
                }
            }
            // A completed native result remains rooted in its frame if slicing parks here.
            if remaining == Some(0)
                && ExecutionStack::try_park_ready(&mut self.stack, self.runtime)?
            {
                return Ok(DriveOutcome::Runnable);
            }
            match action.expect("classified activation") {
                ExecutionAction::NativeReturn => {
                    let result =
                        unsafe { self.stack().finish_native_return_transport(self.runtime) }
                            .map_err(VmError::RuntimeError);
                    match self.report_operation(result)? {
                        ReturnAction::Complete(value) => return Ok(DriveOutcome::Complete(value)),
                        ReturnAction::Continue(next) => action = Some(next),
                    }
                    continue;
                }
                ExecutionAction::NativeEntry => {
                    if let Some(remaining) = &mut remaining {
                        *remaining = remaining.saturating_sub(1);
                    }
                    self.runtime.gc_safepoint()?;
                    self.runtime
                        .observe_execution(ExecutionEvent::BeforeInstruction)?;
                    let result = self
                        .stack()
                        .start_native_entry(self.runtime, native::INVOKE_SCRIPT)
                        .map_err(VmError::RuntimeError);
                    action = Some(self.report_operation(result)?);
                    continue;
                }
                ExecutionAction::Script => {}
                ExecutionAction::Await => {
                    return Err(VmError::RuntimeError(
                        self.runtime.quarantine_execution_invariant(
                            "ready wait retained a pending activation",
                        ),
                    ));
                }
            }
            self.current_frame_mut()?.prepare_instruction();
            self.runtime.gc_safepoint().map_err(VmError::RuntimeError)?;
            self.runtime
                .observe_execution(ExecutionEvent::BeforeInstruction)?;

            let result = unsafe {
                self.stack().execute_region_transport(
                    self.runtime,
                    &mut remaining,
                    native::INVOKE_SCRIPT,
                )
            }
            .map_err(VmError::from);
            let progress = match self.report_operation(result)? {
                RegionExit::Safepoint | RegionExit::Slice => continue,
                RegionExit::Call(next) => {
                    action = Some(next);
                    continue;
                }
                RegionExit::Return(value) => InstructionProgress::Return(value),
                RegionExit::Boundary => {
                    let (loaded, target, pc) = {
                        let frame = self.current_frame()?;
                        (
                            frame.loaded().clone(),
                            frame.target(),
                            frame.instruction_offset(),
                        )
                    };
                    let CallableTarget::Script(function) = target else {
                        return Err(VmError::UnsupportedInstruction(
                            "native boundary in script cursor",
                        ));
                    };
                    let instruction = loaded
                        .bytecode
                        .functions
                        .get(function.index())
                        .and_then(|function| function.instructions.get(pc))
                        .ok_or(VmError::UnsupportedInstruction(
                            "missing boundary instruction",
                        ))?;
                    let result = self.dispatch_instruction(instruction);
                    self.report_operation(result)?
                }
            };
            match progress {
                InstructionProgress::Continue => {}
                InstructionProgress::Await => action = None,
                InstructionProgress::Return(value) => {
                    let result =
                        unsafe { self.stack().finish_return_transport(self.runtime, value) }
                            .map_err(VmError::RuntimeError);
                    match self.report_operation(result)? {
                        ReturnAction::Complete(value) => return Ok(DriveOutcome::Complete(value)),
                        ReturnAction::Continue(next) => action = Some(next),
                    }
                }
            }
        }
    }

    fn report_operation<T>(&self, result: Result<T, VmError>) -> Result<T, VmError> {
        result.map_err(|error| {
            if let Some(reason) = error.invariant_reason() {
                return VmError::RuntimeError(self.runtime.quarantine_execution_invariant(reason));
            }
            let error = error.admit_execution_error(self.runtime.resources());
            match self.runtime.observe_execution(ExecutionEvent::Trap) {
                Ok(()) => error,
                Err(observer_error) => VmError::RuntimeError(observer_error),
            }
            .admit_execution_error(self.runtime.resources())
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::VmError, executor::Executor, tests::native_program::ProgramFixture};
    use kagari_abi::representation::ValueType;
    use kagari_bytecode::{
        instruction::BytecodeInstruction,
        module::{
            BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord, InstructionOrigin,
        },
        program::ModuleRef,
    };

    use kagari_contract::ids::FunctionRef;
    use kagari_runtime::{Runtime, error::RuntimeError};

    #[test]
    fn driver_admits_resource_errors_before_frames_unwind() {
        let mut runtime = Runtime::default();
        let function = BytecodeFunction {
            id: FunctionRef::new(0),
            name: "main".into(),
            instructions: vec![BytecodeInstruction::Return(None)],
            metadata: FunctionMetadata {
                origins: vec![InstructionOrigin {
                    instruction: 0,
                    span: Default::default(),
                    line: None,
                    column: None,
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        let program = ProgramFixture {
            root: ModuleRef::new(0),
            exports: Default::default(),
            modules: vec![BytecodeModule {
                types: vec![ValueType::Unit],
                function_table: vec![FunctionRecord {
                    id: function.id,
                    identity: function.identity.clone(),
                    name: function.name.clone(),
                    params: vec![],
                    return_type: function.metadata.return_type,
                    effects: Default::default(),
                }],
                functions: vec![function],
                ..Default::default()
            }],
        };
        let owner = runtime
            .load_program(
                "driver",
                program
                    .metadata(runtime.host().definitions(), &Default::default())
                    .unwrap(),
            )
            .unwrap();
        let _owner_program = owner;
        let owner = runtime.select_program(&_owner_program).unwrap();
        let executor = Executor::new(&runtime, &owner, FunctionRef::new(0), &[]).unwrap();
        let error = executor
            .report_operation::<()>(Err(VmError::from(RuntimeError::resource_limit(
                "driver operation",
            ))))
            .unwrap_err();
        let VmError::RuntimeError(error) = error else {
            panic!("resource classification")
        };
        assert_eq!(runtime.resources().termination(), Some(error.clone()));
        let trace = error.trace().unwrap();
        assert!(!trace.incomplete);
        assert_eq!(trace.frames.len(), 1);
        assert_eq!(trace.frames[0].function_name, "main");
        assert_eq!(runtime.resources().poll_execution().unwrap_err(), error);
        drop(executor);
        assert!(runtime.resources().termination().is_none());
        assert_eq!(runtime.resources().counters().current_call_depth, 0);
        assert_eq!(runtime.gc().active_roots(), 0);
    }
}
