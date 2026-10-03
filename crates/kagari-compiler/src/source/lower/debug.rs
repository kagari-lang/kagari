use crate::source::lower::MirLoweringError;
use kagari_mir::{
    debug::{SourceOrigin, SourcePosition},
    function::MirFunction,
};
use std::collections::BTreeSet;
use {
    kagari_common::cancellation::CancellationToken,
    kagari_source::{line_index::PositionEncoding, source::SourceFile},
};

pub(super) fn capture_origin(
    function: &MirFunction,
    source: &SourceFile,
    cancel: &CancellationToken,
) -> Result<SourceOrigin, MirLoweringError> {
    let mut offsets = BTreeSet::new();
    for span in function.source_spans() {
        cancel.check().map_err(|_| MirLoweringError::Cancelled)?;
        offsets.insert(span.start);
        offsets.insert(span.end);
    }
    let mut positions = Vec::with_capacity(offsets.len());
    for offset in offsets {
        cancel.check().map_err(|_| MirLoweringError::Cancelled)?;
        let position = source.position(offset, PositionEncoding::Utf8);
        positions.push(SourcePosition {
            offset,
            line: position.and_then(|position| u32::try_from(position.line + 1).ok()),
            column: position.and_then(|position| u32::try_from(position.character + 1).ok()),
        });
    }
    Ok(SourceOrigin {
        uri: source.name().to_owned(),
        byte_len: source.text().len(),
        positions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{source::lower::lower_to_mir, tests::common};
    use kagari_common::span::Span;
    use kagari_mir::verify::verify_mir;

    #[test]
    fn origin_capture_preserves_unlocatable_crlf_offsets_and_cancellation() {
        let checked = common::analyze_ok("fn main() {}\r\n");
        let mut raw = lower_to_mir(&checked, &Default::default())
            .unwrap()
            .into_unverified();
        let function = &mut raw.functions[0];
        let offset = checked.lowered.source.text().find('\n').unwrap();
        function.blocks[0].terminator_span = Some(Span::new(offset, offset));
        let origin =
            capture_origin(function, &checked.lowered.source, &Default::default()).unwrap();
        assert_eq!(
            origin.position(offset),
            Some(&SourcePosition {
                offset,
                line: None,
                column: None
            })
        );
        let cancel = CancellationToken::default();
        cancel.cancel();
        assert!(matches!(
            capture_origin(function, &checked.lowered.source, &cancel),
            Err(MirLoweringError::Cancelled)
        ));
        function.debug.source = Some(origin);
        assert!(verify_mir(raw, &Default::default()).is_ok());
    }
}
