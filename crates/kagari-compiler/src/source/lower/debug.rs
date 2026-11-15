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
    source: Option<&SourceFile>,
    cancel: &CancellationToken,
) -> Result<Option<SourceOrigin>, MirLoweringError> {
    cancel.check().map_err(|_| MirLoweringError::Cancelled)?;
    let Some(source) = source else {
        return Ok(None);
    };
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
    Ok(Some(SourceOrigin {
        uri: source.name().to_owned(),
        byte_len: source.text().len(),
        positions,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{tests::common, tests::common::lower_root};
    use kagari_common::identity::map::DefinitionContext;
    use kagari_common::span::Span;
    use kagari_hir::analysis::AnalysisDatabase;
    use kagari_source::source_database::{SourceDatabase, SourceLayer};

    #[test]
    fn origin_capture_preserves_unlocatable_crlf_offsets_and_cancellation() {
        let context = DefinitionContext::new().unwrap();
        let mut sources = SourceDatabase::default();
        let root = sources
            .set("debug.kgr", "fn main() {}\r\n".into(), SourceLayer::Base)
            .unwrap();
        let checked = AnalysisDatabase::default()
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap()
            .check_program(root, &Default::default())
            .unwrap();
        let mut raw = common::adopt(
            &context,
            lower_root(&checked, &Default::default())
                .unwrap()
                .into_unverified(),
        );
        let function = &mut raw.functions[0];
        let offset = checked.root().lowered.source.text().find('\n').unwrap();
        function.blocks[0].terminator_span = Some(Span::new(offset, offset));
        let origin = capture_origin(
            function,
            Some(&checked.root().lowered.source),
            &Default::default(),
        )
        .unwrap()
        .unwrap();
        assert!(
            capture_origin(function, None, &Default::default())
                .unwrap()
                .is_none()
        );
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
            capture_origin(function, Some(&checked.root().lowered.source), &cancel),
            Err(MirLoweringError::Cancelled)
        ));
        assert!(matches!(
            capture_origin(function, None, &cancel),
            Err(MirLoweringError::Cancelled)
        ));
        function.debug.source = Some(origin);
        assert!(common::verify_edited(&context, raw).is_ok());
    }
}
