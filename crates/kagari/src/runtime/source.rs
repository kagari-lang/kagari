//! Source inspection forwards to the source-free Runtime diagnostic owner.
use crate::{RunResult, runtime::Runtime};
use kagari_contract::source::AuthoredDeclaration;
use kagari_runtime::{
    frame::ExecutionFrame,
    gc::roots::RootedValue,
    module::Program,
    source::{FunctionSourceTarget, ProgramSourceSnapshot, SourceDeclaration, SourceQuery},
};

impl Runtime {
    pub fn source_snapshot(&self, program: &Program) -> RunResult<ProgramSourceSnapshot> {
        Ok(self.runtime().source_snapshot(program)?)
    }

    pub fn function_source(&self, target: &FunctionSourceTarget) -> RunResult<SourceQuery> {
        Ok(self.runtime().function_source(target)?)
    }

    pub fn value_source_target(&self, value: &RootedValue) -> RunResult<FunctionSourceTarget> {
        Ok(self.runtime().value_source_target(value)?)
    }

    pub fn frame_source_target(&self, frame: &ExecutionFrame) -> RunResult<FunctionSourceTarget> {
        Ok(self.runtime().frame_source_target(frame)?)
    }

    pub fn resolve_source_declaration(
        &self,
        program: &Program,
        declaration: &SourceDeclaration,
    ) -> RunResult<AuthoredDeclaration> {
        Ok(self
            .runtime()
            .resolve_source_declaration(program, declaration)?)
    }
}
