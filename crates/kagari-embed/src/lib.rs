use kagari_common::{Diagnostic, Severity, SourceFile, Span};
use kagari_hir::{AnalyzedModule, analyze_module};
use kagari_ir::{
    IrLoweringError,
    bytecode::{
        ArtifactBuildOptions, ArtifactCompatibility, ArtifactFingerprint, ArtifactModuleIdentity,
        ArtifactValidationError, BytecodeInstruction, BytecodeLoweringError, BytecodeModule,
        CallTarget, KbcArtifact, PathDescriptorFingerprint, RuntimeHelper, lower_to_bytecode,
    },
    lower_to_ir,
};
use kagari_runtime::{
    CapabilitySet, HostFunctionId, HostTypeRegistration, LanguageProfile, LoadedModule, ModuleId,
    ResourcePolicy, Runtime, RuntimeConfig, RuntimeError, RuntimeErrorKind, SecurityContext,
    TypeId, host::HostFunction, value::Value,
};
use kagari_syntax::parse_module;
use kagari_vm::{ExecutionReport, Vm, VmError};

pub type CompileResult<T> = Result<T, EmbeddingError>;
pub type LoadResult<T> = Result<T, EmbeddingError>;
pub type RunResult<T> = Result<T, EmbeddingError>;
pub type ReloadResult<T> = Result<T, EmbeddingError>;

pub type BytecodeArtifact = KbcArtifact;

#[derive(Debug, Clone, Default)]
pub struct EngineConfig {
    pub default_runtime: RuntimeConfig,
}

#[derive(Debug, Default)]
pub struct KagariEngine {
    config: EngineConfig,
}

impl KagariEngine {
    pub fn new(config: EngineConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    pub fn runtime(&self, context: ExecutionContext) -> KagariRuntime {
        let mut config = self.config.default_runtime;
        config.security = context.security_context();
        config.resources = context.resources;
        KagariRuntime::new(Runtime::new(config), context)
    }

    pub fn compile_source(
        &self,
        source: SourceFile,
        options: CompileOptions,
    ) -> CompileResult<CheckedModule> {
        let module_identity = options
            .module_identity
            .unwrap_or_else(|| ArtifactModuleIdentity::single_file(source.name()));
        let ast = parse_module(&source).map_err(EmbeddingError::diagnostics)?;
        let analyzed = analyze_module(&ast).map_err(EmbeddingError::diagnostics)?;
        Ok(CheckedModule {
            source_name: source.name().to_owned(),
            module_identity,
            analyzed,
        })
    }

    pub fn emit_bytecode(
        &self,
        checked: &CheckedModule,
        options: ArtifactOptions,
    ) -> CompileResult<BytecodeArtifact> {
        let ir = lower_to_ir(&checked.analyzed).map_err(EmbeddingError::ir_lowering)?;
        let module = lower_to_bytecode(&ir).map_err(EmbeddingError::bytecode_lowering)?;
        let mut build = options.build;
        if options.use_checked_module_identity {
            build.module_identity = checked.module_identity.clone();
        }
        Ok(KbcArtifact::from_module(module, build))
    }

    pub fn compile_to_artifact(
        &self,
        source: SourceFile,
        compile_options: CompileOptions,
        artifact_options: ArtifactOptions,
    ) -> CompileResult<BytecodeArtifact> {
        let checked = self.compile_source(source, compile_options)?;
        self.emit_bytecode(&checked, artifact_options)
    }
}

#[derive(Debug)]
pub struct KagariRuntime {
    vm: Vm,
    default_context: ExecutionContext,
}

impl KagariRuntime {
    pub fn new(runtime: Runtime, default_context: ExecutionContext) -> Self {
        Self {
            vm: Vm::new(runtime),
            default_context,
        }
    }

    pub fn runtime(&self) -> &Runtime {
        self.vm.runtime()
    }

    pub fn runtime_mut(&mut self) -> &mut Runtime {
        self.vm.runtime_mut()
    }

    pub fn default_context(&self) -> &ExecutionContext {
        &self.default_context
    }

    pub fn register_host_function(
        &mut self,
        function: HostFunction,
    ) -> Result<HostFunctionId, RuntimeError> {
        self.vm.runtime_mut().register_host_function(function)
    }

    pub fn register_host_type(
        &mut self,
        registration: HostTypeRegistration,
    ) -> Result<TypeId, RuntimeError> {
        self.vm.runtime_mut().register_host_type(registration)
    }

