//! Test-only instruction harness for bytecode without a source export contract.
//!
//! Raw opcode fixtures select explicit slots. They still run through the ordinary
//! session, frame, native fallback and result-retention implementations; this does
//! not introduce an executable name lookup or a public host entry capability.

use crate::{
    error::VmError,
    vm::{ExecutionReport, Vm, native::PreparedNativeEntry},
};
use kagari_contract::ids::FunctionRef;
use kagari_runtime::{
    module::ExecutableModule,
    session::{ExecutionOptions, owned::OwnedExecution},
    value::Value,
};

pub(crate) trait FixtureExecution {
    fn execute_fixture(
        &self,
        owner: &ExecutableModule,
        function: FunctionRef,
    ) -> Result<ExecutionReport, VmError>;

    fn execute_fixture_prepared(
        &self,
        owner: &ExecutableModule,
        function: FunctionRef,
        prepared: &PreparedNativeEntry,
    ) -> Result<ExecutionReport, VmError>;

    fn start_fixture(
        &self,
        owner: &ExecutableModule,
        function: FunctionRef,
        arguments: &[Value],
        options: ExecutionOptions,
    ) -> Result<OwnedExecution, VmError>;
}

impl FixtureExecution for Vm {
    fn execute_fixture(
        &self,
        owner: &ExecutableModule,
        function: FunctionRef,
    ) -> Result<ExecutionReport, VmError> {
        let session = self.begin_execution(owner)?;
        self.runtime.validate_executable_module(owner)?;
        self.execute_selected(owner, function, "instruction fixture", &session)
    }

    fn execute_fixture_prepared(
        &self,
        owner: &ExecutableModule,
        function: FunctionRef,
        prepared: &PreparedNativeEntry,
    ) -> Result<ExecutionReport, VmError> {
        let session = self.begin_execution(owner)?;
        self.runtime.validate_executable_module(owner)?;
        self.execute_prepared_selected(owner, function, "instruction fixture", prepared, &session)
    }

    fn start_fixture(
        &self,
        owner: &ExecutableModule,
        function: FunctionRef,
        arguments: &[Value],
        options: ExecutionOptions,
    ) -> Result<OwnedExecution, VmError> {
        self.runtime
            .start_owned_execution(owner, function, arguments, options)
            .map_err(VmError::from)
    }
}
