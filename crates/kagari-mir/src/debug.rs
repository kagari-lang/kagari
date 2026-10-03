use crate::{function::MirFunction, ids::LocalId};
use kagari_abi::representation::ValueType;
use kagari_common::{identity::ModuleIdentity, span::Span};
use serde::{Deserialize, Serialize};
use std::iter;

/// Portable source provenance. Coordinates are captured by the frontend; no
/// source text, source-analysis handles or process-local file IDs are retained.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceOrigin {
    pub uri: String,
    pub byte_len: usize,
    /// Strictly increasing offsets. Missing coordinates preserve source offsets
    /// that have no line position (for example, the LF byte of a CRLF pair).
    #[serde(deserialize_with = "kagari_abi::decode_limits::table")]
    pub positions: Vec<SourcePosition>,
}

impl SourceOrigin {
    pub fn position(&self, offset: usize) -> Option<&SourcePosition> {
        self.positions
            .binary_search_by_key(&offset, |position| position.offset)
            .ok()
            .map(|index| &self.positions[index])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourcePosition {
    pub offset: usize,
    /// One-based line and UTF-8 byte column, matching executable debug metadata.
    pub line: Option<u32>,
    pub column: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MirFunctionDebugMetadata {
    pub source: Option<SourceOrigin>,
    pub source_module: Option<ModuleIdentity>,
    pub source_span: Span,
    #[serde(deserialize_with = "kagari_abi::decode_limits::table")]
    pub locals: MirLocalDebugBuffer,
    #[serde(deserialize_with = "kagari_abi::decode_limits::table")]
    pub captured_bindings: CapturedBindingDebugBuffer,
    #[serde(deserialize_with = "kagari_abi::decode_limits::table")]
    pub lexical_scopes: Vec<MirLexicalScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirLexicalScope {
    pub parent: Option<usize>,
    pub local: Option<LocalId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirLocalDebugInfo {
    pub local: LocalId,
    pub name: String,
    pub span: Span,
    pub ty: ValueType,
    pub is_parameter: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirCapturedBindingDebugInfo {
    pub name: String,
    pub span: Span,
    pub ty: ValueType,
}

pub type MirLocalDebugBuffer = Vec<MirLocalDebugInfo>;
pub type CapturedBindingDebugBuffer = Vec<MirCapturedBindingDebugInfo>;

impl<I> MirFunction<I> {
    /// All referenced source ranges, including the synthetic zero-width origin.
    /// Transformations retain their original ranges or provide matching positions.
    pub fn source_spans(&self) -> impl Iterator<Item = Span> + '_ {
        iter::once(Span::default())
            .chain(iter::once(self.debug.source_span))
            .chain(self.debug.locals.iter().map(|local| local.span))
            .chain(
                self.debug
                    .captured_bindings
                    .iter()
                    .map(|binding| binding.span),
            )
            .chain(self.blocks.iter().flat_map(|block| {
                block
                    .instruction_spans
                    .iter()
                    .copied()
                    .chain(block.terminator_span)
            }))
    }
}
