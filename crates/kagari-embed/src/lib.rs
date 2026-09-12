use kagari_common::{
    Diagnostic, Severity, SourceFile,
    identity::{FileId, FileSpan},
    source_database::{SourceDatabase, SourceLayer, SourceSnapshot},
};
use kagari_hir::{
    LanguageFeatureProfile,
    analysis::{AnalysisDatabase, AnalysisSnapshot, CancellationToken},
    program::{CheckedProgram, ProgramCheckError},
};
use kagari_ir::{
    IrLoweringError,
    bytecode::{
        ArtifactBuildOptions, ArtifactCompatibility, ArtifactValidationError, BytecodeInstruction,
        BytecodeLoweringError, BytecodeModule, CallTarget, KbcArtifact, RuntimeHelper,
        lower_program_to_bytecode,
    },
    program::{ProgramErrorKind, lower_program_to_ir},
};
use kagari_runtime::{
    CapabilitySet, CodegenBackend, HostFunctionId, HostTypeRegistration, LanguageProfile,
    LoadedModule, ReloadValidationError as RuntimeReloadValidationError, ResourcePolicy, Runtime,
    RuntimeConfig, RuntimeError, RuntimeErrorKind, SecurityContext, TypeId, host::HostFunction,
    value::Value,
};
use kagari_vm::{ExecutionReport, Vm, VmError};
use std::cell::RefCell;

pub use kagari_runtime::HostExposurePolicy;

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
    sources: RefCell<SourceDatabase>,
    analysis: RefCell<AnalysisDatabase>,
}

impl KagariEngine {
    pub fn set_host_interface(
        &self,
        interface: kagari_common::host_interface::HostInterface,
    ) -> Result<(), kagari_common::host_interface::HostInterfaceError> {
        let declarations = kagari_hir::host::HostDeclarations::new(interface)?;
        self.analysis
            .borrow_mut()
            .set_host_declarations(declarations);
        Ok(())
    }
    pub fn new(config: EngineConfig) -> Self {
        Self {
            config,
            sources: RefCell::default(),
            analysis: RefCell::default(),
        }
    }

    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    pub fn runtime(&self, context: ExecutionContext) -> KagariRuntime {
        let mut config = self.config.default_runtime.clone();
        config.security = context.security_context();
        config.host_exposure = context.host_policy.clone();
        config.resources = context.resources;
        KagariRuntime::new(Runtime::new(config), context)
    }

    pub fn compile_source(
        &self,
        source: SourceFile,
        options: CompileOptions,
    ) -> CompileResult<CheckedModule> {
        let id = self.set_source(source.name(), source.text().to_owned(), SourceLayer::Base)?;
        self.compile_snapshot(
            self.source_snapshot(),
            id,
            options,
            &CancellationToken::default(),
        )
    }

    pub fn set_source(
        &self,
        name: &str,
        text: String,
        layer: SourceLayer,
    ) -> CompileResult<FileId> {
        self.sources
            .borrow_mut()
            .set(name, text, layer)
            .map_err(|message| EmbeddingError::Source { message })
    }

    pub fn load_source(&self, path: &str) -> CompileResult<FileId> {
        self.sources
            .borrow_mut()
            .load_file(path)
            .map_err(|message| EmbeddingError::Source { message })
    }

    pub fn bind_module(
        &self,
        name: &str,
        module: kagari_common::identity::ModuleIdentity,
    ) -> CompileResult<FileId> {
        self.sources
            .borrow_mut()
            .bind_module(name, module)
            .map_err(|message| EmbeddingError::Source { message })
    }

    pub fn close_overlay(&self, name: &str) -> CompileResult<()> {
        self.sources
            .borrow_mut()
            .close_overlay(name)
            .map_err(|message| EmbeddingError::Source { message })
    }

    pub fn source_snapshot(&self) -> SourceSnapshot {
        self.sources.borrow().snapshot()
    }

    pub fn analyze(
        &self,
        source: SourceSnapshot,
        profile: LanguageProfile,
        cancel: &CancellationToken,
    ) -> CompileResult<AnalysisSnapshot> {
        self.analysis
            .borrow_mut()
            .snapshot(
                source,
                language_feature_profile_from_runtime(profile),
                cancel,
            )
            .map_err(|_| EmbeddingError::Cancelled)
    }