    pub fn load_module(
        &mut self,
        artifact: BytecodeArtifact,
        options: LoadOptions,
    ) -> LoadResult<LoadedModule> {
        artifact
            .validate_for_loader(&options.compatibility)
            .map_err(EmbeddingError::artifact_validation)?;
        let module_name = options
            .module_name
            .unwrap_or_else(|| artifact.header.module_identity.source_uri.clone());
        self.vm
            .runtime_mut()
            .load_module(module_name, artifact.module)
            .map_err(EmbeddingError::load)
    }

    pub fn reload_module(
        &mut self,
        previous: &LoadedModule,
        artifact: BytecodeArtifact,
        options: ReloadOptions,
    ) -> ReloadResult<LoadedModule> {
        artifact
            .validate_for_loader(&options.compatibility)
            .map_err(EmbeddingError::reload_validation)?;
        let module_name = options
            .module_name
            .unwrap_or_else(|| artifact.header.module_identity.source_uri.clone());
        if module_name != previous.name {
            return Err(EmbeddingError::reload_validation(
                ReloadValidationError::ModuleIdentityMismatch {
                    expected: previous.name.clone(),
                    found: module_name,
                },
            ));
        }
        let previous_path_fingerprints = path_fingerprints_for_module(&previous.bytecode);
        if previous_path_fingerprints != artifact.verification.typed_path_fingerprints {
            return Err(EmbeddingError::reload_validation(
                ReloadValidationError::PathFingerprintMismatch,
            ));
        }
        let reloaded = self
            .vm
            .runtime_mut()
            .load_module(previous.name.clone(), artifact.module)
            .map_err(EmbeddingError::load)?;
        if reloaded.id != previous.id {
            return Err(EmbeddingError::reload_validation(
                ReloadValidationError::ModuleIdChanged {
                    expected: previous.id,
                    found: reloaded.id,
                },
            ));
        }
        Ok(reloaded)
    }

    pub fn execute(
        &mut self,
        module: &LoadedModule,
        entry: &str,
        args: &[Value],
        context: &ExecutionContext,
    ) -> RunResult<ExecutionReport> {
        if !args.is_empty() {
            return Err(EmbeddingError::runtime(
                RuntimeFailureKind::UnsupportedExecution,
                format!(
                    "entry `{entry}` received {} arguments, but argument passing is not implemented",
                    args.len()
                ),
            ));
        }
        context.validate_for_execute(entry, &module.bytecode)?;
        self.vm.execute(module, entry).map_err(EmbeddingError::vm)
    }

