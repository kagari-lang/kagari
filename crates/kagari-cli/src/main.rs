use std::{
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use kagari_common::{Diagnostic, SourceFile};
use kagari_embed::{
    ArtifactOptions, BytecodeArtifact, CompileOptions, EmbeddingDiagnostic, EmbeddingError,
    ExecutionContext, JitPolicy, KagariEngine, LoadOptions,
};
use kagari_runtime::{
    CapabilitySet, HostExposurePolicy, LanguageProfile,
    host::{HostError, HostFunction, HostParameter, HostPassingStyle},
    value::Value,
};
use kagari_syntax::parse_module;

fn main() -> ExitCode {
    match Cli::parse(env::args().skip(1)).and_then(run_cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(error.exit_code())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Cli {
    command: Command,
    profile: CliProfile,
    jit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Command {
    Parse { source: PathBuf },
    Check { source: PathBuf },
    Emit { source: PathBuf, output: PathBuf },
    RunSource { source: PathBuf },
    RunArtifact { artifact: PathBuf },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CliProfile {
    Restricted,
    Dev,
    Tooling,
}

impl Cli {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, CliError> {
        let mut parser = ArgParser::new(args);
        parser.consume_options()?;
        let Some(first) = parser.next_positional() else {
            return Err(CliError::usage());
        };

        let command_name = match first.as_str() {
            "parse" | "check" | "emit" | "run" | "run-artifact" => first,
            "--help" | "-h" | "help" => return Err(CliError::usage()),
            path => {
                let source = PathBuf::from(path);
                let profile = parser.profile()?;
                let jit = parser.jit()?;
                parser.finish()?;
                return Ok(Self {
                    command: command_for_implicit_path(source),
                    profile,
                    jit,
                });
            }
        };

        let profile = parser.profile()?;
        let jit = parser.jit()?;
        let command = match command_name.as_str() {
            "parse" => Command::Parse {
                source: parser.required_path("source")?,
            },
            "check" => Command::Check {
                source: parser.required_path("source")?,
            },
            "emit" => {
                let output = parser.output();
                let source = parser.required_path("source")?;
                Command::Emit {
                    output: output.unwrap_or_else(|| default_artifact_path(&source)),
                    source,
                }
            }
            "run" => Command::RunSource {
                source: parser.required_path("source")?,
            },
            "run-artifact" => Command::RunArtifact {
                artifact: parser.required_path("artifact")?,
            },
            _ => unreachable!("command name was already filtered"),
        };
        parser.finish()?;

        Ok(Self {
            command,
            profile,
            jit,
        })
    }
}

#[derive(Debug)]
struct ArgParser {
    args: Vec<String>,
    index: usize,
    profile: CliProfile,
    jit: bool,
    output: Option<PathBuf>,
}

impl ArgParser {
    fn new(args: impl IntoIterator<Item = String>) -> Self {
        Self {
            args: args.into_iter().collect(),
            index: 0,
            profile: CliProfile::Dev,
            jit: false,
            output: None,
        }
    }

    fn next_positional(&mut self) -> Option<String> {
        let arg = self.args.get(self.index)?.clone();
        self.index += 1;
        Some(arg)
    }

    fn profile(&mut self) -> Result<CliProfile, CliError> {
        self.consume_options()?;
        Ok(self.profile)
    }

    fn jit(&mut self) -> Result<bool, CliError> {
        self.consume_options()?;
        Ok(self.jit)
    }

    fn output(&mut self) -> Option<PathBuf> {
        let _ = self.consume_options();
        self.output.clone()
    }

    fn required_path(&mut self, label: &'static str) -> Result<PathBuf, CliError> {
        self.consume_options()?;
        self.next_positional()
            .map(PathBuf::from)
            .ok_or_else(|| CliError::message(2, format!("missing {label} path\n\n{}", usage())))
    }

    fn finish(&mut self) -> Result<(), CliError> {
        self.consume_options()?;
        if self.index == self.args.len() {
            Ok(())
        } else {
            Err(CliError::message(
                2,
                format!(
                    "unexpected argument `{}`\n\n{}",
                    self.args[self.index],
                    usage()
                ),
            ))
        }
    }

    fn consume_options(&mut self) -> Result<(), CliError> {
        while self.index < self.args.len() {
            match self.args[self.index].as_str() {
                "--profile" => {
                    self.index += 1;
                    let value = self
                        .args
                        .get(self.index)
                        .ok_or_else(|| CliError::message(2, usage()))?;
                    self.profile = CliProfile::parse(value)?;
                    self.index += 1;
                }
                "--jit" => {
                    self.jit = true;
                    self.index += 1;
                }
                "--no-jit" => {
                    self.jit = false;
                    self.index += 1;
                }
                "-o" | "--output" => {
                    self.index += 1;
                    let value = self
                        .args
                        .get(self.index)
                        .ok_or_else(|| CliError::message(2, usage()))?;
                    self.output = Some(PathBuf::from(value));
                    self.index += 1;
                }
                _ => break,
            }
        }
        Ok(())
    }
}

impl CliProfile {
    fn parse(value: &str) -> Result<Self, CliError> {
        match value {
            "restricted" => Ok(Self::Restricted),
            "dev" => Ok(Self::Dev),
            "tooling" => Ok(Self::Tooling),
            _ => Err(CliError::message(
                2,
                format!("unknown profile `{value}`\n\n{}", usage()),
            )),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Restricted => "restricted",
            Self::Dev => "dev",
            Self::Tooling => "tooling",
        }
    }

    fn compile_options(self) -> CompileOptions {
        CompileOptions {
            language_profile: self.language_profile(false),
            ..CompileOptions::default()
        }
    }

    fn artifact_options(self) -> ArtifactOptions {
        let mut options = ArtifactOptions::default();
        options.build.security_profile = Some(self.name().to_owned());
        options
    }

    fn load_options(self, module_name: Option<String>) -> LoadOptions {
        let mut options = LoadOptions {
            module_name,
            ..LoadOptions::default()
        };
        options.compatibility.security_profile = Some(self.name().to_owned());
        options
    }

    fn execution_context(self, jit: bool) -> ExecutionContext {
        ExecutionContext {
            language_profile: self.language_profile(jit),
            capabilities: self.capabilities(jit),
            host_policy: self.host_policy(),
            jit_policy: if jit {
                JitPolicy::Enabled
            } else {
                JitPolicy::Disabled
            },
            ..ExecutionContext::default()
        }
    }

    fn language_profile(self, jit: bool) -> LanguageProfile {
        match self {
            Self::Restricted => LanguageProfile::default(),
            Self::Dev => LanguageProfile {
                allow_host_calls: true,
                allow_jit: jit,
                ..LanguageProfile::default()
            },
            Self::Tooling => LanguageProfile {
                allow_reflection: true,
                allow_reflection_write: true,
                allow_interface_values: true,
                allow_host_calls: true,
                allow_path_mutation: true,
                allow_module_loading: true,
                allow_jit: jit,
                allow_debugger: true,
                ..LanguageProfile::default()
            },
        }
    }

    fn capabilities(self, jit: bool) -> CapabilitySet {
        match self {
            Self::Restricted => CapabilitySet::default(),
            Self::Dev => CapabilitySet {
                host_calls: true,
                jit,
                ..CapabilitySet::default()
            },
            Self::Tooling => CapabilitySet {
                host_calls: true,
                path_mutation: true,
                reflection_metadata: true,
                reflection_read: true,
                reflection_write: true,
                dynamic_invocation: true,
                downcast: true,
                module_loading: true,
                jit,
                debug_attach: true,
                debug_breakpoints: true,
                debug_pause: true,
                debug_stack_inspection: true,
                debug_value_inspection: true,
                debug_host_value_inspection: true,
                debug_watch_evaluation: true,
                debug_side_effecting_evaluation: true,
                ..CapabilitySet::default()
            },
        }
    }

    fn host_policy(self) -> HostExposurePolicy {
        match self {
            Self::Restricted => HostExposurePolicy::default(),
            Self::Dev => HostExposurePolicy {
                allowed_host_functions: vec!["host.log".to_owned()],
                ..HostExposurePolicy::default()
            },
            Self::Tooling => HostExposurePolicy {
                allow_host_functions: true,
                allow_host_types: true,
                allow_host_path_reads: true,
                allow_host_path_mutation: true,
                ..HostExposurePolicy::default()
            },
        }
    }
}

fn run_cli(cli: Cli) -> Result<(), CliError> {
    match cli.command {
        Command::Parse { source } => parse_source(&source),
        Command::Check { source } => check_source(&source, cli.profile),
        Command::Emit { source, output } => emit_artifact(&source, &output, cli.profile),
        Command::RunSource { source } => run_source(&source, cli.profile, cli.jit),
        Command::RunArtifact { artifact } => run_artifact(&artifact, cli.profile, cli.jit),
    }
}

fn parse_source(path: &Path) -> Result<(), CliError> {
    let source = read_source(path)?;
    match parse_module(&source) {
        Ok(_) => {
            println!("parsed {}", path.display());
            Ok(())
        }
        Err(diagnostics) => {
            print_common_diagnostics(&diagnostics);
            Err(CliError::message(1, "parse failed"))
        }
    }
}

fn check_source(path: &Path, profile: CliProfile) -> Result<(), CliError> {
    let source = read_source(path)?;
    let engine = KagariEngine::default();
    match engine.compile_source(source, profile.compile_options()) {
        Ok(_) => {
            println!("checked {}", path.display());
            Ok(())
        }
        Err(error) => Err(print_embedding_error(error)),
    }
}

fn emit_artifact(path: &Path, output: &Path, profile: CliProfile) -> Result<(), CliError> {
    let source = read_source(path)?;
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            source,
            profile.compile_options(),
            profile.artifact_options(),
        )
        .map_err(print_embedding_error)?;
    let bytes = artifact
        .to_bytes()
        .map_err(|error| CliError::message(1, error.to_string()))?;
    fs::write(output, bytes).map_err(|error| {
        CliError::message(
            1,
            format!("failed to write artifact `{}`: {error}", output.display()),
        )
    })?;
    println!("emitted {}", output.display());
    Ok(())
}

fn run_source(path: &Path, profile: CliProfile, jit: bool) -> Result<(), CliError> {
    let source = read_source(path)?;
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            source,
            profile.compile_options(),
            profile.artifact_options(),
        )
        .map_err(print_embedding_error)?;
    run_loaded_artifact(
        &engine,
        artifact,
        profile.load_options(Some(path.display().to_string())),
        profile,
        jit,
    )
}

fn run_artifact(path: &Path, profile: CliProfile, jit: bool) -> Result<(), CliError> {
    let bytes = fs::read(path).map_err(|error| {
        CliError::message(
            1,
            format!("failed to read artifact `{}`: {error}", path.display()),
        )
    })?;
    let artifact = BytecodeArtifact::from_bytes(&bytes)
        .map_err(|error| CliError::message(1, error.to_string()))?;
    run_loaded_artifact(
        &KagariEngine::default(),
        artifact,
        profile.load_options(None),
        profile,
        jit,
    )
}

fn run_loaded_artifact(
    engine: &KagariEngine,
    artifact: BytecodeArtifact,
    load_options: LoadOptions,
    profile: CliProfile,
    jit: bool,
) -> Result<(), CliError> {
    let has_main = artifact
        .module
        .functions
        .iter()
        .any(|function| function.name == "main");
    let context = profile.execution_context(jit);
    let mut runtime = engine.runtime(context.clone());
    register_default_host_functions(&mut runtime)
        .map_err(|error| CliError::message(1, error.to_string()))?;
    let loaded = runtime
        .load_module(artifact, load_options)
        .map_err(print_embedding_error)?;

    if has_main {
        execute_entry(&mut runtime, &loaded, &context, jit).map(|_| ())
    } else {
        runtime
            .execute_module(&loaded, &context)
            .map(|_| ())
            .map_err(print_embedding_error)
    }
}

fn execute_entry(
    runtime: &mut kagari_embed::KagariRuntime,
    loaded: &kagari_runtime::LoadedModule,
    context: &ExecutionContext,
    jit: bool,
) -> Result<kagari_vm::ExecutionReport, CliError> {
    if !jit {
        return runtime
            .execute(loaded, "main", &[], context)
            .map_err(print_embedding_error);
    }
    execute_entry_with_jit(runtime, loaded, context)
}

#[cfg(feature = "jit")]
fn execute_entry_with_jit(
    runtime: &mut kagari_embed::KagariRuntime,
    loaded: &kagari_runtime::LoadedModule,
    context: &ExecutionContext,
) -> Result<kagari_vm::ExecutionReport, CliError> {
    let mut backend = kagari_jit_cranelift::CraneliftBackend::for_host()
        .map_err(|error| CliError::message(1, format!("failed to initialize JIT: {error}")))?;
    runtime
        .execute_with_backend(loaded, "main", &[], context, &mut backend)
        .map_err(print_embedding_error)
}

#[cfg(not(feature = "jit"))]
fn execute_entry_with_jit(
    _runtime: &mut kagari_embed::KagariRuntime,
    _loaded: &kagari_runtime::LoadedModule,
    _context: &ExecutionContext,
) -> Result<kagari_vm::ExecutionReport, CliError> {
    Err(CliError::message(
        2,
        "this kagari binary was built without the `jit` feature",
    ))
}

fn read_source(path: &Path) -> Result<SourceFile, CliError> {
    let text = fs::read_to_string(path).map_err(|error| {
        CliError::message(1, format!("failed to read `{}`: {error}", path.display()))
    })?;
    Ok(SourceFile::new(path.display().to_string(), text))
}

fn register_default_host_functions(
    runtime: &mut kagari_embed::KagariRuntime,
) -> Result<(), kagari_runtime::RuntimeError> {
    runtime.register_host_function(HostFunction::new(
        "host.log",
        vec![HostParameter {
            name: "message",
            type_name: "String",
            passing: HostPassingStyle::SharedBorrow,
        }],
        "()",
        |args| {
            let Some(Value::Str(message)) = args.first() else {
                return Err(HostError::new("host.log expects one string argument"));
            };
            println!("{message}");
            Ok(Value::Unit)
        },
    ))?;
    Ok(())
}

fn print_common_diagnostics(diagnostics: &[Diagnostic]) {
    for diagnostic in diagnostics {
        match diagnostic.span {
            Some(span) => eprintln!(
                "{}: {} at {}..{}",
                diagnostic.kind.code(),
                diagnostic.kind,
                span.start,
                span.end
            ),
            None => eprintln!("{}: {}", diagnostic.kind.code(), diagnostic.kind),
        }
    }
}

fn print_embedding_diagnostics(diagnostics: &[EmbeddingDiagnostic]) {
    for diagnostic in diagnostics {
        match diagnostic.span {
            Some(span) => eprintln!(
                "{}: {} at {}..{}",
                diagnostic.code, diagnostic.message, span.start, span.end
            ),
            None => eprintln!("{}: {}", diagnostic.code, diagnostic.message),
        }
    }
}

fn print_embedding_error(error: EmbeddingError) -> CliError {
    match error {
        EmbeddingError::Diagnostics { diagnostics } => {
            print_embedding_diagnostics(&diagnostics);
            CliError::message(1, "diagnostics emitted")
        }
        other => CliError::message(1, format!("{}: {other:?}", other.code())),
    }
}

fn command_for_implicit_path(path: PathBuf) -> Command {
    if path.extension().and_then(|extension| extension.to_str()) == Some("kbc") {
        Command::RunArtifact { artifact: path }
    } else {
        Command::RunSource { source: path }
    }
}

fn default_artifact_path(source: &Path) -> PathBuf {
    source.with_extension("kbc")
}

fn usage() -> String {
    [
        "usage:",
        "  kagari parse [--profile restricted|dev|tooling] <script.kgr>",
        "  kagari check [--profile restricted|dev|tooling] <script.kgr>",
        "  kagari emit [--profile restricted|dev|tooling] [-o artifact.kbc] <script.kgr>",
        "  kagari run [--profile restricted|dev|tooling] [--jit] <script.kgr>",
        "  kagari run-artifact [--profile restricted|dev|tooling] [--jit] <artifact.kbc>",
    ]
    .join("\n")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CliError {
    code: u8,
    message: String,
}

impl CliError {
    fn message(code: u8, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn usage() -> Self {
        Self::message(2, usage())
    }

    fn exit_code(&self) -> u8 {
        self.code
    }
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CliError {}

#[cfg(test)]
mod tests {
    use super::{Cli, CliProfile, Command, run_cli};
    use kagari_embed::BytecodeArtifact;
    use std::{
        fs,
        path::PathBuf,
        process,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn parse(args: &[&str]) -> Cli {
        Cli::parse(args.iter().map(|arg| arg.to_string())).expect("args should parse")
    }

    #[test]
    fn parses_pipeline_commands_and_profiles() {
        assert_eq!(
            parse(&["parse", "--profile", "restricted", "main.kgr"]),
            Cli {
                command: Command::Parse {
                    source: PathBuf::from("main.kgr"),
                },
                profile: CliProfile::Restricted,
                jit: false,
            }
        );
        assert_eq!(
            parse(&["check", "main.kgr"]).command,
            Command::Check {
                source: PathBuf::from("main.kgr"),
            }
        );
        assert_eq!(
            parse(&["emit", "-o", "main.kbc", "main.kgr"]).command,
            Command::Emit {
                source: PathBuf::from("main.kgr"),
                output: PathBuf::from("main.kbc"),
            }
        );
    }

    #[test]
    fn parses_leading_options_for_implicit_run() {
        assert_eq!(
            parse(&["--profile", "restricted", "main.kgr"]),
            Cli {
                command: Command::RunSource {
                    source: PathBuf::from("main.kgr"),
                },
                profile: CliProfile::Restricted,
                jit: false,
            }
        );
    }

    #[test]
    fn parses_source_artifact_and_jit_run_modes() {
        assert_eq!(
            parse(&["run", "--jit", "--profile", "tooling", "main.kgr"]),
            Cli {
                command: Command::RunSource {
                    source: PathBuf::from("main.kgr"),
                },
                profile: CliProfile::Tooling,
                jit: true,
            }
        );
        assert_eq!(
            parse(&["run-artifact", "main.kbc"]).command,
            Command::RunArtifact {
                artifact: PathBuf::from("main.kbc"),
            }
        );
        assert_eq!(
            parse(&["main.kbc"]).command,
            Command::RunArtifact {
                artifact: PathBuf::from("main.kbc"),
            }
        );
    }

    #[test]
    fn emits_and_runs_kbc_artifacts_through_cli_pipeline() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be valid")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("kagari-cli-{unique}-{}", process::id()));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        let source = dir.join("main.kgr");
        let artifact = dir.join("main.kbc");
        fs::write(&source, "fn main() -> i32 { 42 }").expect("source should be written");

        run_cli(Cli {
            command: Command::Parse {
                source: source.clone(),
            },
            profile: CliProfile::Dev,
            jit: false,
        })
        .expect("parse command should succeed");
        run_cli(Cli {
            command: Command::Check {
                source: source.clone(),
            },
            profile: CliProfile::Dev,
            jit: false,
        })
        .expect("check command should succeed");
        run_cli(Cli {
            command: Command::Emit {
                source: source.clone(),
                output: artifact.clone(),
            },
            profile: CliProfile::Dev,
            jit: false,
        })
        .expect("emit command should succeed");

        let emitted =
            BytecodeArtifact::from_bytes(&fs::read(&artifact).expect("artifact should exist"))
                .expect("artifact should decode");
        assert_eq!(
            emitted.verification.loader.security_profile.as_deref(),
            Some("dev")
        );

        run_cli(Cli {
            command: Command::RunArtifact {
                artifact: artifact.clone(),
            },
            profile: CliProfile::Dev,
            jit: false,
        })
        .expect("artifact command should run");

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }
}