    pub fn compile_snapshot(
        &self,
        source: SourceSnapshot,
        file: FileId,
        options: CompileOptions,
        cancel: &CancellationToken,
    ) -> CompileResult<CheckedModule> {
        let snapshot = self.analyze(source, options.language_profile, cancel)?;
        let analysis = snapshot.file(file).ok_or_else(|| EmbeddingError::Source {
            message: "file is absent from this source snapshot".into(),
        })?;
        let source = analysis.source();
        use kagari_hir::imports::ModuleOrderError;
        let program = snapshot
            .check_program(file, cancel)
            .map_err(|error| match error {
                ProgramCheckError::Cancelled => EmbeddingError::Cancelled,
                ProgramCheckError::MissingFile(file) => EmbeddingError::Source {
                    message: format!("missing source file {file:?}"),
                },
                ProgramCheckError::Diagnostics(records) => EmbeddingError::Diagnostics {
                    diagnostics: records
                        .into_iter()
                        .map(|record| {
                            EmbeddingDiagnostic::from_diagnostic(
                                record.diagnostic,
                                snapshot.file(record.file).expect("checked source").source(),
                            )
                        })
                        .collect(),
                },
                ProgramCheckError::Graph(error) => {
                    let failed = match error {
                        ModuleOrderError::Cancelled => return EmbeddingError::Cancelled,
                        ModuleOrderError::Cycle(modules) => modules,
                        ModuleOrderError::InvalidImports(module) => vec![module],
                        ModuleOrderError::Missing(module) => {
                            return EmbeddingError::Source {
                                message: format!("missing source module {module}"),
                            };
                        }
                    };
                    let mut diagnostics = Vec::new();
                    for module in failed {
                        let node = snapshot
                            .module_graph()
                            .node(&module)
                            .expect("failed graph node");
                        let file = snapshot.file(node.file).expect("graph source");
                        diagnostics.extend(node.imports.diagnostics.iter().cloned().map(
                            |diagnostic| {
                                EmbeddingDiagnostic::from_diagnostic(diagnostic, file.source())
                            },
                        ));
                    }
                    EmbeddingError::Diagnostics { diagnostics }
                }
            })?;
        Ok(CheckedModule {
            source_name: source.name().to_owned(),
            program,
        })
    }

