//! Command selection and validation, independent of filesystem discovery.

use crate::{CliError, report::MessageFormat};
use kagari_format::Newline;
use kagari_lint::{
    policy::{Level, Policy},
    registry::LintId,
};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Cli {
    pub command: Command,
    pub jit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Command {
    Lint {
        source: PathBuf,
        format: MessageFormat,
        policy: Policy,
    },
    LintProject {
        project: ProjectArgs,
        policy: Policy,
    },
    ListRules,
    Explain {
        rule: LintId,
    },
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
    Disassemble {
        artifact: PathBuf,
        mode: DisassemblyMode,
    },
    Project {
        action: ProjectAction,
        options: ProjectArgs,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum DisassemblyMode {
    #[default]
    All,
    ListFunctions,
    Function(String),
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
    lint: Policy,
    lint_option: bool,
    list_rules: bool,
    explain: Option<LintId>,
    positionals: Vec<String>,
    project: ProjectArgs,
    jit: bool,
    jit_option: bool,
    message_option: bool,
    format: FormatArgs,
    format_option: bool,
    disassembly: Option<DisassemblyMode>,
}

impl Arguments {
    fn collect(args: impl IntoIterator<Item = String>) -> Result<Self, CliError> {
        let mut result = Self::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--list-functions" | "--function" => {
                    if result.disassembly.is_some() {
                        return Err(CliError::message(
                            2,
                            "--list-functions and --function cannot be repeated or combined",
                        ));
                    }
                    result.disassembly = Some(if arg == "--list-functions" {
                        DisassemblyMode::ListFunctions
                    } else {
                        DisassemblyMode::Function(value(&mut args, &arg)?)
                    });
                }
                "--list-rules" => {
                    result.list_rules = true;
                    result.lint_option = true;
                }
                "--explain" => {
                    result.explain = Some(lint_id(&value(&mut args, &arg)?)?);
                    result.lint_option = true;
                }
                "-A" | "-W" | "-D" => {
                    result.lint_option = true;
                    let rule = value(&mut args, &arg)?;
                    if arg == "-D" && rule == "warnings" {
                        result.lint.deny_warnings = true;
                    } else {
                        let id = lint_id(&rule)?;
                        let level = match arg.as_str() {
                            "-A" => Level::Allow,
                            "-W" => Level::Warn,
                            _ => Level::Deny,
                        };
                        result.lint.overrides.insert(id, level);
                    }
                }
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
                    return Err(CliError::message(
                        2,
                        format!("unknown option `{}`", option.escape_default()),
                    ));
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
                | "lint"
                | "parse"
                | "check"
                | "emit"
                | "build"
                | "run"
                | "run-artifact"
                | "build-aot"
                | "run-aot"
                | "disasm"
        );
        let rest = &parsed.positionals[usize::from(explicit)..];
        if rest.len() > 1 {
            return Err(CliError::message(
                2,
                "expected at most one source or artifact path",
            ));
        }
        let name = if explicit { first.as_str() } else { "implicit" };
        if parsed.disassembly.is_some() && name != "disasm" {
            return Err(CliError::message(
                2,
                "--list-functions and --function apply only to disasm",
            ));
        }
        if parsed.lint_option && name != "lint" {
            return Err(CliError::message(2, "lint options apply only to lint"));
        }
        if name == "lint" && (parsed.list_rules || parsed.explain.is_some()) {
            parsed.source_options()?;
            if !rest.is_empty()
                || parsed.list_rules && parsed.explain.is_some()
                || parsed.message_option
                || parsed.jit_option
                || parsed.format_option
                || parsed.project.output.is_some()
                || parsed.lint != Policy::default()
            {
                return Err(CliError::message(
                    2,
                    "rule help cannot be combined with execution options",
                ));
            }
            return Ok(Self {
                command: parsed
                    .explain
                    .map_or(Command::ListRules, |rule| Command::Explain { rule }),
                jit: false,
            });
        }
        if parsed.format_option && name != "fmt" {
            return Err(CliError::message(2, "format options apply only to fmt"));
        }
        if parsed.message_option && !matches!(name, "check" | "lint") {
            return Err(CliError::message(
                2,
                "--message-format applies only to check or lint",
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
        if name == "lint" && rest.is_empty() {
            return Ok(Self {
                command: Command::LintProject {
                    project: parsed.project,
                    policy: parsed.lint,
                },
                jit: false,
            });
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
            "lint" => Command::Lint {
                source,
                format: parsed.project.message_format,
                policy: parsed.lint,
            },
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
            "disasm" => {
                if source.as_os_str().is_empty() {
                    return Err(CliError::message(2, "missing artifact path"));
                }
                Command::Disassemble {
                    artifact: source,
                    mode: parsed.disassembly.unwrap_or_default(),
                }
            }
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

fn lint_id(name: &str) -> Result<LintId, CliError> {
    LintId::parse(name).ok_or_else(|| {
        CliError::message(2, format!("unknown lint rule `{}`", name.escape_default()))
    })
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
        "  kg lint [script.kgr] [project options] [-A RULE|-W RULE|-D RULE|-D warnings] [--message-format human|json]",
        "  kg lint --list-rules | --explain RULE",
        "  kg check [script.kgr] [project options] [--message-format human|json]",
        "  kg build [-o artifact.kbc] [project options]",
        "  kg emit [-o artifact.kbc] <script.kgr>",
        "  kg run [--jit] [script.kgr] [project options]",
        "  kg run-artifact [--jit] <artifact.kbc>",
        "  kg disasm <artifact.kbc> [--list-functions | --function <selector>]",
        "    Inspect one local file offline; write plain text to stdout (no execution).",
        "    Select an exact qualified name or artifact-local slot, e.g. --function '@0:3'.",
        "project options: --manifest-path Kagari.toml, -p NAME, --workspace,",
        "                 --locked, --offline, --frozen, --input-root DIRECTORY",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::{Cli, Command, DisassemblyMode, FormatArgs, ProjectAction, ProjectArgs};
    use crate::report::MessageFormat;
    use rstest::rstest;

    #[rstest]
    #[case::all(&[], DisassemblyMode::All)]
    #[case::list_functions(&["--list-functions"], DisassemblyMode::ListFunctions)]
    #[case::artifact_slot(
                &["--function", "@0:3"],
                DisassemblyMode::Function("@0:3".into()),
            )]
    #[case::raw_unicode_selector(
                &["--function", "module::名前\u{202e}"],
                DisassemblyMode::Function("module::名前\u{202e}".into()),
            )]
    fn disassembly_modes_preserve_raw_selectors_and_allow_option_ordering(
        #[case] options: &[&str],
        #[case] mode: DisassemblyMode,
        #[values(0, 1, 2)] position: usize,
    ) {
        let mut args = vec!["disasm", "file.kbc"];
        args.splice(position..position, options.iter().copied());
        assert_eq!(
            Cli::parse(args.iter().map(|arg| (*arg).to_owned())).unwrap(),
            Cli {
                command: Command::Disassemble {
                    artifact: "file.kbc".into(),
                    mode: mode.clone(),
                },
                jit: false,
            },
            "{args:?}",
        );
    }

    #[rstest]
    #[case::missing_artifact(&["disasm"])]
    #[case::empty_artifact(&["disasm", ""])]
    #[case::stdin_artifact(&["disasm", "-"])]
    #[case::extra_artifact(&["disasm", "file.kbc", "extra.kbc"])]
    #[case::missing_selector(&["disasm", "file.kbc", "--function"])]
    #[case::empty_selector(&["disasm", "file.kbc", "--function", ""])]
    #[case::selector_is_option(&["disasm", "file.kbc", "--function", "--list-functions"])]
    #[case::duplicate_list_trailing(&["disasm", "file.kbc", "--list-functions", "--list-functions"])]
    #[case::duplicate_list_across_command(&["--list-functions", "disasm", "--list-functions", "file.kbc"])]
    #[case::duplicate_selector_across_command(&[
                "--function",
                "@0:0",
                "disasm",
                "file.kbc",
                "--function",
                "@0:1",
            ])]
    #[case::list_then_selector_across_command(&[
                "--list-functions",
                "disasm",
                "--function",
                "@0:0",
                "file.kbc",
            ])]
    #[case::selector_then_list_across_command(&[
                "--function",
                "@0:0",
                "disasm",
                "file.kbc",
                "--list-functions",
            ])]
    #[case::post_terminator_extra_operand(&["disasm", "file.kbc", "--", "--list-functions"])]
    #[case::duplicate_selector_trailing(&[
                "disasm",
                "file.kbc",
                "--function",
                "@0:0",
                "--function",
                "@0:1",
            ])]
    #[case::list_then_selector_trailing(&[
                "disasm",
                "file.kbc",
                "--list-functions",
                "--function",
                "@0:0",
            ])]
    #[case::selector_then_list_trailing(&[
                "disasm",
                "file.kbc",
                "--function",
                "@0:0",
                "--list-functions",
            ])]
    #[case::list_on_run_artifact(&["run-artifact", "file.kbc", "--list-functions"])]
    #[case::selector_on_check(&["check", "--function", "main"])]
    #[case::selector_on_implicit_artifact(&["file.kbc", "--function", "main"])]
    fn disassembly_rejects_invalid_operands_and_selection_modes(#[case] args: &[&str]) {
        let error = Cli::parse(args.iter().map(|arg| (*arg).to_owned())).unwrap_err();
        assert_eq!(error.exit_code(), 2, "{args:?}");
    }

    #[rstest]
    #[case::jit(&["--jit"])]
    #[case::no_jit(&["--no-jit"])]
    #[case::manifest(&["--manifest-path", "Kagari.toml"])]
    #[case::input_root(&["--input-root", "."])]
    #[case::package_short(&["-p", "game"])]
    #[case::package_long(&["--package", "game"])]
    #[case::workspace(&["--workspace"])]
    #[case::locked(&["--locked"])]
    #[case::offline(&["--offline"])]
    #[case::frozen(&["--frozen"])]
    #[case::output_short(&["-o", "out.kbc"])]
    #[case::output_long(&["--output", "out.kbc"])]
    #[case::message_format(&["--message-format", "human"])]
    #[case::format_check(&["--check"])]
    #[case::indent_width(&["--indent-width", "4"])]
    #[case::max_width(&["--max-width", "80"])]
    #[case::newline(&["--newline", "lf"])]
    #[case::list_rules(&["--list-rules"])]
    #[case::explain_unknown(&["--explain", "unknown"])]
    #[case::deny_warnings(&["-D", "warnings"])]
    #[case::allow_unknown(&["-A", "unknown"])]
    #[case::warn_unknown(&["-W", "unknown"])]
    #[case::escaped_unknown_option(&["--unknown\n\u{202e}"])]
    fn disassembly_rejects_unrelated_options_including_leading_options(
        #[case] options: &[&str],
        #[values(0, 1, 2)] position: usize,
    ) {
        let mut args = vec!["disasm", "file.kbc"];
        args.splice(position..position, options.iter().copied());
        let error = Cli::parse(args.iter().map(|arg| (*arg).to_owned())).unwrap_err();
        assert_eq!(error.exit_code(), 2, "{args:?}");
        assert!(!error.to_string().contains('\n'), "{args:?}");
        assert!(!error.to_string().contains('\u{202e}'), "{args:?}");
    }

    #[rstest]
    #[case::frozen_package_build(&["--frozen", "build", "-p", "game", "--manifest-path", "project/Kagari.toml"], Cli { command: Command::Project {action: ProjectAction::Build, options: ProjectArgs { packages: vec!["game".into()], locked: true, offline: true, manifest: Some("project/Kagari.toml".into()), ..Default::default()}}, jit: false})]
    #[case::trailing_jit_run(&["run", "main.kgr", "--jit"], Cli {command: Command::RunSource {source: "main.kgr".into()}, jit: true})]
    #[case::json_source_check(&["check", "main.kgr", "--message-format", "json"], Cli {command: Command::Check {source: "main.kgr".into(), format: MessageFormat::Json}, jit: false})]
    #[case::stdin_format_width(&["fmt", "-", "--max-width", "80"], Cli {command: Command::Format {source: "-".into(), options: FormatArgs {max_width: Some(80), ..Default::default()}}, jit: false})]
    fn project_options_are_order_independent_and_command_scoped(
        #[case] args: &[&str],
        #[case] expected: Cli,
    ) {
        assert_eq!(
            Cli::parse(args.iter().map(|arg| (*arg).to_owned())).unwrap(),
            expected
        );
    }

    #[rstest]
    #[case::source_with_lock(&["check", "main.kgr", "--locked"])]
    #[case::check_output(&["check", "-o", "ignored.kbc"])]
    #[case::workspace_and_package(&["build", "--workspace", "-p", "game"])]
    #[case::missing_manifest_value(&["run", "--manifest-path"])]
    #[case::build_source_operand(&["build", "main.kgr"])]
    #[case::parse_unknown(&["parse", "--unknown"])]
    #[case::check_jit(&["check", "--jit"])]
    #[case::yaml_message_format(&["check", "--message-format", "yaml"])]
    #[case::build_json(&["build", "--message-format", "json"])]
    #[case::unsupported_fix(&["check", "--fix"])]
    #[case::stdin_format_check(&["fmt", "-", "--check"])]
    #[case::source_fmt_lock(&["fmt", "file.kgr", "--locked"])]
    #[case::workspace_fmt_frozen(&["fmt", "--workspace", "--frozen"])]
    #[case::zero_max_width(&["fmt", "file.kgr", "--max-width", "0"])]
    #[case::stdin_check(&["check", "-"])]
    #[case::format_flag_on_check(&["check", "--check"])]
    fn project_options_reject_invalid_combinations(#[case] args: &[&str]) {
        assert!(
            Cli::parse(args.iter().map(|arg| (*arg).to_owned())).is_err(),
            "{args:?}"
        );
    }

    #[test]
    fn disassembly_accepts_dash_path_after_terminator() {
        assert!(matches!(
            Cli::parse(["disasm", "--", "--file.kbc"].map(str::to_owned))
                .unwrap()
                .command,
            Command::Disassemble { artifact, .. } if artifact.to_str() == Some("--file.kbc")
        ));
    }

    #[test]
    fn disassembly_selector_validation_is_not_argument_parsing() {
        // Selector syntax and existence belong to library selection (status 1), not parsing.
        assert!(
            Cli::parse(["disasm", "file.kbc", "--function", "@bad"].map(str::to_owned)).is_ok()
        );
    }
}
