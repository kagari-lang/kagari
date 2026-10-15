//! Formatter command IO; layout remains in the pure formatter crate.

pub(crate) mod project;
mod writeback;

use crate::{
    CliError,
    args::FormatArgs,
    report::{MessageFormat, Report},
    tool_config::{FormatConfig, Newline as ConfigNewline},
};
use kagari_format::{FormatError, Newline, Options, format};
use kagari_source::source::SourceFile;
use std::{
    io::{self, Error, Read, Write},
    path::Path,
};
use writeback::CapturedFile;

pub(crate) fn run(path: &Path, args: &FormatArgs) -> Result<(), CliError> {
    let mut report = Report::new("fmt");
    if let Err(error) = run_into(path, args, &mut report) {
        report.summary.incomplete += 1;
        report.operational(
            &path.display().to_string(),
            "KG_FORMAT_IO",
            error.to_string(),
        );
    }
    report.finish(MessageFormat::Human)
}

fn run_into(path: &Path, args: &FormatArgs, report: &mut Report) -> io::Result<()> {
    let captured = if path.as_os_str() == "-" {
        None
    } else {
        Some(CapturedFile::read(path)?)
    };
    let text = if let Some(captured) = &captured {
        captured.text.clone()
    } else {
        let mut text = String::new();
        io::stdin()
            .take(4 * 1024 * 1024 + 1)
            .read_to_string(&mut text)?;
        if text.len() > 4 * 1024 * 1024 {
            return Err(Error::other("stdin exceeds source byte limit"));
        }
        text
    };
    let source = SourceFile::new(path.display().to_string(), text);
    let options = options(args, &Default::default());
    format_captured(source, captured, args.check, options, report)
}

fn options(args: &FormatArgs, config: &FormatConfig) -> Options {
    Options {
        indent_width: args.indent_width.unwrap_or(config.indent_width),
        max_width: args.max_width.unwrap_or(config.max_width),
        newline: args.newline.unwrap_or(match config.newline {
            ConfigNewline::Preserve => Newline::Preserve,
            ConfigNewline::Lf => Newline::Lf,
            ConfigNewline::Crlf => Newline::Crlf,
        }),
    }
}

fn format_captured(
    source: SourceFile,
    captured: Option<CapturedFile>,
    check: bool,
    options: Options,
    report: &mut Report,
) -> io::Result<()> {
    match format(&source, options, &Default::default()) {
        Ok(output) => {
            if let Some(captured) = captured {
                if check && captured.text != output {
                    report.summary.failed += 1;
                    report.operational(
                        source.name(),
                        "KG_FORMAT_DIFFERENCE",
                        "source requires formatting",
                    );
                    return Ok(());
                }
                if !check {
                    captured.replace(&output)?;
                }
            } else {
                io::stdout().lock().write_all(output.as_bytes())?;
            }
            report.summary.succeeded += 1;
        }
        Err(FormatError::Diagnostics(diagnostics)) => {
            report.summary.failed += 1;
            for diagnostic in diagnostics {
                report.source_diagnostic(source.name(), diagnostic, &source);
            }
        }
        Err(error) => {
            report.summary.incomplete += 1;
            report.operational(
                source.name(),
                "KG_FORMAT",
                format!("formatting failed without changing source: {error:?}"),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs, process,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn file_command_checks_without_write_and_preserves_malformed_input() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/format-commands")
            .join(format!(
                "{}-{}",
                process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("source.kgr");
        fs::write(&path, "fn answer()->i32{true}").unwrap();
        let metadata = fs::metadata(&path).unwrap();
        assert_eq!(
            run(
                &path,
                &FormatArgs {
                    check: true,
                    ..Default::default()
                }
            )
            .unwrap_err()
            .exit_code(),
            1
        );
        assert_eq!(
            fs::metadata(&path).unwrap().modified().unwrap(),
            metadata.modified().unwrap()
        );
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        run(&path, &Default::default()).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "fn answer() -> i32 {\n    true\n}\n"
        );
        run(
            &path,
            &FormatArgs {
                check: true,
                ..Default::default()
            },
        )
        .unwrap();
        fs::write(&path, "fn broken(").unwrap();
        assert!(run(&path, &Default::default()).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "fn broken(");
    }
}
