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
use kagari::{
    engine::{KagariEngine, source::analysis::SourceAnalysis},
    error::EmbeddingError,
};
use kagari_common::cancellation::CancellationToken;
use kagari_lint::{
    lint,
    policy::Policy,
    registry::{LintId, RULES},
};
use kagari_package::{
    error::PackageError,
    lock::ResolutionOptions,
    project::{ProjectDiscovery, ProjectOptions},
    workspace::MemberSelection,
};
use kagari_source::{diagnostic::Severity, presentation::DiagnosticRecord};
use std::path::Path;

pub(crate) fn source(path: &Path, format: MessageFormat) -> Result<(), CliError> {
    source_report(path, None).finish(format)
}

pub(crate) fn lint_source(
    path: &Path,
    format: MessageFormat,
    policy: &Policy,
) -> Result<(), CliError> {
    source_report(path, Some(policy)).finish(format)
}

fn source_report(path: &Path, policy: Option<&Policy>) -> Report {
    let mut report = Report::new(if policy.is_some() { "lint" } else { "check" });
    let result = (|| {
        let source = read_source(path)?;
        let engine = source_engine("target/kagari-declarations")?;
        let cancel = CancellationToken::default();
        analyze(
            &mut report,
            &path.display().to_string(),
            &engine,
            engine.analyze_source(source, &cancel),
            policy,
            &cancel,
        );
        Ok(())
    })();
    request_error(&mut report, result);
    report
}

pub(crate) fn lint_project(
    args: &ProjectArgs,
    cwd: &Path,
    policy: &Policy,
) -> Result<(), CliError> {
    let mut report = Report::new("lint");
    let result = project_into(args, cwd, &mut report, Some(policy));
    request_error(&mut report, result);
    report.finish(args.message_format)
}

pub(crate) fn rule_help(rule: Option<LintId>) {
    for entry in RULES
        .iter()
        .filter(|entry| rule.is_none_or(|id| entry.id == id))
    {
        println!(
            "{} (default: {:?}, stage: {:?})",
            entry.name, entry.default_level, entry.stage
        );
        if rule.is_some() {
            println!(
                "{}\nScope: {}\n{}\nLimitations: {}",
                entry.rationale, entry.scope, entry.example, entry.limitation
            );
        }
    }
}

/// Mandatory checking and optional rules consume the very same analysis handle.
fn analyze(
    report: &mut Report,
    context: &str,
    engine: &KagariEngine,
    analysis: Result<SourceAnalysis, EmbeddingError>,
    policy: Option<&Policy>,
    cancel: &CancellationToken,
) -> bool {
    let mut failed = false;
    let mut cancelled = false;
    match analysis {
        Ok(analysis) => {
            if let Err(error) = analysis.check(cancel) {
                failed = true;
                cancelled = matches!(error, EmbeddingError::Cancelled);
                semantic_error(report, context, engine, error);
            }
            if let Some(policy) = policy
                && !cancelled
            {
                match lint(analysis.snapshot(), analysis.root(), policy, cancel) {
                    Ok(diagnostics) => {
                        let sources = engine.source_snapshot();
                        for diagnostic in diagnostics {
                            failed |= diagnostic.severity == Severity::Error;
                            report.diagnostic(context, diagnostic, &sources);
                        }
                    }
                    Err(_) => {
                        failed = true;
                        cancelled = true;
                        semantic_error(report, context, engine, EmbeddingError::Cancelled);
                    }
                }
            }
        }
        Err(error) => {
            failed = true;
            cancelled = matches!(error, EmbeddingError::Cancelled);
            semantic_error(report, context, engine, error);
        }
    }
    if failed {
        report.summary.failed += 1;
    } else {
        report.summary.succeeded += 1;
    }
    cancelled
}

pub(crate) fn project(args: &ProjectArgs, cwd: &Path) -> Result<(), CliError> {
    project_report(args, cwd).finish(args.message_format)
}

pub(crate) fn project_report(args: &ProjectArgs, cwd: &Path) -> Report {
    let mut report = Report::new("check");
    let result = project_into(args, cwd, &mut report, None);
    request_error(&mut report, result);
    report
}

fn project_into(
    args: &ProjectArgs,
    cwd: &Path,
    report: &mut Report,
    policy: Option<&Policy>,
) -> Result<(), CliError> {
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
    let config = ToolConfig::load(&workspace)?;
    let policy = policy.map(|policy| {
        let mut policy = policy.clone();
        config.lint.apply(&mut policy);
        policy
    });
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
            if analyze(
                report,
                member,
                &engine,
                engine.analyze_package(inputs, &cancel),
                policy.as_ref(),
                &cancel,
            ) {
                break;
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
