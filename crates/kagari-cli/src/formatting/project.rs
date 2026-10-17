//! Project formatting selects member sources without resolving dependency contents.

use crate::{
    CliError,
    args::{FormatArgs, ProjectArgs},
    formatting::{format_captured, options, writeback::CapturedFile},
    project::locate,
    report::{MessageFormat, Report},
    tool_config::ToolConfig,
};
use kagari_common::cancellation::CancellationToken;
use kagari_package::{
    error::PackageError,
    modules::{ModuleDiscovery, ModuleLimits},
    workspace::{MemberDiscovery, MemberSelection, WorkspaceInputs, WorkspaceLimits},
};
use kagari_source::{presentation::DiagnosticRecord, source::SourceFile};
use std::{collections::BTreeSet, env, io::Error, path::Path};

pub(crate) fn run(args: &ProjectArgs, format: &FormatArgs) -> Result<(), CliError> {
    let cwd = env::current_dir().map_err(|error| CliError::message(1, error.to_string()))?;
    run_from(args, format, &cwd)
}

fn run_from(args: &ProjectArgs, format: &FormatArgs, cwd: &Path) -> Result<(), CliError> {
    let mut report = Report::new("fmt");
    if let Err(error) = project_into(args, format, cwd, &mut report) {
        report.summary.incomplete += 1;
        report.summary.exit_code = error.exit_code();
        report.operational("", "KG_FORMAT_INPUT", error.to_string());
    }
    report.finish(MessageFormat::Human)
}

fn project_into(
    args: &ProjectArgs,
    format: &FormatArgs,
    cwd: &Path,
    report: &mut Report,
) -> Result<(), CliError> {
    let cancel = CancellationToken::default();
    let (input, directory) = locate(args, cwd)?;
    let workspace = WorkspaceInputs::load(
        &input,
        &directory,
        None,
        WorkspaceLimits::default(),
        MemberDiscovery::Declared,
        &cancel,
    )
    .map_err(package_error)?;
    let config = ToolConfig::load(&input.root().join(&workspace.directory))?;
    let selection = if args.workspace {
        MemberSelection::Workspace
    } else if args.packages.is_empty() {
        MemberSelection::Default
    } else {
        MemberSelection::Names(args.packages.clone())
    };
    let selected = workspace.select(&selection).map_err(package_error)?;
    if selected.is_empty() {
        return Err(CliError::message(2, "no packages selected"));
    }
    let mut seen = BTreeSet::new();
    for (member, package) in selected {
        let discovery = ModuleDiscovery::load(
            &input,
            &package.manifest.root,
            ModuleLimits::default(),
            &cancel,
        )
        .map_err(package_error)?;
        if !discovery.is_complete() {
            report.summary.incomplete += 1;
            report.operational(
                member,
                "KG_INPUT_INCOMPLETE",
                "module discovery is incomplete; undiscovered children were not formatted",
            );
        }
        for error in discovery.errors() {
            report.operational(member, "KG_INPUT", error.to_string());
        }
        for (source, diagnostics) in discovery.diagnostics() {
            report.summary.failed += 1;
            for diagnostic in diagnostics {
                report.source_diagnostic(
                    member,
                    DiagnosticRecord::from_diagnostic(diagnostic.clone(), source),
                    source,
                );
            }
        }
        // Acquisition failures (including physical aliases) do not authorize
        // changing that member's files. Syntax-invalid files are isolated below.
        if !discovery.errors().is_empty() {
            continue;
        }
        for (relative, source) in discovery.files() {
            if !seen.insert(source.key.clone())
                || discovery
                    .diagnostics()
                    .iter()
                    .any(|(invalid, _)| invalid.name() == source.origin)
            {
                continue;
            }
            let path = input.root().join(relative);
            let result = CapturedFile::read(&path).and_then(|captured| {
                if captured.text != source.text.as_ref() {
                    return Err(Error::other("source changed after module discovery"));
                }
                format_captured(
                    SourceFile::new(&source.origin, source.text.as_ref()),
                    Some(captured),
                    format.check,
                    options(format, &config.format),
                    report,
                )
            });
            if let Err(error) = result {
                report.summary.incomplete += 1;
                report.operational(
                    &path.display().to_string(),
                    "KG_FORMAT_IO",
                    error.to_string(),
                );
            }
        }
    }
    Ok(())
}

fn package_error(error: PackageError) -> CliError {
    CliError::message(1, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs, process,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn project_selection_ignores_dependency_resolution_and_reports_incomplete_discovery() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/format-projects")
            .join(format!(
                "{}-{}",
                process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        for (path, text) in [
            (
                "Kagari.toml",
                "[workspace]\nnamespace='tests/format'\nmembers=['a','b']\ndefault-members=['a']",
            ),
            ("Kagari.tools.toml", "[format]\nindent_width=2"),
            (
                "a/Kagari.toml",
                "[package]\nname='a'\nversion='1.0.0'\n[dependencies]\nmissing={path='../not-present'}",
            ),
            ("b/Kagari.toml", "[package]\nname='b'\nversion='1.0.0'"),
            ("a/src/lib.kgr", "mod child;fn answer()->i32{true}"),
            ("a/src/child.kgr", "fn child(){}"),
            ("a/src/undeclared.kgr", "fn untouched(){}"),
            ("b/src/lib.kgr", "fn other(){}"),
        ] {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        let root = root.canonicalize().unwrap();
        run_from(&Default::default(), &Default::default(), &root).unwrap();
        assert_eq!(
            fs::read_to_string(root.join("a/src/lib.kgr")).unwrap(),
            "mod child;\n\nfn answer() -> i32 {\n  true\n}\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("a/src/child.kgr")).unwrap(),
            "fn child() {}\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("a/src/undeclared.kgr")).unwrap(),
            "fn untouched(){}"
        );
        assert_eq!(
            fs::read_to_string(root.join("b/src/lib.kgr")).unwrap(),
            "fn other(){}"
        );
        assert!(!root.join("Kagari.lock").exists());
        assert!(!root.join("target").exists());
        fs::write(root.join("a/src/child.kgr"), "mod hidden;fn broken(").unwrap();
        assert!(
            run_from(
                &ProjectArgs {
                    workspace: true,
                    ..Default::default()
                },
                &Default::default(),
                &root
            )
            .is_err()
        );
        assert_eq!(
            fs::read_to_string(root.join("b/src/lib.kgr")).unwrap(),
            "fn other() {}\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("a/src/child.kgr")).unwrap(),
            "mod hidden;fn broken("
        );
        assert!(!root.join("Kagari.lock").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            fs::write(root.join("a/src/lib.kgr"), "mod one;mod two;").unwrap();
            fs::write(root.join("a/src/one.kgr"), "fn one(){}").unwrap();
            symlink(root.join("a/src/one.kgr"), root.join("a/src/two.kgr")).unwrap();
            assert!(
                run_from(
                    &ProjectArgs {
                        packages: vec!["a".into()],
                        ..Default::default()
                    },
                    &Default::default(),
                    &root
                )
                .is_err()
            );
            assert_eq!(
                fs::read_to_string(root.join("a/src/lib.kgr")).unwrap(),
                "mod one;mod two;"
            );
            assert_eq!(
                fs::read_to_string(root.join("a/src/one.kgr")).unwrap(),
                "fn one(){}"
            );
        }
    }
}
