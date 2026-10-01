//! Versioned portable native input. Analysis seals are never serialized.
use bincode::{DefaultOptions, Options};
use kagari_abi::{
    decode_limits,
    version::{KAGARI_RUNTIME_ABI_VERSION, KAGARI_RUNTIME_HELPER_ABI_VERSION},
};
use kagari_common::{cancellation::CancellationToken, identity::ModuleIdentity};
use serde::{Deserialize, Deserializer, Serialize};
use smallvec::{Array, SmallVec};
use std::io::{self, Error as IoError, Read, Write};

use crate::{
    function::MirModule,
    program::{ProgramError, VerifiedMirProgram, verify_program},
};

pub const MIR_MAGIC: [u8; 4] = *b"KMIR";
pub const MIR_FORMAT_VERSION: u16 = 14;
pub const MAX_MIR_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum MirCodecError {
    #[error("MIR codec cancelled")]
    Cancelled,
    #[error("unsupported MIR format or execution ABI")]
    Version,
    #[error("MIR encoded byte limit exceeded")]
    SizeLimit,
    #[error("invalid MIR encoding: {0}")]
    Encoding(String),
    #[error("MIR program verification failed: {0:?}")]
    Verification(ProgramError),
}

#[derive(Serialize, Deserialize)]
struct Header {
    magic: [u8; 4],
    version: u16,
    runtime_abi: String,
    helper_abi: String,
}
impl Header {
    fn current() -> Self {
        Self {
            magic: MIR_MAGIC,
            version: MIR_FORMAT_VERSION,
            runtime_abi: KAGARI_RUNTIME_ABI_VERSION.into(),
            helper_abi: KAGARI_RUNTIME_HELPER_ABI_VERSION.into(),
        }
    }
    fn validate(&self) -> Result<(), MirCodecError> {
        if self.magic != MIR_MAGIC
            || self.version != MIR_FORMAT_VERSION
            || self.runtime_abi != KAGARI_RUNTIME_ABI_VERSION
            || self.helper_abi != KAGARI_RUNTIME_HELPER_ABI_VERSION
        {
            return Err(MirCodecError::Version);
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct ProgramRef<'a> {
    root: &'a ModuleIdentity,
    modules: Vec<&'a MirModule>,
}
#[derive(Deserialize)]
struct RawProgram {
    root: ModuleIdentity,
    #[serde(deserialize_with = "decode_limits::modules")]
    modules: Vec<MirModule>,
}

fn options() -> impl Options {
    DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .with_limit(MAX_MIR_BYTES as u64)
}
fn check_cancel(cancel: &CancellationToken) -> Result<(), MirCodecError> {
    cancel.check().map_err(|_| MirCodecError::Cancelled)
}
fn encoding(error: impl ToString, cancel: &CancellationToken) -> MirCodecError {
    if cancel.check().is_err() {
        MirCodecError::Cancelled
    } else {
        MirCodecError::Encoding(error.to_string())
    }
}

/// Canonical little-endian encoding contains only raw program facts and origins.
/// The bounded decoder also checks the wire shape before bytes are published, so
/// an in-memory seal with larger collections cannot produce an unreadable artifact.
pub fn encode_program(
    program: &VerifiedMirProgram,
    cancel: &CancellationToken,
) -> Result<Vec<u8>, MirCodecError> {
    check_cancel(cancel)?;
    check_counts(program.modules().iter().map(|module| &**module), cancel)?;
    let mut writer = CancellableWriter {
        bytes: Vec::new(),
        cancel,
    };
    options()
        .serialize_into(&mut writer, &Header::current())
        .map_err(|error| encoding(error, cancel))?;
    let wire_program = ProgramRef {
        root: program.root(),
        modules: program.modules().iter().map(|module| &**module).collect(),
    };
    options()
        .serialize_into(&mut writer, &wire_program)
        .map_err(|error| encoding(error, cancel))?;
    // This checks collection/type decoding bounds without retaining or encoding analyses.
    read_raw(&writer.bytes, cancel)?;
    Ok(writer.bytes)
}

/// Decode untrusted native input, then verify every module and cross-module link.
/// Liveness, roots, debug availability, safepoints and logical charges are rebuilt.
pub fn decode_program(
    bytes: &[u8],
    cancel: &CancellationToken,
) -> Result<VerifiedMirProgram, MirCodecError> {
    let raw = read_raw(bytes, cancel)?;
    verify_program(raw.root, raw.modules, cancel).map_err(|error| {
        if cancel.check().is_err() {
            MirCodecError::Cancelled
        } else {
            MirCodecError::Verification(error)
        }
    })
}

fn read_raw(bytes: &[u8], cancel: &CancellationToken) -> Result<RawProgram, MirCodecError> {
    check_cancel(cancel)?;
    if bytes.len() > MAX_MIR_BYTES {
        return Err(MirCodecError::SizeLimit);
    }
    if bytes.get(..4) != Some(MIR_MAGIC.as_slice())
        || bytes.get(4..6) != Some(MIR_FORMAT_VERSION.to_le_bytes().as_slice())
    {
        return Err(MirCodecError::Version);
    }
    let mut reader = CancellableReader { bytes, cancel };
    let header: Header = options()
        .deserialize_from(&mut reader)
        .map_err(|error| encoding(error, cancel))?;
    header.validate()?;
    let raw = options()
        .deserialize_from(&mut reader)
        .map_err(|error| encoding(error, cancel))?;
    if !reader.bytes.is_empty() {
        return Err(MirCodecError::Encoding("trailing bytes".into()));
    }
    let raw: RawProgram = raw;
    check_counts(raw.modules.iter(), cancel)?;
    Ok(raw)
}

fn check_counts<'a>(
    modules: impl Iterator<Item = &'a MirModule>,
    cancel: &CancellationToken,
) -> Result<(), MirCodecError> {
    let mut functions = 0usize;
    let mut instructions = 0usize;
    let mut blocks = 0usize;
    let mut native_targets = 0usize;
    for (index, module) in modules.enumerate() {
        check_cancel(cancel)?;
        if index >= decode_limits::MAX_MODULES {
            return Err(MirCodecError::Encoding(
                "module count limit exceeded".into(),
            ));
        }
        functions = functions.saturating_add(module.functions.len());
        native_targets = native_targets.saturating_add(module.native_targets.len());
        if native_targets > decode_limits::MAX_TABLE_RECORDS {
            return Err(MirCodecError::Encoding(
                "program native target count limit exceeded".into(),
            ));
        }
        if functions > decode_limits::MAX_FUNCTIONS {
            return Err(MirCodecError::Encoding(
                "program function count limit exceeded".into(),
            ));
        }
        for function in &module.functions {
            check_cancel(cancel)?;
            blocks = blocks.saturating_add(function.blocks.len());
            if blocks > decode_limits::MAX_TABLE_RECORDS {
                return Err(MirCodecError::Encoding(
                    "program block count limit exceeded".into(),
                ));
            }
            for block in &function.blocks {
                check_cancel(cancel)?;
                instructions = instructions
                    .saturating_add(block.instructions.len())
                    .saturating_add(1);
                if instructions > decode_limits::MAX_INSTRUCTIONS {
                    return Err(MirCodecError::Encoding(
                        "program instruction count limit exceeded".into(),
                    ));
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn small_operands<'de, D, A>(deserializer: D) -> Result<SmallVec<A>, D::Error>
where
    D: Deserializer<'de>,
    A: Array,
    A::Item: Deserialize<'de>,
{
    decode_limits::operands(deserializer).map(SmallVec::from_vec)
}

struct CancellableReader<'a> {
    bytes: &'a [u8],
    cancel: &'a CancellationToken,
}
impl Read for CancellableReader<'_> {
    fn read(&mut self, target: &mut [u8]) -> io::Result<usize> {
        self.cancel
            .check()
            .map_err(|_| IoError::other("MIR codec cancelled"))?;
        self.bytes.read(target)
    }
}
struct CancellableWriter<'a> {
    bytes: Vec<u8>,
    cancel: &'a CancellationToken,
}
impl Write for CancellableWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.cancel
            .check()
            .map_err(|_| IoError::other("MIR codec cancelled"))?;
        if self.bytes.len().saturating_add(bytes.len()) > MAX_MIR_BYTES {
            return Err(IoError::other("MIR encoded byte limit exceeded"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
