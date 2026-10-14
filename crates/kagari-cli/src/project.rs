//! Project commands reuse package acquisition and the SDK compilation pipeline.

mod output;
#[cfg(test)]
mod tests;

use crate::{
    CliError,
    args::{ProjectAction, ProjectArgs},
    print_source_error, run_loaded_artifact, source_engine,
};
use kagari_common::cancellation::CancellationToken;
use kagari_embed::{engine::source::ArtifactOptions, runtime::LoadOptions};
use kagari_package::{
    error::PackageError,
    filesystem::FilesystemSources,
    lock::ResolutionOptions,
    manifest::MANIFEST_NAME,
    project::{ProjectInputs, ProjectOptions},
    workspace::MemberSelection,
};
use std::{
    env, fs,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
};

pub(crate) fn run(action: ProjectAction, args: ProjectArgs, jit: bool) -> Result<(), CliError> {
    let cwd = env::current_dir().map_err(io_error)?;
    run_from(action, args, jit, &cwd)
}

fn run_from(
    action: ProjectAction,
    args: ProjectArgs,
    jit: bool,
    cwd: &Path,
) -> Result<(), CliError> {
    let cancel = CancellationToken::default();
    let (source, directory) = locate(&args, cwd)?;
    let options = ProjectOptions {
        selection: if args.workspace {
            MemberSelection::Workspace
        } else if args.packages.is_empty() {
            MemberSelection::Default
        } else {
            MemberSelection::Names(args.packages)
        },
        resolution: ResolutionOptions {
            locked: args.locked,
            offline: args.offline,
        },
        ..Default::default()
    };
    let project =
        ProjectInputs::load(&source, &directory, &options, &cancel).map_err(package_error)?;
    if project.programs().is_empty() {
        return Err(CliError::message(2, "no packages selected"));
    }
    if (action == ProjectAction::Run || args.output.is_some()) && project.programs().len() != 1 {
        return Err(CliError::message(
            2,
            "run and --output require exactly one selected package; use -p NAME",
        ));
    }
    project
        .lockfile()
        .publish(
            &source,
            &project.workspace().directory,
            options.resolution,
            &cancel,
        )
        .map_err(package_error)?;
    let workspace = source.root().join(&project.workspace().directory);
    let engine = source_engine(workspace.join("target/kagari/declarations"))?;
    for inputs in project.programs().values() {
        let checked = engine
            .compile_package(inputs, &cancel)
            .map_err(|error| print_source_error(&engine, error))?;
        let name = inputs.root().family.name();
        if action == ProjectAction::Check {
            println!("checked {name}");
            continue;
        }
        let artifact = engine
            .emit_bytecode(&checked, ArtifactOptions::default())
            .map_err(|error| print_source_error(&engine, error))?;
        let bytes = artifact
            .records()
            .to_bytes(artifact.definitions())
            .map_err(|error| CliError::message(1, error.to_string()))?;
        let path = if let Some(path) = &args.output {
            let path = cwd.join(path);
            fs::write(&path, &bytes).map_err(io_error)?;
            path
        } else {
            output::publish(
                &workspace,
                checked
                    .package_fingerprint()
                    .expect("checked package inputs"),
                &bytes,
            )?
        };
        println!("built {name}: {}", path.display());
        if action == ProjectAction::Run {
            run_loaded_artifact(
                &engine,
                artifact,
                LoadOptions {
                    module_name: Some(name.into()),
                },
                jit,
            )?;
        }
    }
    Ok(())
}

/// The CLI admits explicitly declared local inputs on the invoking filesystem
/// volume. --input-root narrows this policy; embedding hosts always supply their
/// own provider root. Workspace ownership and path normalization stay in package.
fn locate(args: &ProjectArgs, cwd: &Path) -> Result<(FilesystemSources, String), CliError> {
    let boundary = args
        .input_root
        .as_ref()
        .map(|root| FilesystemSources::new(&cwd.join(root)).map_err(package_error))
        .transpose()?;
    let directory = if let Some(manifest) = &args.manifest {
        let manifest = cwd.join(manifest);
        if manifest.file_name().and_then(|name| name.to_str()) != Some(MANIFEST_NAME) {
            return Err(CliError::message(
                2,
                "--manifest-path must name Kagari.toml",
            ));
        }
        manifest
            .parent()
            .expect("joined manifest parent")
            .canonicalize()
            .map_err(io_error)?
    } else {
        let cwd = cwd.canonicalize().map_err(io_error)?;
        nearest_manifest(&cwd, boundary.as_ref().map(FilesystemSources::root))?
    };
    let source = match boundary {
        Some(source) => source,
        None => FilesystemSources::new(
            directory
                .ancestors()
                .last()
                .expect("absolute filesystem root"),
        )
        .map_err(package_error)?,
    };
    let relative = directory
        .strip_prefix(source.root())
        .map_err(|_| CliError::message(2, "manifest is outside --input-root"))?
        .to_str()
        .ok_or_else(|| CliError::message(2, "manifest path is not UTF-8"))?
        .replace('\\', "/");
    Ok((source, relative))
}

fn nearest_manifest(cwd: &Path, boundary: Option<&Path>) -> Result<PathBuf, CliError> {
    for directory in cwd.ancestors().take(65) {
        if boundary.is_some_and(|root| !directory.starts_with(root)) {
            break;
        }
        match fs::metadata(directory.join(MANIFEST_NAME)) {
            Ok(metadata) if metadata.is_file() => return Ok(directory.to_owned()),
            Ok(_) => return Err(CliError::message(1, "Kagari.toml is not a regular file")),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(io_error(error)),
        }
    }
    Err(CliError::message(
        2,
        "no Kagari.toml found in this directory or its permitted ancestors",
    ))
}

fn package_error(error: PackageError) -> CliError {
    CliError::message(1, error.to_string())
}

fn io_error(error: io::Error) -> CliError {
    CliError::message(1, error.to_string())
}
