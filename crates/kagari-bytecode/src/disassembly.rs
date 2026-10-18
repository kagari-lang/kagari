//! Offline inspection of fully validated KBC artifacts.
//!
//! Preparation checks the entire envelope and bytecode graph without source files,
//! native providers or a runtime. It does not authenticate signatures, establish
//! host linkability or interpret the optional portable MIR payload.

mod presentation;
mod selection;

#[cfg(test)]
mod tests;

use crate::{
    artifact::{
        ArtifactCodecError, ArtifactCompatibility, ArtifactHeader, ArtifactValidationError,
        KbcArtifact,
    },
    disassembly::presentation::{DisplayContext, quoted},
    program::verified::VerifiedBytecodeProgram,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{map::DefinitionContext, mapping::DefinitionMappingError},
};
use std::{
    error::Error,
    fmt,
    io::{self, Write},
};

/// A verified bytecode program and the small envelope summary needed for inspection.
///
/// The program owns its definition scope. Decoded envelopes, import contexts and
/// optional MIR buffers are released before preparation returns.
#[derive(Debug)]
pub struct PreparedDisassembly {
    program: VerifiedBytecodeProgram,
    header: ArtifactHeader,
    has_portable_mir: bool,
}

impl PreparedDisassembly {
    /// Decodes, imports and verifies a complete bounded artifact offline.
    ///
    /// Uses the codec's byte/count/depth limits and current compatibility defaults.
    /// Every function is validated before any selection or output is possible.
    /// Returns decode, identity-import or envelope/bytecode validation errors;
    /// optional MIR is retained only as a presence flag, not semantically checked.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DisassemblyError> {
        let decoded = KbcArtifact::from_bytes(bytes).map_err(DisassemblyError::Decode)?;
        let context =
            DefinitionContext::new().map_err(|error| DisassemblyError::Identity(error.into()))?;
        let imported = decoded
            .import_into(&context, &CancellationToken::default())
            .map_err(DisassemblyError::Identity)?;
        drop(decoded);
        let artifact = imported.into_records();
        let header = artifact.header.clone();
        let has_portable_mir = artifact.portable_mir.is_some();
        let program = artifact
            .into_verified(&ArtifactCompatibility::default(), &context)
            .map_err(|error| DisassemblyError::Validation(Box::new(error)))?
            .into_bytecode();
        Ok(Self {
            program,
            header,
            has_portable_mir,
        })
    }

    /// Borrows immutable bytecode evidence, including its retained definition table.
    pub fn program(&self) -> &VerifiedBytecodeProgram {
        &self.program
    }

    /// Writes the envelope summary and one compact header per script function.
    ///
    /// Includes private/generated bodies in stored module/function order, but no
    /// instruction bodies, detailed slot tables or native-provider implementations.
    /// Stops at the first IO error; buffering and flushing belong to the caller.
    pub fn write_directory(&self, writer: &mut impl Write) -> io::Result<()> {
        self.write_summary(writer)?;
        let context = DisplayContext {
            definitions: self.program.definitions(),
        };
        for (m, module) in self.program.program().modules.iter().enumerate() {
            writeln!(
                writer,
                "\nmodule @{m}: {}",
                context.module(&module.identity)
            )?;
            for (f, function) in module.functions.iter().enumerate() {
                writeln!(
                    writer,
                    "{}",
                    selection::function_header(&context, module, function, m, f)
                )?;
            }
        }
        Ok(())
    }

    /// Writes deterministic encoded provenance, validation scope and root/count facts.
    ///
    /// The content hash identifies content, not authenticity or a live generation.
    /// The root module is not an assertion about public exports or an entry function.
    /// Artifact text is escaped; no source paths are opened. Stops on the first IO
    /// error without flushing the caller's writer.
    pub fn write_summary(&self, writer: &mut impl Write) -> io::Result<()> {
        let header = &self.header;
        writeln!(
            writer,
            "artifact: format={} language={} validation=envelope+bytecode",
            header.format_version,
            quoted(&header.language_version)
        )?;
        writeln!(writer, "compiler: {}", quoted(&header.compiler_fingerprint))?;
        writeln!(
            writer,
            "runtime_abi: {} helper_abi: {}",
            quoted(&header.runtime_abi_version),
            quoted(&header.runtime_helper_abi_version)
        )?;
        writeln!(writer, "content_hash: {}", header.content_hash.to_hex())?;
        let program = self.program.program();
        let context = DisplayContext {
            definitions: self.program.definitions(),
        };
        writeln!(
            writer,
            "root: @{} {}",
            program.root.index(),
            context.module(&header.module_identity)
        )?;
        writeln!(
            writer,
            "modules: {}, functions: {}",
            program.modules.len(),
            program
                .modules
                .iter()
                .map(|module| module.functions.len())
                .sum::<usize>()
        )?;
        writeln!(
            writer,
            "portable_mir: {}",
            if self.has_portable_mir {
                "present, not semantically checked"
            } else {
                "absent"
            }
        )
    }

    /// Resolves all functions, an exact underlying qualified name, or `@m:f`.
    ///
    /// Returned pairs are artifact-local module/function buffer indices, not stable
    /// definition IDs. `None` returns all slots in stored order. Names use raw text
    /// (not display escapes) and exclude instance arguments; ambiguous names fail
    /// with deterministic candidate headers including their concrete arguments.
    /// Malformed/out-of-range numeric selectors and absent names return errors.
    pub fn select(&self, selector: Option<&str>) -> Result<Vec<(usize, usize)>, DisassemblyError> {
        selection::selected_slots(&self.program, selector)
    }
}

/// An offline preparation or exact function-selection failure.
///
/// Display escapes artifact/selector text, including nested validation diagnostics.
/// IO errors are returned directly by the writing methods instead.
#[derive(Debug)]
pub enum DisassemblyError {
    Decode(ArtifactCodecError),
    Identity(DefinitionMappingError),
    Validation(Box<ArtifactValidationError>),
    InvalidSelector(String),
    FunctionNotFound(String),
    AmbiguousFunction {
        selector: String,
        /// Escaped compact headers in artifact slot order.
        candidates: Vec<String>,
    },
}

impl fmt::Display for DisassemblyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decode(error) => write!(
                f,
                "disassembly decode failed: {}",
                quoted(&error.to_string())
            ),
            Self::Identity(error) => write!(
                f,
                "disassembly identity import failed: {}",
                quoted(&error.to_string())
            ),
            Self::Validation(error) => write!(
                f,
                "disassembly validation failed: {}",
                quoted(&error.to_string())
            ),
            Self::InvalidSelector(selector) => write!(
                f,
                "invalid function slot {}; expected an in-range @<module-index>:<function-index>",
                quoted(selector)
            ),
            Self::FunctionNotFound(selector) => {
                write!(f, "function not found: {}", quoted(selector))
            }
            Self::AmbiguousFunction {
                selector,
                candidates,
            } => {
                write!(f, "ambiguous function {}", quoted(selector))?;
                for candidate in candidates {
                    write!(f, "\n  {candidate}")?;
                }
                Ok(())
            }
        }
    }
}

impl Error for DisassemblyError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Decode(error) => Some(error),
            Self::Identity(error) => Some(error),
            Self::Validation(error) => Some(error.as_ref()),
            Self::InvalidSelector(_)
            | Self::FunctionNotFound(_)
            | Self::AmbiguousFunction { .. } => None,
        }
    }
}
