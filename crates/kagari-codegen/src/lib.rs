//! Native compilation consumes verified MIR and immutable ABI/link descriptions.
mod diagnostic;
pub use diagnostic::{BackendCompileError, BackendDiagnostic, BackendDiagnosticKind};
use kagari_abi::native::{
    BackendId, BackendTarget, NativeCompilationProduct, NativeLinkDescription,
};
use kagari_mir::analysis::FunctionAnalysis;
use kagari_mir::ids::InstanceId;
use kagari_mir::{MirFunction, VerifiedMirModule};

#[derive(Debug, Clone, Copy)]
pub struct BackendFunctionInput<'a> {
    module: &'a VerifiedMirModule,
    function: InstanceId,
    links: &'a NativeLinkDescription,
}
impl<'a> BackendFunctionInput<'a> {
    pub fn new(
        module: &'a VerifiedMirModule,
        function: InstanceId,
        links: &'a NativeLinkDescription,
    ) -> Option<Self> {
        module.functions.get(function.index())?;
        Some(Self {
            module,
            function,
            links,
        })
    }
    pub fn module(&self) -> &'a VerifiedMirModule {
        self.module
    }
    pub fn function(&self) -> &'a MirFunction {
        &self.module.functions[self.function.index()]
    }
    pub fn analysis(&self) -> &'a FunctionAnalysis {
        self.module
            .analysis(self.function)
            .expect("sealed function facts")
    }
    pub fn function_ref(&self) -> InstanceId {
        self.function
    }
    pub fn links(&self) -> &'a NativeLinkDescription {
        self.links
    }
}

pub trait CodegenBackend {
    fn backend_id(&self) -> BackendId;
    fn target(&self) -> BackendTarget;
    fn compile_function(
        &mut self,
        input: BackendFunctionInput<'_>,
    ) -> Result<NativeCompilationProduct, BackendCompileError>;
}
