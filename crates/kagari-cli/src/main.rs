use std::{env, fs, process::ExitCode};

use kagari_common::SourceFile;
use kagari_embed::{
    ArtifactOptions, CompileOptions, EmbeddingDiagnostic, EmbeddingError, ExecutionContext,
    KagariEngine, LoadOptions,
};
use kagari_runtime::{
    CapabilitySet, HostExposurePolicy, LanguageProfile,
    host::{HostError, HostFunction, HostParameter, HostPassingStyle},
    value::Value,
};

fn main() -> ExitCode {
    let Some(path) = script_path() else {
        eprintln!("usage: kagari <script.kgr>");
        eprintln!("       kagari run <script.kgr>");
        return ExitCode::from(2);
    };

    let source_text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("failed to read `{path}`: {error}");
            return ExitCode::from(1);
        }
    };
    let source = SourceFile::new(path.clone(), source_text);

    let engine = KagariEngine::default();
    let artifact = match engine.compile_to_artifact(
        source,
        CompileOptions::default(),
        ArtifactOptions::default(),
    ) {
        Ok(artifact) => artifact,
        Err(error) => {
            print_embedding_error(error);
            return ExitCode::from(1);
        }
    };

    let has_main = artifact
        .module
        .functions
        .iter()
        .any(|function| function.name == "main");

    let context = ExecutionContext {
        language_profile: LanguageProfile {
            allow_host_calls: true,
            ..LanguageProfile::default()
        },
        capabilities: CapabilitySet {
            host_calls: true,
            ..CapabilitySet::default()
        },
        host_policy: HostExposurePolicy {
            allowed_host_functions: vec!["host.log".to_owned()],
            ..HostExposurePolicy::default()
        },
        ..ExecutionContext::default()
    };
    let mut runtime = engine.runtime(context.clone());
    if let Err(error) = register_default_host_functions(&mut runtime) {
        eprintln!("{error:?}");
        return ExitCode::from(1);
    }
    let loaded = match runtime.load_module(
        artifact,
        LoadOptions {
            module_name: Some(path),
            ..LoadOptions::default()
        },
    ) {
        Ok(loaded) => loaded,
        Err(error) => {
            print_embedding_error(error);
            return ExitCode::from(1);
        }
    };

    let result = if has_main {
        runtime.execute(&loaded, "main", &[], &context).map(|_| ())
    } else {
        runtime.execute_module(&loaded, &context).map(|_| ())
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            print_embedding_error(error);
            ExitCode::from(1)
        }
    }
}

fn script_path() -> Option<String> {
    let mut args = env::args().skip(1);
    match args.next()?.as_str() {
        "run" => args.next(),
        path => Some(path.to_owned()),
    }
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

fn print_diagnostics(diagnostics: &[EmbeddingDiagnostic]) {
    for diagnostic in diagnostics {
        eprintln!("{}", diagnostic.message);
    }
}

fn print_embedding_error(error: EmbeddingError) {
    match error {
        EmbeddingError::Diagnostics { diagnostics } => print_diagnostics(&diagnostics),
        other => eprintln!("{other:?}"),
    }
}
