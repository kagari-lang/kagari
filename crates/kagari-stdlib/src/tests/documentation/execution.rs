//! Execute displayed documentation against isolated mutable runtime state.
use kagari_common::cancellation::CancellationToken;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::{analysis::AnalysisSnapshot, compile_input::program::CheckedCompilation};
use kagari_runtime::{
    gc::roots::RootedValue,
    module::{VerifiedProgram, entry::ProgramEntry},
    native::installation::NativeInstallation,
    scheduler::TurnBudget,
    session::{ExecutionOptions, owned::OwnedExecution},
    value::Value,
};
use kagari_source::identity::FileId;
use kagari_vm::{
    error::VmError,
    vm::{Vm, owned::DriveResult},
};
use std::{
    num::NonZeroUsize,
    task::{Context, Poll, Waker},
    time::{Duration, Instant},
};

#[derive(Debug)]
pub(super) struct Observation {
    pub value: bool,
    pub compilation: Duration,
    pub execution: Duration,
}

#[derive(Debug, thiserror::Error)]
pub(super) enum Failure {
    #[error("documentation preparation failed: {0}")]
    Preparation(String),
    #[error("documentation execution failed: {0:?}")]
    Execution(VmError),
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self::Preparation(message)
    }
}

impl From<&str> for Failure {
    fn from(message: &str) -> Self {
        Self::Preparation(message.into())
    }
}

impl From<VmError> for Failure {
    fn from(error: VmError) -> Self {
        Self::Execution(error)
    }
}

#[derive(Default)]
pub(super) struct Setup<'a> {
    pub scope: Option<bool>,
    pub export: Option<&'a str>,
}

pub(super) fn run(
    snapshot: &AnalysisSnapshot,
    file: FileId,
    installation: &NativeInstallation,
    setup: Setup<'_>,
) -> Result<Observation, Failure> {
    let started = Instant::now();
    let cancel = CancellationToken::default();
    let checked = snapshot
        .check_program(file, &cancel)
        .map_err(|error| format!("check: {error:?}"))?;
    let input = CheckedCompilation::from_program(&checked, &cancel)
        .map_err(|error| format!("prepare: {error:?}"))?;
    let mir = lower_program_to_mir(&input, &Default::default())
        .map_err(|error| format!("lower: {error:?}"))?;
    let bytecode = lower_program_to_bytecode(&mir, &Default::default())
        .map_err(|error| format!("emit: {error:?}"))?;
    let mut runtime = installation.runtime(Default::default());
    let program = runtime
        .load_verified_program("documentation", VerifiedProgram::from_bytecode(bytecode))
        .map_err(|error| format!("load: {error:?}"))?;
    let module = runtime
        .select_program(&program)
        .map_err(|error| format!("select: {error:?}"))?;
    let vm = Vm::new(runtime);
    let scope = setup
        .scope
        .is_some()
        .then(|| {
            vm.runtime()
                .create_scheduled_task_scope(&module, Default::default())
        })
        .transpose()
        .map_err(|error| format!("scope: {error:?}"))?;
    if setup.scope == Some(true) {
        scope.as_ref().expect("closed scope setup").close();
    }
    let arguments = scope
        .as_ref()
        .map(|scope| {
            vec![
                scope
                    .capability()
                    .value(vm.runtime().gc())
                    .expect("scope root"),
            ]
        })
        .unwrap_or_default();
    let options = ExecutionOptions {
        cancellation: cancel.clone(),
        ..Default::default()
    };
    let entry = if let Some(export) = setup.export {
        ProgramEntry::Export(export)
    } else if setup.scope.is_some() {
        ProgramEntry::Export("main")
    } else {
        ProgramEntry::Main
    };
    let compilation = started.elapsed();
    let started = Instant::now();
    let evaluated = (|| -> Result<bool, Failure> {
        let owner = vm.start(&module, entry, &arguments, options.clone())?;
        let mut result = drive(&vm, &owner, started, &cancel)?;
        if !matches!(result.value(vm.runtime().gc()), Some(Value::Bool(_))) {
            let future = result.value(vm.runtime().gc()).ok_or("result root")?;
            let owner = vm.start_future(&future, options)?;
            result = drive(&vm, &owner, started, &cancel)?;
        }
        match result.value(vm.runtime().gc()) {
            Some(Value::Bool(value)) => Ok(value),
            other => Err(format!("expected bool, got {other:?}").into()),
        }
    })();
    if let Some(scope) = &scope {
        scope.close();
    }
    vm.runtime()
        .close()
        .map_err(|error| Failure::Execution(error.into()))?;
    if let Some(scope) = scope {
        assert!(scope.is_closed());
    }
    evaluated.map(|value| Observation {
        value,
        compilation,
        execution: started.elapsed(),
    })
}

fn drive(
    vm: &Vm,
    owner: &OwnedExecution,
    started: Instant,
    cancel: &CancellationToken,
) -> Result<RootedValue, Failure> {
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if started.elapsed() > Duration::from_secs(5) {
            cancel.cancel();
        }
        match vm.drive(owner, NonZeroUsize::new(4096).expect("positive quantum"))? {
            DriveResult::Complete(result) => {
                return result.map_err(Failure::Execution);
            }
            DriveResult::Runnable | DriveResult::Waiting => {
                if let Poll::Ready(Err(error)) =
                    vm.poll_scheduler(&mut cx, TurnBudget::default(), None)
                {
                    return Err(Failure::Execution(error));
                }
            }
        }
    }
}
