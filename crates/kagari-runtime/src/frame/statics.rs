//! A static access suspends its own PC until the fixed recipe returns successfully.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{
        ExecutionStack, FrameDispatch, FrameEntry, arguments::FrameArguments, driver::ReturnAction,
        transfer::ReturnValue,
    },
};
use kagari_bytecode::{
    instruction::{BytecodeInstruction, ModuleSlot},
    module::CallableTarget,
};

impl ExecutionStack<'_> {
    /// Ensure the current verified static operand is ready. `false` means a hidden
    /// initializer frame was entered: the driver continues normally, then retries
    /// this same access PC. This grants no arbitrary function-entry authority.
    pub fn ensure_static_ready(
        &self,
        runtime: &Runtime,
        slot: ModuleSlot,
    ) -> Result<bool, RuntimeError> {
        self.validate_runtime(runtime)?;
        let (owner, pc) = {
            let frame = self.current()?;
            let pc = frame.executing.ok_or_else(|| {
                RuntimeError::module_validation("static access requires an executing instruction")
            })?;
            if !frame.function().and_then(|function| function.instructions.get(pc)).is_some_and(|instruction| {
                matches!(instruction, BytecodeInstruction::LoadModule { slot: operand, .. } | BytecodeInstruction::StoreModule { slot: operand, .. } if *operand == slot)
            }) {
                return Err(RuntimeError::module_validation("static access operand does not match current instruction"));
            }
            (frame.loaded.clone(), pc)
        };
        let Some(attempt) = runtime.prepare_static_initialization(&owner, slot)? else {
            return Ok(true);
        };
        let recipe = attempt.recipe().clone();
        self.push_admitted_arguments(
            runtime,
            recipe.module,
            CallableTarget::Script(recipe.function),
            FrameArguments::plain(&[]),
            None,
            FrameDispatch {
                prepared: None,
                entry: FrameEntry::StaticInitializer(attempt),
                invocation: None,
                environment: None,
            },
        )?;
        // Admission cannot call user code. The caller's windows and inputs remain
        // live, and only this access is retried, never an earlier RHS or index call.
        let mut frames = self
            .session
            .resources
            .sessions
            .frames_mut(self.session.id)
            .expect("initializer admission released the frame stack");
        let caller_index = frames.len() - 2;
        frames[caller_index].ip = pc;
        frames[caller_index].completed = None;
        Ok(false)
    }

    pub(super) fn finish_static_return(
        &self,
        runtime: &Runtime,
        packet: ReturnValue,
    ) -> Result<ReturnAction, RuntimeError> {
        let value = packet
            .materialize()
            .ok_or_else(|| RuntimeError::module_validation("initializer return representation"))?;
        // The callee window roots the value until this explicit root is admitted;
        // this root then spans retirement and complete semantic value admission.
        let _result = runtime
            .root_value(value)
            .ok_or_else(|| RuntimeError::module_validation("initializer return root"))?;
        let mut frames = self
            .session
            .resources
            .sessions
            .frames_mut(self.session.id)
            .ok_or_else(|| {
                runtime
                    .resources()
                    .quarantine("initializer return stack is borrowed")
            })?;
        let frame = frames.pop().expect("checked initializer return frame");
        let retired = frame.retire_values(self.session.resources).is_some();
        self.session.resources.leave_call();
        let initializer = frame
            .initializer
            .as_ref()
            .expect("checked initializer authority");
        let outcome = (|| {
            runtime.gc().ensure_execution_allowed()?;
            if !retired {
                return Err(runtime
                    .resources()
                    .quarantine("initializer return window was not retired"));
            }
            let caller = frames
                .last()
                .filter(|_| frames.len() > self.base)
                .ok_or_else(|| {
                    runtime
                        .resources()
                        .quarantine("initializer return lost its caller")
                })?;
            let action = caller.execution_action(runtime)?;
            initializer.commit(runtime, value)?;
            Ok(ReturnAction::Continue(action))
        })();
        if let Err(error) = &outcome {
            initializer.fail(&error.to_string());
        }
        // No fallible operation follows publication. Dropping the frame releases
        // its phase marker; the caller repeats only the suspended access PC.
        drop(frame);
        outcome
    }

    /// Preserve the real error before unwinding. No execution admission is required:
    /// cancellation, quarantine and observer failure must also settle attempts.
    pub fn fail_static_initializers(&self, cause: &str) {
        if let Some(frames) = self.session.resources.sessions.frames(self.session.id) {
            for frame in frames.iter().skip(self.base) {
                if let Some(initializer) = &frame.initializer {
                    initializer.fail(cause);
                }
            }
        }
    }
}
