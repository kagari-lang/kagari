//! Test-only instruction harness for bytecode without a source export contract.
//!
//! Raw opcode fixtures select explicit slots. They still run through the ordinary
//! session, frame, native fallback and result-retention implementations; this does
//! not introduce an executable name lookup or a public host entry capability.
use crate::error::RuntimeError;

use crate::execution::{ExecutionReport, Runtime, native::PreparedNativeExecution};
use crate::{
    module::ExecutableModule,
    session::{ExecutionOptions, owned::OwnedExecution},
    value::Value,
};
use kagari_contract::ids::FunctionRef;

pub(crate) trait FixtureExecution {
    fn execute_fixture(
        &self,
        owner: &ExecutableModule,
        function: FunctionRef,
    ) -> Result<ExecutionReport, RuntimeError>;

    fn execute_fixture_prepared<'p>(
        &self,
        owner: &ExecutableModule,
        function: FunctionRef,
        prepared: impl Into<PreparedNativeExecution<'p>>,
    ) -> Result<ExecutionReport, RuntimeError>;

    fn start_fixture(
        &self,
        owner: &ExecutableModule,
        function: FunctionRef,
        arguments: &[Value],
        options: ExecutionOptions,
    ) -> Result<OwnedExecution, RuntimeError>;
}

impl FixtureExecution for Runtime {
    fn execute_fixture(
        &self,
        owner: &ExecutableModule,
        function: FunctionRef,
    ) -> Result<ExecutionReport, RuntimeError> {
        let (owner, function) = self.select_function(owner, function)?;
        let session = self.begin_interpreter_execution(&owner, self.execution_options())?;
        self.execute_selected(&owner, function, "instruction fixture", &session)
    }

    fn execute_fixture_prepared<'p>(
        &self,
        owner: &ExecutableModule,
        function: FunctionRef,
        prepared: impl Into<PreparedNativeExecution<'p>>,
    ) -> Result<ExecutionReport, RuntimeError> {
        let (owner, function) = self.select_function(owner, function)?;
        let session = self.begin_interpreter_execution(&owner, self.execution_options())?;
        self.execute_prepared_selected(&owner, function, "instruction fixture", prepared, &session)
    }

    fn start_fixture(
        &self,
        owner: &ExecutableModule,
        function: FunctionRef,
        arguments: &[Value],
        options: ExecutionOptions,
    ) -> Result<OwnedExecution, RuntimeError> {
        let (owner, function) = self.select_function(owner, function)?;
        self.start_owned_execution(&owner, function, arguments, options)
    }
}
