use kagari_bytecode::artifact::ArtifactCompatibility;
#[cfg(feature = "jit")]
use kagari_codegen_cranelift::CraneliftBackend;
use kagari_embed::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::{KagariEngine, source::ArtifactOptions},
    error::{EmbeddingDiagnostic, EmbeddingError},
    program::{PreparedProgram, ProgramPreparationError},
    runtime::{KagariRuntime, LoadOptions},
};
use kagari_runtime::{
    error::RuntimeError,
    host::{HostError, HostFunction},
    module::LoadedModule,
    value::Value,
};
use kagari_source::{diagnostic::Diagnostic, source::SourceFile};
use kagari_syntax::parser::parse_module;
use kagari_types::host_interface;
use kagari_vm::vm::ExecutionReport;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

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

                let jit = parser.jit()?;
                parser.finish()?;
                return Ok(Self {
                    command: command_for_implicit_path(source),

                    jit,
                });
            }
        };

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

        Ok(Self { command, jit })
    }
}

#[derive(Debug)]
struct ArgParser {
    args: Vec<String>,
    index: usize,

    jit: bool,
    output: Option<PathBuf>,
}

impl ArgParser {
    fn new(args: impl IntoIterator<Item = String>) -> Self {
        Self {
            args: args.into_iter().collect(),
            index: 0,

            jit: false,
            output: None,
        }
    }