    pub fn emit_bytecode(
        &self,
        checked: &CheckedModule,
        options: ArtifactOptions,
    ) -> CompileResult<BytecodeArtifact> {
        let ir = lower_program_to_ir(&checked.program, &options.lowering).map_err(|error| {
            let source = &checked
                .program
                .modules()
                .iter()
                .find(|module| module.lowered.source.module_identity() == error.module.as_ref())
                .expect("lowered program source")
                .lowered
                .source;
            match error.kind {
                ProgramErrorKind::Cancelled => EmbeddingError::Cancelled,
                ProgramErrorKind::Lowering(error) => EmbeddingError::ir_lowering(error, source),
                error => EmbeddingError::Compilation {
                    phase: CompilationPhase::IrLowering,
                    message: format!("{error:?}"),
                },
            }
        })?;
        let program = lower_program_to_bytecode(&ir).map_err(EmbeddingError::bytecode_lowering)?;
        Ok(KbcArtifact::from_program(program, options.build))
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

    pub fn load_program(
        &mut self,
        artifact: BytecodeArtifact,
        options: LoadOptions,
    ) -> LoadResult<LoadedModule> {
        artifact
            .validate_for_loader(&options.compatibility)
            .map_err(EmbeddingError::artifact_validation)?;
        let module_name = options.module_name.unwrap_or_else(|| {
            artifact.program.modules[artifact.program.root.index()]
                .source_name
                .clone()
        });
        self.vm
            .runtime_mut()
            .load_program(module_name, artifact.program)
            .map_err(EmbeddingError::load)
    }

    pub fn reload_program(
        &mut self,
        previous: &LoadedModule,
        artifact: BytecodeArtifact,
        options: ReloadOptions,
    ) -> ReloadResult<LoadedModule> {
        let module_name = options.module_name.unwrap_or_else(|| previous.name.clone());
        self.vm
            .runtime_mut()
            .reload_artifact(previous, module_name, artifact, &options.compatibility)
            .map_err(EmbeddingError::reload_validation)
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
        for member in module.members() {
            context.validate_for_execute(entry, &member.bytecode)?;
        }
        self.vm
            .runtime_mut()
            .set_security_context(context.security_context());
        self.vm
            .runtime_mut()
            .set_host_exposure_policy(context.host_policy.clone());
        self.vm.execute(module, entry).map_err(EmbeddingError::vm)
    }

    pub fn execute_with_backend<B: CodegenBackend>(
        &mut self,
        module: &LoadedModule,
        entry: &str,
        args: &[Value],
        context: &ExecutionContext,
        backend: &mut B,
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
        for member in module.members() {
            context.validate_for_backend_execute(entry, &member.bytecode)?;
        }
        self.vm
            .runtime_mut()
            .set_security_context(context.security_context());
        self.vm
            .runtime_mut()
            .set_host_exposure_policy(context.host_policy.clone());
        self.vm
            .execute_with_backend(module, entry, backend)
            .map_err(EmbeddingError::vm)
    }

    pub fn execute_module(
        &mut self,
        module: &LoadedModule,
        context: &ExecutionContext,
    ) -> RunResult<Value> {
        for member in module.members() {
            context.validate_for_execute("__module_init__", &member.bytecode)?;
        }
        self.vm
            .runtime_mut()
            .set_security_context(context.security_context());
        self.vm
            .runtime_mut()
            .set_host_exposure_policy(context.host_policy.clone());
        self.vm.execute_module(module).map_err(EmbeddingError::vm)
    }
}

#[derive(Debug)]
pub struct CheckedModule {
    pub source_name: String,
    program: CheckedProgram,
}

impl CheckedModule {
    pub fn module_identity(&self) -> &kagari_common::identity::ModuleIdentity {
        self.program.root().lowered.source.module_identity()
    }
    pub fn program(&self) -> &CheckedProgram {
        &self.program
    }
}

#[derive(Debug, Clone, Default)]
pub struct CompileOptions {
    pub language_profile: LanguageProfile,
}

fn language_feature_profile_from_runtime(profile: LanguageProfile) -> LanguageFeatureProfile {
    LanguageFeatureProfile {
        allow_reflection: profile.allow_reflection,
        allow_reflection_write: profile.allow_reflection_write,
        allow_interface_values: profile.allow_interface_values,
        allow_host_calls: profile.allow_host_calls,
        allow_path_mutation: profile.allow_path_mutation,
        allow_module_loading: profile.allow_module_loading,
        allow_jit: profile.allow_jit,
        allow_eval: profile.allow_eval,
        allow_async: profile.allow_async,
    }
}

#[derive(Debug, Clone, Default)]
pub struct ArtifactOptions {
    pub build: ArtifactBuildOptions,
    pub lowering: kagari_ir::IrLoweringOptions,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JitPolicy {
    #[default]
    Disabled,
    Enabled,
    CompileOnLoad,
    CompileOnFirstCall,
    CompileAfterThreshold(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PanicPolicy {
    Propagate,
    #[default]
    ConvertToError,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
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
    pub fn security_context(&self) -> SecurityContext {
        SecurityContext {
            profile: self.language_profile,
            capabilities: self.capabilities,
        }
    }

    fn validate_for_execute(&self, entry: &str, module: &BytecodeModule) -> RunResult<()> {
        if self.jit_policy != JitPolicy::Disabled {
            if !self.security_context().allows_jit() {
                return Err(EmbeddingError::runtime(
                    RuntimeFailureKind::CapabilityDenied,
                    "JIT execution is denied by execution context",
                ));
            }
            return Err(EmbeddingError::runtime(
                RuntimeFailureKind::UnsupportedExecution,
                format!("JIT policy for `{entry}` is not implemented by the baseline runtime"),
            ));
        }
        self.validate_bytecode_policy(module)
    }

    fn validate_for_backend_execute(&self, _entry: &str, module: &BytecodeModule) -> RunResult<()> {
        if !self.security_context().allows_jit() {
            return Err(EmbeddingError::runtime(
                RuntimeFailureKind::CapabilityDenied,
                "JIT execution is denied by execution context",
            ));
        }
        self.validate_bytecode_policy(module)
    }

    fn validate_bytecode_policy(&self, module: &BytecodeModule) -> RunResult<()> {
        for declaration in &module.host_interface.functions {
            if !self.host_policy.exposes_host_function(&declaration.symbol)
                || !self.security_context().allows_host_calls()
            {
                return Err(EmbeddingError::runtime(
                    RuntimeFailureKind::CapabilityDenied,
                    format!(
                        "host function '{}' is denied by execution context",
                        declaration.symbol
                    ),
                ));
            }
        }
        let security = self.security_context();
        for function in &module.functions {
            for instruction in &function.instructions {
                match instruction {
                    BytecodeInstruction::Call {
                        callee: CallTarget::RuntimeHelper(helper),
                        ..
                    } => match helper {
                        RuntimeHelper::ReflectTypeOf if !security.allows_reflection_metadata() => {
                            return Err(EmbeddingError::runtime(
                                RuntimeFailureKind::CapabilityDenied,
                                "reflection metadata is denied by execution context",
                            ));
                        }
                        RuntimeHelper::ReflectGetField(_) if !security.allows_reflection_read() => {
                            return Err(EmbeddingError::runtime(
                                RuntimeFailureKind::CapabilityDenied,
                                "reflective read is denied by execution context",
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
                        RuntimeHelper::DynamicCall if !security.allows_dynamic_invocation() => {
                            return Err(EmbeddingError::runtime(
                                RuntimeFailureKind::CapabilityDenied,
                                "dynamic invocation is denied by execution context",
                            ));
                        }
                        _ => {}
                    },
                    BytecodeInstruction::SetPath { .. }
                    | BytecodeInstruction::ModifyPath { .. }
                        if !self.host_policy.exposes_host_path_mutation() =>
                    {
                        return Err(EmbeddingError::runtime(
                            RuntimeFailureKind::CapabilityDenied,
                            "host path mutation is denied by execution context",
                        ));
                    }
                    BytecodeInstruction::ReadPath { .. }
                    | BytecodeInstruction::MakePathView { .. }
                        if !self.host_policy.exposes_host_path_read() =>
                    {
                        return Err(EmbeddingError::runtime(
                            RuntimeFailureKind::CapabilityDenied,
                            "host path read is denied by execution context",
                        ));
                    }
                    BytecodeInstruction::SetPath { .. }
                    | BytecodeInstruction::ModifyPath { .. }
                        if !security.allows_path_mutation() =>
                    {
                        return Err(EmbeddingError::runtime(
                            RuntimeFailureKind::CapabilityDenied,
                            "host path mutation is denied by runtime capabilities",
                        ));
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingDiagnostic {
    pub severity: Severity,
    pub code: String,
    pub span: Option<FileSpan>,
    pub message: String,
    pub notes: Vec<String>,
    pub labels: Vec<DiagnosticLabel>,
}

impl EmbeddingDiagnostic {
    fn from_diagnostic(diagnostic: Diagnostic, source: &SourceFile) -> Self {
        let span = diagnostic.span.and_then(|span| source.span(span));
        Self {
            severity: diagnostic.severity,
            code: diagnostic.kind.code().to_owned(),
            span,
            message: diagnostic.kind.to_string(),
            notes: Vec::new(),
            labels: span
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
    pub span: FileSpan,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompilationPhase {
    Parse,
    Analyze,
    IrLowering,
    BytecodeLowering,
}

impl CompilationPhase {
    pub fn code(self) -> &'static str {
        match self {
            Self::Parse => "KG_COMPILE_PARSE",
            Self::Analyze => "KG_COMPILE_ANALYZE",
            Self::IrLowering => "KG_COMPILE_IR_LOWERING",
            Self::BytecodeLowering => "KG_COMPILE_BYTECODE_LOWERING",
        }
    }
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

impl RuntimeFailureKind {
    pub fn code(self) -> &'static str {
        match self {
            Self::ScriptTrap => "KG_RUNTIME_SCRIPT_TRAP",
            Self::BytecodeVerification => "KG_BYTECODE_VERIFICATION_FAILED",
            Self::CapabilityDenied => "KG_RUNTIME_CAPABILITY_DENIED",
            Self::ResourceLimitExceeded => "KG_RUNTIME_RESOURCE_LIMIT_EXCEEDED",
            Self::HostCallFailure => "KG_RUNTIME_HOST_CALL_FAILURE",
            Self::TypedPathValidation => "KG_RUNTIME_TYPED_PATH_VALIDATION",
            Self::StaleModuleOrHostRoot => "KG_RUNTIME_STALE_HANDLE",
            Self::ReloadValidation => "KG_RELOAD_VALIDATION_FAILED",
            Self::EngineInvariant => "KG_ENGINE_INVARIANT",
            Self::UnsupportedExecution => "KG_RUNTIME_UNSUPPORTED_EXECUTION",
        }
    }
}

#[derive(Debug)]
pub enum EmbeddingError {
    Source {
        message: String,
    },
    Cancelled,
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
        code: String,
        message: String,
    },
}

impl EmbeddingError {
    pub fn code(&self) -> String {
        match self {
            Self::Source { .. } => "KG_SOURCE_INPUT".to_owned(),
            Self::Cancelled => "KG_ANALYSIS_CANCELLED".to_owned(),
            Self::Diagnostics { diagnostics } => diagnostics
                .first()
                .map(|diagnostic| diagnostic.code.clone())
                .unwrap_or_else(|| "KG_DIAGNOSTIC_EMPTY".to_owned()),
            Self::Compilation { phase, .. } => phase.code().to_owned(),
            Self::ArtifactValidation { error } => error.code().to_owned(),
            Self::Load { error } => error.code().to_owned(),
            Self::Runtime { kind, .. } => kind.code().to_owned(),
            Self::ReloadValidation { code, .. } => code.clone(),
        }
    }

    fn diagnostics(
        diagnostics: Box<smallvec::SmallVec<[Diagnostic; 4]>>,
        source: &SourceFile,
    ) -> Self {
        Self::Diagnostics {
            diagnostics: diagnostics
                .into_vec()
                .into_iter()
                .map(|diagnostic| EmbeddingDiagnostic::from_diagnostic(diagnostic, source))
                .collect(),
        }
    }

    fn ir_lowering(error: IrLoweringError, source: &SourceFile) -> Self {
        if let IrLoweringError::Cancelled = error {
            return Self::Cancelled;
        }
        if let IrLoweringError::Diagnostic(diagnostic) = error {
            return Self::diagnostics(Box::new(smallvec::smallvec![*diagnostic]), source);
        }
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
        let error = error.into();
        Self::ReloadValidation {
            code: error.code().to_owned(),
            message: error.to_string(),
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
                RuntimeErrorKind::HostCallFailure => RuntimeFailureKind::HostCallFailure,
                RuntimeErrorKind::ModuleValidation => RuntimeFailureKind::BytecodeVerification,
                RuntimeErrorKind::InvalidReflectiveRead
                | RuntimeErrorKind::ScriptTrap
                | RuntimeErrorKind::InvalidReflectiveWrite
                | RuntimeErrorKind::MetadataConflict => RuntimeFailureKind::ScriptTrap,
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
            VmError::JitBackend(_) => RuntimeFailureKind::UnsupportedExecution,
            VmError::JitInvocation(_) => RuntimeFailureKind::EngineInvariant,
            VmError::MissingFunction(_)
            | VmError::MissingField(_)
            | VmError::ImmutableModuleSlot(_)
            | VmError::ModuleInitializing(_)
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
    Runtime(RuntimeReloadValidationError),
}

impl std::fmt::Display for ReloadValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Artifact(error) => write!(f, "artifact validation failed: {error}"),
            Self::Runtime(error) => write!(f, "{error}"),
        }
    }
}

impl ReloadValidationError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Artifact(error) => error.code(),
            Self::Runtime(error) => error.code(),
        }
    }
}

impl From<ArtifactValidationError> for ReloadValidationError {
    fn from(error: ArtifactValidationError) -> Self {
        Self::Artifact(error)
    }
}

impl From<RuntimeReloadValidationError> for ReloadValidationError {
    fn from(error: RuntimeReloadValidationError) -> Self {
        Self::Runtime(error)
    }
}
