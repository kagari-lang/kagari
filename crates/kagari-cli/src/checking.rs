//! Required checks and recoverable acquisition reporting share existing SDK analysis.

use crate::{
    CliError,
    args::ProjectArgs,
    project::locate,
    read_source,
    report::{MessageFormat, Report},
    source_engine,
    tool_config::ToolConfig,
};
use kagari::{engine::KagariEngine, error::EmbeddingError};
use kagari_common::cancellation::CancellationToken;
use kagari_package::{
    error::PackageError,
    lock::ResolutionOptions,
    project::{ProjectDiscovery, ProjectOptions},
    workspace::MemberSelection,
};
use kagari_source::presentation::DiagnosticRecord;
use std::path::Path;

pub(crate) fn source(path: &Path, format: MessageFormat) -> Result<(), CliError> {
    let mut report = Report::new("check");
    let result = (|| {
        let source = read_source(path)?;
        let engine = source_engine("target/kagari-declarations")?;
        match engine.compile_source(source) {
            Ok(_) => report.summary.succeeded += 1,
            Err(error) => {
                report.summary.failed += 1;
                semantic_error(&mut report, &path.display().to_string(), &engine, error);
            }
        }
        Ok(())
    })();
    request_error(&mut report, result);
    report.finish(format)
}

pub(crate) fn project(args: &ProjectArgs, cwd: &Path) -> Result<(), CliError> {
    project_report(args, cwd).finish(args.message_format)
}

pub(crate) fn project_report(args: &ProjectArgs, cwd: &Path) -> Report {
    let mut report = Report::new("check");
    let result = project_into(args, cwd, &mut report);
    request_error(&mut report, result);
    report
}

fn project_into(args: &ProjectArgs, cwd: &Path, report: &mut Report) -> Result<(), CliError> {
    let cancel = CancellationToken::default();
    let (source, directory) = locate(args, cwd)?;
    let options = ProjectOptions {
        selection: if args.workspace {
            MemberSelection::Workspace
        } else if args.packages.is_empty() {
            MemberSelection::Default
        } else {
            MemberSelection::Names(args.packages.clone())
        },
        resolution: ResolutionOptions {
            locked: args.locked,
            offline: args.offline,
        },
        ..Default::default()
    };
    let project = ProjectDiscovery::load(&source, &directory, &options, &cancel)
        .map_err(|error| CliError::message(1, error.to_string()))?;
    let workspace = source.root().join(&project.workspace.directory);
    ToolConfig::load(&workspace)?;
    if project.roots.is_empty() {
        return Err(CliError::message(2, "no packages selected"));
    }
    if project.is_complete() {
        project
            .lock
            .publish(
                &source,
                &project.workspace.directory,
                options.resolution,
                &cancel,
            )
            .map_err(|error| CliError::message(1, error.to_string()))?;
    }
    let engine = source_engine(workspace.join("target/kagari/declarations"))?;
    for (member, capture) in &project.roots {
        if let Some(inputs) = &capture.program {
            match engine.compile_package(inputs, &cancel) {
                Ok(_) => report.summary.succeeded += 1,
                Err(error) => {
                    report.summary.failed += 1;
                    let cancelled = matches!(error, EmbeddingError::Cancelled);
                    semantic_error(report, member, &engine, error);
                    if cancelled {
                        break;
                    }
                }
            }
        } else {
            report.summary.incomplete += 1;
            for discovery in capture.discoveries.values() {
                for (source, diagnostics) in discovery.diagnostics() {
                    for diagnostic in diagnostics {
                        report.source_diagnostic(
                            member,
                            DiagnosticRecord::from_diagnostic(diagnostic.clone(), source),
                            source,
                        );
                    }
                }
                for error in discovery.errors() {
                    acquisition_error(report, member, error);
                }
            }
            for error in &capture.errors {
                acquisition_error(report, member, error);
            }
            report.operational(
                member,
                "KG_INPUT_INCOMPLETE",
                "module discovery is incomplete; this root was not checked",
            );
        }
    }
    Ok(())
}

fn acquisition_error(report: &mut Report, context: &str, error: &PackageError) {
    report.operational(context, "KG_INPUT", error.to_string());
}

fn semantic_error(
    report: &mut Report,
    context: &str,
    engine: &KagariEngine,
    error: EmbeddingError,
) {
    if let EmbeddingError::Diagnostics { diagnostics } = error {
        let sources = engine.source_snapshot();
        for diagnostic in diagnostics {
            report.diagnostic(context, diagnostic, &sources);
        }
    } else {
        report.operational(context, &error.code(), format!("{error:?}"));
    }
}

fn request_error(report: &mut Report, result: Result<(), CliError>) {
    if let Err(error) = result {
        report.summary.incomplete += 1;
        report.summary.exit_code = error.exit_code();
        report.operational(
            "",
            if error.exit_code() == 2 {
                "KG_CONFIG"
            } else {
                "KG_INPUT"
            },
            error.to_string(),
        );
    }
}
