//! Source factories, nested awaits and owned iteration across real native waits.
use crate::{
    engine::{KagariEngine, builder::KagariEngineBuilder},
    program::PreparedProgram,
    runtime::Runtime,
};
use kagari_runtime::value::Value;
use kagari_vm::vm::owned::DriveResult;

// Shared with the source-free standalone consumer; it uses these parent imports.
#[path = "../../../../../tests/support/async_fixture.rs"]
mod fixture;
use crate::{BytecodeArtifact, RunResult, engine::EngineConfig, error::EmbeddingError};
use fixture::{Fixture, slice};
use kagari_runtime::{gc::roots::RootedValue, session::owned::OwnedExecution};
use kagari_source::source::SourceFile;
use std::num::NonZeroUsize;

mod blocks;
mod factories;
mod group_boundaries;
mod groups;
mod iteration;
mod scheduler;

mod tasks;

mod static_values;

mod task_waiters;

mod task_handlers;

mod lifecycle;

mod diagnostics;

mod debugger;

mod conversion;

impl Fixture {
    fn source(text: &str) -> Self {
        Self::configured_source(text, Default::default())
    }

    fn configured_source(text: &str, config: EngineConfig) -> Self {
        Self::with_source(text, config, |_| {})
    }

    fn with_source(
        text: &str,
        config: EngineConfig,
        install: impl FnOnce(&mut KagariEngineBuilder),
    ) -> Self {
        Self::with_prepared_program(config, install, |engine, _, _| prepare_source(engine, text))
    }

    fn invoke(&self, entry: &str, arguments: &[Value]) -> RootedValue {
        self.call(entry, arguments).unwrap()
    }

    fn call(&self, entry: &str, arguments: &[Value]) -> RunResult<RootedValue> {
        if arguments.is_empty() {
            return self
                .runtime
                .execute(&self.module, entry, arguments, &Default::default())
                .map(|report| report.return_value);
        }
        let owner =
            self.runtime
                .backend()
                .start(&self.module, entry, arguments, Default::default())?;
        for _ in 0..1000 {
            match self.runtime.backend().drive(&owner, slice())? {
                DriveResult::Complete(result) => return result.map_err(EmbeddingError::from),
                DriveResult::Runnable => {}
                DriveResult::Waiting => panic!("synchronous entry waited"),
            }
        }
        panic!("bounded ordinary call");
    }

    fn value(&self, root: &RootedValue) -> Value {
        root.value(self.runtime.runtime().gc()).unwrap()
    }

    fn start_future(&self, future: &RootedValue) -> OwnedExecution {
        self.runtime
            .backend()
            .start_future(&self.value(future), Default::default())
            .unwrap()
    }

    fn complete(&self, owner: &OwnedExecution) -> RootedValue {
        for _ in 0..1000 {
            match self
                .runtime
                .backend()
                .drive(owner, NonZeroUsize::new(1).unwrap())
                .unwrap()
            {
                DriveResult::Runnable => {
                    self.runtime.runtime().collect_garbage().unwrap();
                }
                DriveResult::Complete(value) => return value.unwrap(),
                DriveResult::Waiting => panic!("unexpected native wait"),
            }
        }
        panic!("bounded completion");
    }
}

fn prepare_source(engine: &KagariEngine, text: &str) -> PreparedProgram {
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("async-script.kgr", text),
            Default::default(),
        )
        .unwrap();
    let artifact =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap()
}