    pub fn execute_module(
        &mut self,
        module: &LoadedModule,
        context: &ExecutionContext,
    ) -> RunResult<Value> {
        context.validate_for_execute("__module_init__", &module.bytecode)?;
        self.vm.execute_module(module).map_err(EmbeddingError::vm)
    }
}

#[derive(Debug)]
pub struct CheckedModule {
    pub source_name: String,
    pub module_identity: ArtifactModuleIdentity,
    analyzed: AnalyzedModule,
}

impl CheckedModule {
    pub fn analyzed(&self) -> &AnalyzedModule {
        &self.analyzed
    }
}

#[derive(Debug, Clone, Default)]
pub struct CompileOptions {
    pub module_identity: Option<ArtifactModuleIdentity>,
}

#[derive(Debug, Clone)]
pub struct ArtifactOptions {
    pub build: ArtifactBuildOptions,
    pub use_checked_module_identity: bool,
}

impl Default for ArtifactOptions {
    fn default() -> Self {
        Self {
            build: ArtifactBuildOptions::default(),
            use_checked_module_identity: true,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct LoadOptions {
    pub module_name: Option<String>,
    pub compatibility: ArtifactCompatibility,
}

#[derive(Debug, Clone, Default)]
pub struct ReloadOptions {
    pub module_name: Option<String>,
    pub compatibility: ArtifactCompatibility,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JitPolicy {
    Disabled,
    Enabled,
    CompileOnLoad,
    CompileOnFirstCall,
    CompileAfterThreshold(u32),
}

impl Default for JitPolicy {
    fn default() -> Self {
        Self::Disabled
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanicPolicy {
    Propagate,
    ConvertToError,
}

impl Default for PanicPolicy {
    fn default() -> Self {
        Self::ConvertToError
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HostExposurePolicy {
    pub allow_host_functions: bool,
    pub allow_host_path_mutation: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionContext {
    pub language_profile: LanguageProfile,
    pub capabilities: CapabilitySet,
    pub resources: ResourcePolicy,
    pub host_policy: HostExposurePolicy,
    pub jit_policy: JitPolicy,
    pub tracing_enabled: bool,
    pub panic_policy: PanicPolicy,
}

impl ExecutionContext {
    pub fn security_context(self) -> SecurityContext {
        SecurityContext {
            profile: self.language_profile,
            capabilities: self.capabilities,
        }
    }

    fn validate_for_execute(&self, entry: &str, module: &BytecodeModule) -> RunResult<()> {
        if self.jit_policy != JitPolicy::Disabled {
            return Err(EmbeddingError::runtime(
                RuntimeFailureKind::UnsupportedExecution,
                format!("JIT policy for `{entry}` is not implemented by the baseline runtime"),
            ));
        }
        self.validate_bytecode_policy(module)
    }

    fn validate_bytecode_policy(&self, module: &BytecodeModule) -> RunResult<()> {
        let security = self.security_context();
        for function in &module.functions {
            for instruction in &function.instructions {
                match instruction {
                    BytecodeInstruction::Call {
                        callee: CallTarget::RuntimeHelper(helper),
                        ..
                    } => match helper {
                        RuntimeHelper::HostFunction(symbol)
                            if !self.host_policy.allow_host_functions =>
                        {
                            return Err(EmbeddingError::runtime(
                                RuntimeFailureKind::CapabilityDenied,
                                format!("host function `{symbol}` is denied by execution context"),
                            ));
                        }
                        RuntimeHelper::ReflectTypeOf | RuntimeHelper::ReflectGetField(_)
                            if !security.allows_reflection_read() =>
                        {
                            return Err(EmbeddingError::runtime(
                                RuntimeFailureKind::CapabilityDenied,
                                "reflection read is denied by execution context",
                            ));
                        }
                        RuntimeHelper::ReflectSetField(_) | RuntimeHelper::ReflectSetIndex
                            if !security.allows_reflection_write() =>
                        {
                            return Err(EmbeddingError::runtime(
                                RuntimeFailureKind::CapabilityDenied,
                                "reflection write is denied by execution context",
                            ));
                        }
                        _ => {}
                    },
                    BytecodeInstruction::SetPath { .. }
                    | BytecodeInstruction::ModifyPath { .. }
                        if !self.host_policy.allow_host_path_mutation =>
                    {
                        return Err(EmbeddingError::runtime(
                            RuntimeFailureKind::CapabilityDenied,
                            "host path mutation is denied by execution context",
                        ));
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
}

impl Default for ExecutionContext {
    fn default() -> Self {
        Self {
            language_profile: LanguageProfile::default(),
            capabilities: CapabilitySet::default(),
            resources: ResourcePolicy::default(),
            host_policy: HostExposurePolicy {
                allow_host_functions: true,
                allow_host_path_mutation: false,
            },
            jit_policy: JitPolicy::default(),
            tracing_enabled: false,
            panic_policy: PanicPolicy::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingDiagnostic {
    pub severity: Severity,
    pub code: String,
    pub span: Option<Span>,
    pub message: String,
    pub notes: Vec<String>,
    pub labels: Vec<DiagnosticLabel>,
}

impl EmbeddingDiagnostic {
    fn from_diagnostic(diagnostic: Diagnostic) -> Self {
        Self {
            severity: diagnostic.severity,
            code: format!("{:?}", diagnostic.kind),
            span: diagnostic.span,
            message: diagnostic.kind.to_string(),
            notes: Vec::new(),
            labels: diagnostic
                .span
                .into_iter()
                .map(|span| DiagnosticLabel {
                    span,
                    message: "primary".to_owned(),
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticLabel {
    pub span: Span,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompilationPhase {
    Parse,
    Analyze,
    IrLowering,
    BytecodeLowering,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeFailureKind {
    ScriptTrap,
    BytecodeVerification,
    CapabilityDenied,
    ResourceLimitExceeded,
    HostCallFailure,
    TypedPathValidation,
    StaleModuleOrHostRoot,
    ReloadValidation,
    EngineInvariant,
    UnsupportedExecution,
}

#[derive(Debug)]
pub enum EmbeddingError {
    Diagnostics {
        diagnostics: Vec<EmbeddingDiagnostic>,
    },
    Compilation {
        phase: CompilationPhase,
        message: String,
    },
    ArtifactValidation {
        error: ArtifactValidationError,
    },
    Load {
        error: RuntimeError,
    },
    Runtime {
        kind: RuntimeFailureKind,
        message: String,
    },
    ReloadValidation {
        message: String,
    },
}

impl EmbeddingError {
    fn diagnostics(diagnostics: Box<smallvec::SmallVec<[Diagnostic; 4]>>) -> Self {
        Self::Diagnostics {
            diagnostics: diagnostics
                .into_vec()
                .into_iter()
                .map(EmbeddingDiagnostic::from_diagnostic)
                .collect(),
        }
    }

    fn ir_lowering(error: IrLoweringError) -> Self {
        Self::Compilation {
            phase: CompilationPhase::IrLowering,
            message: format!("{error:?}"),
        }
    }

    fn bytecode_lowering(error: BytecodeLoweringError) -> Self {
        Self::Compilation {
            phase: CompilationPhase::BytecodeLowering,
            message: format!("{error:?}"),
        }
    }

    fn artifact_validation(error: ArtifactValidationError) -> Self {
        Self::ArtifactValidation { error }
    }

    fn load(error: RuntimeError) -> Self {
        Self::Load { error }
    }

    fn runtime(kind: RuntimeFailureKind, message: impl Into<String>) -> Self {
        Self::Runtime {
            kind,
            message: message.into(),
        }
    }

    fn reload_validation(error: impl Into<ReloadValidationError>) -> Self {
        Self::ReloadValidation {
            message: error.into().to_string(),
        }
    }

    fn vm(error: VmError) -> Self {
        let kind = match &error {
            VmError::HostError(_) => RuntimeFailureKind::HostCallFailure,
            VmError::RuntimeError(error) => match error.kind() {
                RuntimeErrorKind::CapabilityDenied => RuntimeFailureKind::CapabilityDenied,
                RuntimeErrorKind::ResourceLimitExceeded => {
                    RuntimeFailureKind::ResourceLimitExceeded
                }
                RuntimeErrorKind::StaleHandle => RuntimeFailureKind::StaleModuleOrHostRoot,
                RuntimeErrorKind::HostBorrowConflict
                | RuntimeErrorKind::HostBorrowEscape
                | RuntimeErrorKind::ExpiredHostBorrow
                | RuntimeErrorKind::TypedPathValidation => RuntimeFailureKind::TypedPathValidation,
                RuntimeErrorKind::InvalidReflectiveWrite | RuntimeErrorKind::MetadataConflict => {
                    RuntimeFailureKind::ScriptTrap
                }
            },
            VmError::BytecodeVerification(_) => RuntimeFailureKind::BytecodeVerification,
            VmError::InvalidFunctionRef(_)
            | VmError::InvalidFrameArity { .. }
            | VmError::InvalidJumpTarget(_)
            | VmError::InvalidRegister(_)
            | VmError::InvalidLocal(_)
            | VmError::InvalidModuleSlot(_)
            | VmError::UnsupportedCallTarget(_)
            | VmError::UnsupportedInstruction(_) => RuntimeFailureKind::BytecodeVerification,
            VmError::MissingFunction(_)
            | VmError::MissingField(_)
            | VmError::ImmutableModuleSlot(_)
            | VmError::InvalidIndex(_)
            | VmError::InvalidBranchCondition
            | VmError::BuiltinError(_)
            | VmError::ReflectionError(_)
            | VmError::Trap(_)
            | VmError::TypeMismatch(_) => RuntimeFailureKind::ScriptTrap,
        };
        Self::runtime(kind, format!("{error:?}"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReloadValidationError {
    Artifact(ArtifactValidationError),
    ModuleIdentityMismatch { expected: String, found: String },
    ModuleIdChanged { expected: ModuleId, found: ModuleId },
    PathFingerprintMismatch,
}

impl std::fmt::Display for ReloadValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Artifact(error) => write!(f, "artifact validation failed: {error:?}"),
            Self::ModuleIdentityMismatch { expected, found } => write!(
                f,
                "reload module identity mismatch: expected `{expected}`, found `{found}`"
            ),
            Self::ModuleIdChanged { expected, found } => write!(
                f,
                "reload module id changed: expected {:?}, found {:?}",
                expected, found
            ),
            Self::PathFingerprintMismatch => {
                write!(f, "reload typed path fingerprints changed")
            }
        }
    }
}

impl From<ArtifactValidationError> for ReloadValidationError {
    fn from(error: ArtifactValidationError) -> Self {
        Self::Artifact(error)
    }
}

fn path_fingerprints_for_module(module: &BytecodeModule) -> Vec<PathDescriptorFingerprint> {
    module
        .paths
        .iter()
        .map(|path| PathDescriptorFingerprint {
            path: path.id,
            fingerprint: ArtifactFingerprint::of_debug(path),
        })
        .collect()
}
