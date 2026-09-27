use crate::bytecode::lower_to_bytecode;
use crate::lower_to_mir;
use crate::tests::common;
use kagari_common::Span;
use kagari_common::line_index::PositionEncoding;
use kagari_mir::debug::SourcePosition;
use kagari_mir::{MirVerificationErrorKind, verify_mir};
use std::sync::Arc;

#[test]
fn portable_origins_preserve_utf8_positions_after_source_and_codec_handoff() {
    for text in [
        "fn main() -> i32 {\n    val text = \"雪😀\"; 7\n}\n",
        "fn main() -> i32 {\r\n    val text = \"雪😀\"; 7\r\n}\r\n",
    ] {
        let checked = common::analyze_ok(text);
        let weak_source = Arc::downgrade(&checked.lowered.source);
        let verified = lower_to_mir(&checked, &Default::default()).unwrap();
        let expected = verified.functions[0]
            .blocks
            .iter()
            .flat_map(|block| {
                block
                    .instruction_spans
                    .iter()
                    .copied()
                    .chain(Some(block.terminator_span.unwrap_or_default()))
            })
            .map(|span| {
                let position = checked
                    .lowered
                    .source
                    .position(span.start, PositionEncoding::Utf8)
                    .unwrap();
                (
                    span.start,
                    Some((position.line + 1) as u32),
                    Some((position.character + 1) as u32),
                )
            })
            .collect::<Vec<_>>();
        let mut raw = verified.into_unverified();
        for function in &mut raw.functions {
            let encoded = bincode::serialize(function.debug.source.as_ref().unwrap()).unwrap();
            function.debug.source = Some(bincode::deserialize(&encoded).unwrap());
        }
        drop(checked);
        assert!(
            weak_source.upgrade().is_none(),
            "MIR must not retain source text"
        );
        let verified = verify_mir(raw, &Default::default()).unwrap();
        let bytecode = lower_to_bytecode(&verified).unwrap();
        let debug = &bytecode.functions[0].metadata.debug;
        assert_eq!(debug.source_uri.as_deref(), Some("test.kg"));
        assert_eq!(
            debug
                .line_table
                .iter()
                .map(|entry| (entry.source_offset, entry.line, entry.column))
                .collect::<Vec<_>>(),
            expected
        );
    }
}

#[test]
fn malformed_origins_and_unmapped_ranges_are_rejected_before_lowering() {
    let checked = common::analyze_ok("fn main() -> i32 { val x = 1; x + 2 }");
    let original = lower_to_mir(&checked, &Default::default())
        .unwrap()
        .into_unverified();
    for mutation in 0..9 {
        let mut raw = original.clone();
        let function = &mut raw.functions[0];
        let origin = function.debug.source.as_mut().unwrap();
        match mutation {
            0 => {
                origin.positions.remove(0);
            }
            1 => {
                origin.positions.insert(1, origin.positions[0]);
            }
            2 => {
                origin.positions.reverse();
            }
            3 => {
                origin.positions[0].line = Some(0);
            }
            4 => {
                origin.positions[0].column = Some(0);
            }
            5 => {
                function.blocks[0].instruction_spans[0] =
                    Span::new(origin.byte_len + 1, origin.byte_len + 1);
            }
            6 => {
                function.debug.source_span = Span::new(3, 2);
            }
            7 => {
                origin
                    .positions
                    .retain(|position| position.offset != function.debug.source_span.end);
            }
            8 => {
                origin.byte_len = usize::MAX;
                origin.positions.truncate(1);
                origin.positions.push(SourcePosition {
                    offset: usize::MAX,
                    line: Some(1),
                    column: Some(2),
                });
            }
            _ => unreachable!(),
        }
        assert_eq!(
            verify_mir(raw, &Default::default()).unwrap_err().kind,
            MirVerificationErrorKind::InvalidDebugMetadata,
            "mutation {mutation}"
        );
    }
}

#[test]
fn origin_free_mir_keeps_spans_but_does_not_invent_source_coordinates() {
    let checked = common::analyze_ok("fn main() -> i32 { 7 }");
    let mut raw = lower_to_mir(&checked, &Default::default())
        .unwrap()
        .into_unverified();
    for function in &mut raw.functions {
        function.debug.source = None;
    }
    let verified = verify_mir(raw, &Default::default()).unwrap();
    let bytecode = lower_to_bytecode(&verified).unwrap();
    let debug = &bytecode.functions[0].metadata.debug;
    assert!(debug.source_uri.is_none());
    assert!(
        debug
            .line_table
            .iter()
            .all(|entry| entry.line.is_none() && entry.column.is_none())
    );
    assert!(
        debug
            .source_spans
            .iter()
            .any(|entry| entry.span != Span::default())
    );
}
