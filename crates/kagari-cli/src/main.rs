mod aot;
mod args;
mod checking;
mod formatting;
mod project;
mod report;
mod tool_config;

use crate::{
    args::{Cli, Command, usage},
    formatting::project as format_project,
};
use kagari::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::{KagariEngine, builder::KagariEngineBuilder, source::ArtifactOptions},
    error::EmbeddingError,
    program::{PreparedProgram, ProgramPreparationError},
    runtime::{KagariRuntime, LoadOptions},
};
use kagari_bytecode::artifact::ArtifactCompatibility;
#[cfg(feature = "jit")]
use kagari_codegen_cranelift::CraneliftBackend;
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_runtime::{
    error::RuntimeError,
    host::{HostCallContext, HostError},
    module::{LoadedModule, entry::ProgramEntry},
    native::builder::ModuleBuilder,
    value::Value,
};
use kagari_source::{
    diagnostic::Diagnostic, presentation::DiagnosticRecord, source::SourceFile,
    source_database::SourceSnapshot,
};
use kagari_syntax::parser::parse_module;
use kagari_types::declaration::logging;
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

fn run_cli(cli: Cli) -> Result<(), CliError> {
    match cli.command {
        Command::FormatProject { project, options } => format_project::run(&project, &options),
        Command::Format { source, options } => formatting::run(&source, &options),
        Command::BuildAot { source, output } => aot::build(&source, &output),
        Command::RunAot { bundle } => aot::run(&bundle),
        Command::Project { action, options } => project::run(action, options, cli.jit),
        Command::Parse { source } => parse_source(&source),
        Command::Check { source, format } => checking::source(&source, format),
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

fn source_engine(declaration_cache: impl Into<PathBuf>) -> Result<KagariEngine, CliError> {
    let mut builder = engine_builder()?;
    builder.declaration_cache(declaration_cache);
    builder.build().map_err(print_embedding_error)
}

fn emit_artifact(path: &Path, output: &Path) -> Result<(), CliError> {
    let source = read_source(path)?;
    let engine = source_engine("target/kagari-declarations")?;
    let artifact = engine
        .compile_to_artifact(source, ArtifactOptions::default())
        .map_err(|error| print_source_error(&engine, error))?;
    let bytes = artifact
        .records()
        .to_bytes(artifact.definitions())
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
    let engine = source_engine("target/kagari-declarations")?;
    let artifact = engine
        .compile_to_artifact(source, ArtifactOptions::default())
        .map_err(|error| print_source_error(&engine, error))?;
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
    run_loaded_artifact(&runtime_engine()?, artifact, LoadOptions::default(), jit)
}

fn run_loaded_artifact(
    engine: &KagariEngine,
    artifact: DefinitionMetadata<BytecodeArtifact>,
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
            .execute(loaded, ProgramEntry::Main, &[], context)
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
        .prepare_native(
            program,
            loaded,
            ProgramEntry::Main,
            &mut backend,
            &context.cancellation,
        )
        .map_err(|error| CliError::message(1, format!("native preparation failed: {error}")))?;
    runtime
        .execute_prepared(loaded, ProgramEntry::Main, &[], context, &prepared)
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

fn engine_builder() -> Result<KagariEngineBuilder, CliError> {
    let registration = |error: RuntimeError| CliError::message(1, error.to_string());
    let mut engine = KagariEngine::builder().map_err(registration)?;
    let mut module = ModuleBuilder::from_declaration(
        logging::declaration().map_err(|error| CliError::message(1, error.to_string()))?,
        engine.declarations(),
        Default::default(),
    )
    .map_err(registration)?;
    let log = module.declared_function("log").map_err(registration)?;
    module
        .bind(log, |cx: &HostCallContext<'_>, args: &[Value]| {
            let [Value::Str(message)] = args else {
                return Err(HostError::new("host.log expects one string argument"));
            };
            let message = cx
                .runtime()
                .gc()
                .string(*message)
                .ok_or_else(|| HostError::new("invalid host.log string"))?;
            println!("{}", &*message);
            Ok(Value::Unit)
        })
        .map_err(registration)?;
    engine
        .install(module.finish().map_err(registration)?)
        .map_err(registration)?;
    Ok(engine)
}

fn runtime_engine() -> Result<KagariEngine, CliError> {
    engine_builder()?.build().map_err(print_embedding_error)
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

fn print_embedding_diagnostics(diagnostics: &[DiagnosticRecord], sources: Option<&SourceSnapshot>) {
    for diagnostic in diagnostics {
        let origin = diagnostic
            .span
            .and_then(|span| {
                sources
                    .filter(|sources| sources.contains(span))?
                    .file(span.file)
            })
            .map(|file| format!("{}: ", file.name()))
            .unwrap_or_default();
        match diagnostic.span {
            Some(span) => eprintln!(
                "{origin}{}: {} at {}..{}",
                diagnostic.code, diagnostic.message, span.range.start, span.range.end
            ),
            None => eprintln!("{}: {}", diagnostic.code, diagnostic.message),
        }
    }
}

fn print_source_error(engine: &KagariEngine, error: EmbeddingError) -> CliError {
    if let EmbeddingError::Diagnostics { diagnostics } = error {
        print_embedding_diagnostics(&diagnostics, Some(&engine.source_snapshot()));
        CliError::message(1, "diagnostics emitted")
    } else {
        print_embedding_error(error)
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
            print_embedding_diagnostics(&diagnostics, None);
            CliError::message(1, "diagnostics emitted")
        }
        EmbeddingError::Runtime {
            kind,
            message,
            trace,
            task_origin,
        } => CliError::message(
            1,
            format!(
                "{}: {message}{}{}",
                kind.code(),
                task_origin
                    .map(|origin| format!(
                        "\n  originating task: {:?}, cancellation: {:?}",
                        origin.task, origin.cancellation
                    ))
                    .unwrap_or_default(),
                trace.as_ref().map(ToString::to_string).unwrap_or_default()
            ),
        ),
        other => CliError::message(1, format!("{}: {other:?}", other.code())),
    }
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
    use kagari::BytecodeArtifact;
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
            parse(&["build-aot", "main.kgr", "-o", "release.aot"]).command,
            Command::BuildAot {
                source: PathBuf::from("main.kgr"),
                output: PathBuf::from("release.aot")
            }
        );
        assert_eq!(
            parse(&["run-aot", "release.aot"]).command,
            Command::RunAot {
                bundle: PathBuf::from("release.aot")
            }
        );
        assert!(Cli::parse(["run-aot", "release.aot", "--jit"].map(str::to_owned)).is_err());
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
                format: Default::default(),
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
                format: Default::default(),
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
