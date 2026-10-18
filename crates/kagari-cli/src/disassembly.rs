//! Bounded local artifact input and fallible buffered output for offline inspection.

use crate::{CliError, args::DisassemblyMode};
use kagari_bytecode::{artifact::MAX_ARTIFACT_BYTES, disassembly::PreparedDisassembly};
use std::{
    fs::File,
    io::{self, BufWriter, Error, ErrorKind, Read, Write},
    path::Path,
};

pub(crate) fn run(path: &Path, mode: &DisassemblyMode) -> Result<(), CliError> {
    run_with_writer(path, mode, &mut io::stdout().lock())
}

pub(crate) fn run_with_writer(
    path: &Path,
    mode: &DisassemblyMode,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let prepared = prepare(path)?;
    buffered_output(writer, |writer| match mode {
        DisassemblyMode::ListFunctions => prepared.write_directory(writer).map_err(output_error),
        DisassemblyMode::All | DisassemblyMode::Function(_) => {
            let selector = match mode {
                DisassemblyMode::Function(selector) => Some(selector.as_str()),
                _ => None,
            };
            prepared
                .write_disassembly(writer, selector)
                .map_err(|error| CliError::message(1, error.to_string()))
        }
    })
}

fn prepare(path: &Path) -> Result<PreparedDisassembly, CliError> {
    let read = || -> io::Result<Vec<u8>> {
        let file = File::open(path)?;
        if !file.metadata()?.is_file() {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "disasm requires one regular local file",
            ));
        }
        read_bounded(file, MAX_ARTIFACT_BYTES)
    };
    let bytes = read().map_err(|error| {
        CliError::message(
            1,
            format!(
                "cannot read artifact \"{}\": {}",
                path.to_string_lossy().escape_default(),
                error.to_string().escape_default(),
            ),
        )
    })?;
    PreparedDisassembly::from_bytes(&bytes).map_err(|error| CliError::message(1, error.to_string()))
}

fn read_bounded(reader: impl Read, limit: u64) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!("artifact exceeds size limit ({limit} bytes)"),
        ));
    }
    Ok(bytes)
}

fn buffered_output<W: Write>(
    writer: &mut W,
    render: impl FnOnce(&mut BufWriter<&mut W>) -> Result<(), CliError>,
) -> Result<(), CliError> {
    let mut writer = BufWriter::new(writer);
    let result = render(&mut writer).and_then(|()| writer.flush().map_err(output_error));
    // Do not let BufWriter::drop retry buffered bytes after the first write failure.
    let _ = writer.into_parts();
    result
}

fn output_error(error: io::Error) -> CliError {
    CliError::message(
        1,
        format!(
            "cannot write disassembly: {}",
            error.to_string().escape_default(),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::{buffered_output, output_error, prepare, read_bounded};
    use std::{
        io::{self, Cursor, Read, Write},
        path::Path,
    };

    #[test]
    fn bounded_input_accepts_limit_and_consumes_only_one_sentinel() {
        const LIMIT: u64 = 32;
        for length in [0, 1, LIMIT as usize] {
            let input = vec![42; length];
            assert_eq!(read_bounded(input.as_slice(), LIMIT).unwrap(), input);
        }
        let mut input = Cursor::new(vec![42; LIMIT as usize + 16]);
        let error = read_bounded(&mut input, LIMIT).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(input.position(), LIMIT + 1);

        let mut endless = io::repeat(42);
        assert_eq!(
            read_bounded(&mut endless, LIMIT).unwrap_err().kind(),
            io::ErrorKind::InvalidData,
        );
    }

    struct ReadFailure;

    impl Read for ReadFailure {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("read failed"))
        }
    }

    #[test]
    fn bounded_input_preserves_reader_errors() {
        assert_eq!(
            read_bounded(ReadFailure, 32).unwrap_err().to_string(),
            "read failed",
        );
    }

    #[derive(Default)]
    struct Output {
        bytes: Vec<u8>,
        writes: usize,
        flushes: usize,
        fail_write: bool,
        fail_flush: bool,
    }

    impl Write for Output {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.writes += 1;
            if self.fail_write {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "closed pipe\n\u{1b}[31m\u{202e}\u{200d}",
                ));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flushes += 1;
            if self.fail_flush {
                return Err(io::Error::other("flush failed\u{2066}"));
            }
            Ok(())
        }
    }

    #[test]
    fn buffered_output_flushes_success_and_reports_flush_failure() {
        for fail_flush in [false, true] {
            let mut output = Output {
                fail_flush,
                ..Output::default()
            };
            let result = buffered_output(&mut output, |writer| {
                writer.write_all(b"disassembly\n").map_err(output_error)
            });
            assert_eq!(output.bytes, b"disassembly\n");
            assert_eq!(output.flushes, 1);
            if fail_flush {
                let error = result.unwrap_err();
                assert_eq!(error.exit_code(), 1);
                assert!(error.to_string().contains("flush failed\\u{2066}"));
            } else {
                result.unwrap();
            }
        }
    }

    #[test]
    fn broken_pipe_stops_writes_without_drop_retry_or_flush() {
        // Exercise both a failure while rendering and one draining the final buffer.
        for during_render in [false, true] {
            let mut output = Output {
                fail_write: true,
                ..Output::default()
            };
            let error = buffered_output(&mut output, |writer| {
                writer.write_all(b"buffered prefix").map_err(output_error)?;
                if during_render {
                    writer
                        .write_all(&vec![b'x'; writer.capacity()])
                        .map_err(output_error)?;
                    panic!("rendering must stop at the first write error");
                }
                Ok(())
            })
            .unwrap_err();
            assert_eq!(error.exit_code(), 1);
            assert_eq!(output.writes, 1);
            assert_eq!(output.flushes, 0);
            assert_eq!(output.bytes, b"");
            assert!(
                error
                    .to_string()
                    .contains("closed pipe\\n\\u{1b}[31m\\u{202e}\\u{200d}")
            );
        }
    }

    #[test]
    fn file_diagnostics_escape_control_and_unicode_format_characters() {
        // NUL makes this an invalid path on every supported platform.
        let error = prepare(Path::new("missing\0\n\u{1b}[31m\u{202e}\u{200d}.kbc")).unwrap_err();
        assert_eq!(error.exit_code(), 1);
        assert!(
            error
                .to_string()
                .contains("missing\\u{0}\\n\\u{1b}[31m\\u{202e}\\u{200d}.kbc")
        );
        assert!(error.to_string().is_ascii());
        assert!(!error.to_string().contains('\n'));
    }
}
