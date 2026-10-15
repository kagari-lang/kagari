//! Command selection and validation, independent of filesystem discovery.

use crate::{CliError, report::MessageFormat};
use kagari_format::Newline;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Cli {
    pub command: Command,
    pub jit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Command {
    FormatProject {
        project: ProjectArgs,
        options: FormatArgs,
    },
    Format {
        source: PathBuf,
        options: FormatArgs,
    },
    BuildAot {
        source: PathBuf,
        output: PathBuf,
    },
    RunAot {
        bundle: PathBuf,
    },
    Parse {
        source: PathBuf,
    },
    Check {
        source: PathBuf,
        format: MessageFormat,
    },
    Emit {
        source: PathBuf,
        output: PathBuf,
    },
    RunSource {
        source: PathBuf,
    },
    RunArtifact {
        artifact: PathBuf,
    },
    Project {
        action: ProjectAction,
        options: ProjectArgs,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProjectAction {
    Check,
    Build,
    Run,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ProjectArgs {
    pub message_format: MessageFormat,
    pub manifest: Option<PathBuf>,
    pub input_root: Option<PathBuf>,
    pub packages: Vec<String>,
    pub workspace: bool,
    pub locked: bool,
    pub offline: bool,
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FormatArgs {
    pub check: bool,
    pub indent_width: Option<usize>,
    pub max_width: Option<usize>,
    pub newline: Option<Newline>,
}

#[derive(Default)]
struct Arguments {
    positionals: Vec<String>,
    project: ProjectArgs,
    jit: bool,
    jit_option: bool,
    message_option: bool,
    format: FormatArgs,
    format_option: bool,
}

impl Arguments {
    fn collect(args: impl IntoIterator<Item = String>) -> Result<Self, CliError> {
        let mut result = Self::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--check" => {
                    result.format.check = true;
                    result.format_option = true;
                }
                "--indent-width" | "--max-width" => {
                    result.format_option = true;
                    let number = value(&mut args, &arg)?.parse::<usize>().map_err(|_| {
                        CliError::message(2, "format width must be a positive integer")
                    })?;
                    if arg == "--indent-width" {
                        if !(1..=16).contains(&number) {
                            return Err(CliError::message(2, "indent width must be 1..=16"));
                        }
                        result.format.indent_width = Some(number);
                    } else {
                        if !(20..=1000).contains(&number) {
                            return Err(CliError::message(2, "maximum width must be 20..=1000"));
                        }
                        result.format.max_width = Some(number);
                    }
                }
                "--newline" => {
                    result.format_option = true;
                    result.format.newline = Some(match value(&mut args, &arg)?.as_str() {
                        "preserve" => Newline::Preserve,
                        "lf" => Newline::Lf,
                        "crlf" => Newline::Crlf,
                        _ => {
                            return Err(CliError::message(
                                2,
                                "newline must be preserve, lf or crlf",
                            ));
                        }
                    });
                }
                "-" => result.positionals.push(arg),
                "--message-format" => {
                    result.message_option = true;
                    result.project.message_format = match value(&mut args, &arg)?.as_str() {
                        "human" => MessageFormat::Human,
                        "json" => MessageFormat::Json,
                        _ => {
                            return Err(CliError::message(
                                2,
                                "--message-format expects human or json",
                            ));
                        }
                    };
                }
                "--" => {
                    result.positionals.extend(args);
                    break;
                }
                "--help" | "-h" => return Err(CliError::usage()),
                "--jit" | "--no-jit" => {
                    result.jit = arg == "--jit";
                    result.jit_option = true;
                }
                "-o" | "--output" => result.project.output = Some(value(&mut args, &arg)?.into()),
                "--manifest-path" => result.project.manifest = Some(value(&mut args, &arg)?.into()),
                "--input-root" => result.project.input_root = Some(value(&mut args, &arg)?.into()),
                "-p" | "--package" => result.project.packages.push(value(&mut args, &arg)?),
                "--workspace" => result.project.workspace = true,
                "--locked" => result.project.locked = true,
                "--offline" => result.project.offline = true,
                "--frozen" => {
                    result.project.locked = true;
                    result.project.offline = true;
                }
                option if option.starts_with('-') => {
                    return Err(CliError::message(2, format!("unknown option `{option}`")));
                }
                _ => result.positionals.push(arg),
            }
        }
        if result.project.workspace && !result.project.packages.is_empty() {
            return Err(CliError::message(
                2,
                "--workspace and --package are mutually exclusive",
            ));
        }
        Ok(result)
    }

    fn source_options(&self) -> Result<(), CliError> {
        let project = &self.project;
        if project.manifest.is_some()
            || project.input_root.is_some()
            || !project.packages.is_empty()
            || project.workspace
            || project.locked
            || project.offline
        {
            return Err(CliError::message(
                2,
                "project options cannot be combined with a source or artifact path",
            ));
        }
        Ok(())
    }
}

impl Cli {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, CliError> {
        let parsed = Arguments::collect(args)?;
        let Some(first) = parsed.positionals.first() else {
            return Err(CliError::usage());
        };
        if first == "help" {
            return Err(CliError::usage());
        }
        let explicit = matches!(
            first.as_str(),
            "fmt"
                | "parse"
                | "check"
                | "emit"
                | "build"
                | "run"
                | "run-artifact"
                | "build-aot"
                | "run-aot"
        );
        let rest = &parsed.positionals[usize::from(explicit)..];
        if rest.len() > 1 {
            return Err(CliError::message(
                2,
                "expected at most one source or artifact path",
            ));
        }
        let name = if explicit { first.as_str() } else { "implicit" };
        if parsed.format_option && name != "fmt" {
            return Err(CliError::message(2, "format options apply only to fmt"));
        }
        if parsed.message_option && name != "check" {
            return Err(CliError::message(
                2,
                "--message-format applies only to check",
            ));
        }
        if parsed.jit_option && !matches!(name, "run" | "run-artifact" | "implicit") {
            return Err(CliError::message(2, "JIT options apply only to execution"));
        }
        if parsed.project.output.is_some() && !matches!(name, "emit" | "build" | "build-aot") {
            return Err(CliError::message(
                2,
                "--output applies only to emit, build or build-aot",
            ));
        }
        if name == "fmt" && (parsed.project.locked || parsed.project.offline) {
            return Err(CliError::message(
                2,
                "fmt does not accept resolution or lock options",
            ));
        }
        if name == "fmt" && rest.is_empty() {
            return Ok(Self {
                command: Command::FormatProject {
                    project: parsed.project,
                    options: parsed.format,
                },
                jit: false,
            });
        }
        if matches!(name, "check" | "build" | "run") && rest.is_empty() {
            let action = match name {
                "check" => ProjectAction::Check,
                "build" => ProjectAction::Build,
                _ => ProjectAction::Run,
            };
            return Ok(Self {
                command: Command::Project {
                    action,
                    options: parsed.project,
                },
                jit: parsed.jit,
            });
        }
        if name == "build" {
            return Err(CliError::message(
                2,
                "build selects packages; use --manifest-path or --package",
            ));
        }
        parsed.source_options()?;
        let source = rest
            .first()
            .map(PathBuf::from)
            .ok_or_else(|| CliError::message(2, "missing source or artifact path"))?;
        if source.as_os_str() == "-" && (name != "fmt" || parsed.format.check) {
            return Err(CliError::message(
                2,
                "stdin is supported only by fmt without --check",
            ));
        }
        let command = match name {
            "fmt" => Command::Format {
                source,
                options: parsed.format,
            },
            "build-aot" => Command::BuildAot {
                output: parsed
                    .project
                    .output
                    .unwrap_or_else(|| source.with_extension("aot")),
                source,
            },
            "run-aot" => Command::RunAot { bundle: source },
            "parse" => Command::Parse { source },
            "check" => Command::Check {
                source,
                format: parsed.project.message_format,
            },
            "emit" => Command::Emit {
                output: parsed
                    .project
                    .output
                    .unwrap_or_else(|| source.with_extension("kbc")),
                source,
            },
            "run-artifact" => Command::RunArtifact { artifact: source },
            "implicit" if source.extension().and_then(|ext| ext.to_str()) == Some("kbc") => {
                Command::RunArtifact { artifact: source }
            }
            _ => Command::RunSource { source },
        };
        Ok(Self {
            command,
            jit: parsed.jit,
        })
    }
}

fn value(args: &mut impl Iterator<Item = String>, option: &str) -> Result<String, CliError> {
    args.next()
        .filter(|value| !value.is_empty() && !value.starts_with('-'))
        .ok_or_else(|| CliError::message(2, format!("missing value for {option}")))
}

pub(crate) fn usage() -> String {
    [
        "usage:",
        "  kg fmt [script.kgr|-] [project options] [--check] [--indent-width N] [--max-width N] [--newline preserve|lf|crlf]",
        "  kg parse <script.kgr>",
        "  kg build-aot <script.kgr> [-o bundle-dir] (feature aot; KAGARI_AOT_LINKER selects linker)",
        "  kg run-aot <trusted-bundle-dir> (feature aot; executes trusted native code)",
        "  kg check [script.kgr] [project options] [--message-format human|json]",
        "  kg build [-o artifact.kbc] [project options]",
        "  kg emit [-o artifact.kbc] <script.kgr>",
        "  kg run [--jit] [script.kgr] [project options]",
        "  kg run-artifact [--jit] <artifact.kbc>",
        "project options: --manifest-path Kagari.toml, -p NAME, --workspace,",
        "                 --locked, --offline, --frozen, --input-root DIRECTORY",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::{Cli, Command, ProjectAction};
    use crate::report::MessageFormat;

    #[test]
    fn project_options_are_order_independent_and_command_scoped() {
        let parse = |args: &[&str]| Cli::parse(args.iter().map(|arg| (*arg).to_owned()));
        let cli = parse(&[
            "--frozen",
            "build",
            "-p",
            "game",
            "--manifest-path",
            "project/Kagari.toml",
        ])
        .unwrap();
        let Command::Project { action, options } = cli.command else {
            panic!("project command")
        };
        assert_eq!(action, ProjectAction::Build);
        assert_eq!(options.packages, ["game"]);
        assert!(options.locked && options.offline);
        assert_eq!(
            options.manifest.unwrap().to_str(),
            Some("project/Kagari.toml")
        );
        assert!(parse(&["run", "main.kgr", "--jit"]).unwrap().jit);
        assert!(matches!(
            parse(&["check", "main.kgr", "--message-format", "json"])
                .unwrap()
                .command,
            Command::Check {
                format: MessageFormat::Json,
                ..
            }
        ));
        assert!(matches!(
            parse(&["fmt", "-", "--max-width", "80"]).unwrap().command,
            Command::Format { .. }
        ));
        for args in [
            vec!["check", "main.kgr", "--locked"],
            vec!["check", "-o", "ignored.kbc"],
            vec!["build", "--workspace", "-p", "game"],
            vec!["run", "--manifest-path"],
            vec!["build", "main.kgr"],
            vec!["parse", "--unknown"],
            vec!["check", "--jit"],
            vec!["check", "--message-format", "yaml"],
            vec!["build", "--message-format", "json"],
            vec!["check", "--fix"],
            vec!["fmt", "-", "--check"],
            vec!["fmt", "file.kgr", "--locked"],
            vec!["fmt", "--workspace", "--frozen"],
            vec!["fmt", "file.kgr", "--max-width", "0"],
            vec!["check", "-"],
            vec!["check", "--check"],
        ] {
            assert!(parse(&args).is_err(), "{args:?}");
        }
    }
}