    fn next_positional(&mut self) -> Option<String> {
        let arg = self.args.get(self.index)?.clone();
        self.index += 1;
        Some(arg)
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

fn run_cli(cli: Cli) -> Result<(), CliError> {
    match cli.command {
        Command::Parse { source } => parse_source(&source),
        Command::Check { source } => check_source(&source),
        Command::Emit { source, output } => emit_artifact(&source, &output),
        Command::RunSource { source } => run_source(&source, cli.jit),
        Command::RunArtifact { artifact } => run_artifact(&artifact, cli.jit),
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

fn check_source(path: &Path) -> Result<(), CliError> {
    let source = read_source(path)?;
    let engine = KagariEngine::default();
    match engine.compile_source(source) {
        Ok(_) => {
            println!("checked {}", path.display());
            Ok(())
        }
        Err(error) => Err(print_embedding_error(error)),
    }
}

fn emit_artifact(path: &Path, output: &Path) -> Result<(), CliError> {
    let source = read_source(path)?;
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(source, ArtifactOptions::default())
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

fn run_source(path: &Path, jit: bool) -> Result<(), CliError> {
    let source = read_source(path)?;
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(source, ArtifactOptions::default())
        .map_err(print_embedding_error)?;
    run_loaded_artifact(
        &engine,
        artifact,
        LoadOptions {
            module_name: Some(path.display().to_string()),
        },
        jit,
    )
}

fn run_artifact(path: &Path, jit: bool) -> Result<(), CliError> {
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
        LoadOptions::default(),
        jit,
    )
}

fn run_loaded_artifact(
    engine: &KagariEngine,
    artifact: BytecodeArtifact,
    load_options: LoadOptions,

    jit: bool,
) -> Result<(), CliError> {
    let context = ExecutionContext {
        jit_policy: if jit {
            JitPolicy::Enabled
        } else {
            JitPolicy::Disabled
        },
        ..Default::default()
    };
    let program = PreparedProgram::from_artifact(
        artifact,
        &ArtifactCompatibility::default(),
        &context.cancellation,
    )
    .map_err(print_program_error)?;
    let mut runtime = engine.runtime(context.clone());
    register_default_host_functions(&mut runtime)
        .map_err(|error| CliError::message(1, error.to_string()))?;
    let loaded = runtime
        .load_program(&program, load_options)
        .map_err(print_embedding_error)?;

    let report = execute_entry(&mut runtime, &program, &loaded, &context, jit)?;
    if let Some(failure) = report.failure {
        return Err(CliError::message(1, format!("Result::Err: {failure}")));
    }
    Ok(())
}

fn execute_entry(
    runtime: &mut KagariRuntime,
    program: &PreparedProgram,
    loaded: &LoadedModule,
    context: &ExecutionContext,
    jit: bool,
) -> Result<ExecutionReport, CliError> {
    if !jit {
        return runtime
            .execute(loaded, "main", &[], context)
            .map_err(print_embedding_error);
    }
    execute_entry_with_jit(runtime, program, loaded, context)
}

#[cfg(feature = "jit")]
fn execute_entry_with_jit(
    runtime: &mut KagariRuntime,
    program: &PreparedProgram,
    loaded: &LoadedModule,
    context: &ExecutionContext,
) -> Result<ExecutionReport, CliError> {
    let mut backend = CraneliftBackend::for_host()
        .map_err(|error| CliError::message(1, format!("failed to initialize JIT: {error}")))?;
    let prepared = runtime
        .prepare_native(program, loaded, "main", &mut backend, &context.cancellation)
        .map_err(|error| CliError::message(1, format!("native preparation failed: {error}")))?;
    runtime
        .execute_prepared(loaded, "main", &[], context, &prepared)
        .map_err(print_embedding_error)
}

#[cfg(not(feature = "jit"))]
fn execute_entry_with_jit(
    _runtime: &mut KagariRuntime,
    _program: &PreparedProgram,
    _loaded: &LoadedModule,
    _context: &ExecutionContext,
) -> Result<ExecutionReport, CliError> {
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

fn register_default_host_functions(runtime: &mut KagariRuntime) -> Result<(), RuntimeError> {
    runtime.register_host_function(HostFunction::new(
        host_interface::standard_log(),
        |_, args| {
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
                diagnostic.code, diagnostic.message, span.range.start, span.range.end
            ),
            None => eprintln!("{}: {}", diagnostic.code, diagnostic.message),
        }
    }
}

fn print_program_error(error: ProgramPreparationError) -> CliError {
    match error {
        ProgramPreparationError::Artifact(error) => {
            print_embedding_error(EmbeddingError::ArtifactValidation { error })
        }
        ProgramPreparationError::Cancelled => print_embedding_error(EmbeddingError::Cancelled),
        other => CliError::message(1, format!("program preparation failed: {other}")),
    }
}

fn print_embedding_error(error: EmbeddingError) -> CliError {
    match error {
        EmbeddingError::Diagnostics { diagnostics } => {
            print_embedding_diagnostics(&diagnostics);
            CliError::message(1, "diagnostics emitted")
        }
        EmbeddingError::Runtime {
            kind,
            message,
            trace,
        } => CliError::message(
            1,
            format!(
                "{}: {message}{}",
                kind.code(),
                trace.as_ref().map(ToString::to_string).unwrap_or_default()
            ),
        ),
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
        "  kagari parse <script.kgr>",
        "  kagari check <script.kgr>",
        "  kagari emit [-o artifact.kbc] <script.kgr>",
        "  kagari run [--jit] <script.kgr>",
        "  kagari run-artifact [--jit] <artifact.kbc>",
    ]
    .join("\n")
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
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

#[cfg(test)]
mod tests {
    use super::{Cli, Command, run_cli};
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
    fn parses_pipeline_commands() {
        assert_eq!(
            parse(&["parse", "main.kgr"]),
            Cli {
                command: Command::Parse {
                    source: PathBuf::from("main.kgr"),
                },

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
            parse(&["main.kgr"]),
            Cli {
                command: Command::RunSource {
                    source: PathBuf::from("main.kgr"),
                },

                jit: false,
            }
        );
    }

    #[test]
    fn parses_source_artifact_and_jit_run_modes() {
        assert_eq!(
            parse(&["run", "--jit", "main.kgr"]),
            Cli {
                command: Command::RunSource {
                    source: PathBuf::from("main.kgr"),
                },

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

            jit: false,
        })
        .expect("parse command should succeed");
        run_cli(Cli {
            command: Command::Check {
                source: source.clone(),
            },

            jit: false,
        })
        .expect("check command should succeed");
        run_cli(Cli {
            command: Command::Emit {
                source: source.clone(),
                output: artifact.clone(),
            },

            jit: false,
        })
        .expect("emit command should succeed");

        let _emitted =
            BytecodeArtifact::from_bytes(&fs::read(&artifact).expect("artifact should exist"))
                .expect("artifact should decode");

        run_cli(Cli {
            command: Command::RunArtifact {
                artifact: artifact.clone(),
            },

            jit: false,
        })
        .expect("artifact command should run");

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn returned_errors_report_the_original_site_from_source_and_artifact() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("kagari-cli-origin-{unique}-{}", process::id()));
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("main.kgr");
        let artifact = dir.join("main.kbc");
        fs::write(&source, "fn fail()->Result<i32,String>{\n    Err(\"original\")\n}\nfn main()->Result<i32,String>{Ok(fail()?)}").unwrap();
        run_cli(Cli {
            command: Command::Emit {
                source: source.clone(),
                output: artifact.clone(),
            },

            jit: false,
        })
        .unwrap();
        let direct = run_cli(Cli {
            command: Command::RunSource {
                source: source.clone(),
            },

            jit: false,
        })
        .unwrap_err();
        assert_eq!(direct.exit_code(), 1);
        assert!(direct.to_string().contains("Result::Err: original"));
        assert!(direct.to_string().contains("main.kgr:2:5"));
        // Portable reporting must not read edited source files during artifact execution.
        fs::write(&source, "this is no longer the compiled code").unwrap();
        let encoded = run_cli(Cli {
            command: Command::RunArtifact { artifact },

            jit: cfg!(feature = "jit"),
        })
        .unwrap_err();
        assert_eq!(direct.to_string(), encoded.to_string());
        fs::remove_file(&source).unwrap();
        fs::remove_file(dir.join("main.kbc")).unwrap();
        fs::remove_dir(dir).unwrap();
    }
}
