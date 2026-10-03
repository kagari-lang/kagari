//! Native compilation consumes verified MIR and immutable ABI/link descriptions.
pub mod diagnostic;
use crate::diagnostic::BackendCompileError;
use kagari_abi::native::{
    BackendId, BackendTarget, NativeCompilationProduct, NativeLinkDescription,
};
use kagari_mir::{
    analysis::FunctionAnalysis, function::MirFunction, ids::InstanceId, verify::VerifiedMirModule,
};

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

/// Complete code-generation identity for reuse across runtime installations.
/// Backends expose every setting affecting generated code in `options`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BackendConfiguration {
    pub backend: BackendId,
    pub target: BackendTarget,
    pub options: Vec<(String, String)>,
}

/// Trusted in-process native compiler. Ordinary safe plugin implementations cannot
/// authorize executable pointers for safe SDK installation.
///
/// # Safety
/// Given verified input and correctly bound helper symbols, successful products
/// must implement that exact function's semantics, point charges, roots and native
/// ABI without unwinding across C boundaries. Code must run on this host, including
/// its instruction-set features. Products must retain all executable pages and
/// referenced links independently of the backend's lifetime and later compiles.
/// `configuration` must identify all code-generation settings and remain accurate
/// for the compile call; equal configurations must permit sharing their products.
/// Unsupported input must fail before any script effects. Descriptors must match
/// the declared backend, target, function and current runtime/helper ABI.
pub unsafe trait CodegenBackend {
    fn configuration(&self) -> BackendConfiguration;

    fn compile_function(
        &mut self,
        input: BackendFunctionInput<'_>,
    ) -> Result<NativeCompilationProduct, BackendCompileError>;
}
