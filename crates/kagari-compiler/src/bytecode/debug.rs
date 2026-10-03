use crate::bytecode::lower_local;
use kagari_bytecode::{
    module::{
        BytecodeDebugMetadata, CapturedBindingDebugInfo, FrameLayout, InstructionSourceSpan,
        LineTableEntry, LocalLiveRange, SafeDebugPoint, SafeDebugPointKind,
    },
    program::ModuleRef,
};
use kagari_common::identity::table::DefinitionId;
use kagari_common::span::Span;
use kagari_contract::ids::DebugPointId;
use kagari_mir::{
    analysis::{FunctionAnalysis, PointAnalysis},
    function::MirFunction,
    ids::BlockId,
    instruction::{Instruction, Terminator},
};
use std::collections::HashMap;

pub(super) fn collect_debug_metadata(
    function: &MirFunction<DefinitionId>,
    analysis: &FunctionAnalysis,
    instruction_spans: &[Span],
    source_module: Option<ModuleRef>,
) -> BytecodeDebugMetadata {
    let source_spans = instruction_spans
        .iter()
        .enumerate()
        .map(|(instruction_offset, span)| InstructionSourceSpan {
            instruction_offset,
            span: *span,
        })
        .collect::<Vec<_>>();

    let line_table = instruction_spans
        .iter()
        .enumerate()
        .map(|(instruction_offset, span)| {
            let position = function
                .debug
                .source
                .as_ref()
                .and_then(|source| source.position(span.start));
            LineTableEntry {
                instruction_offset,
                source_offset: span.start,
                line: position.and_then(|p| p.line),
                column: position.and_then(|p| p.column),
            }
        })
        .collect::<Vec<_>>();

    let mut safe_debug_points = Vec::new();
    if !instruction_spans.is_empty() {
        push_debug_point(
            &mut safe_debug_points,
            0,
            instruction_spans.first().copied().unwrap_or_default(),
            SafeDebugPointKind::FunctionEntry,
        );
    }
    let mut points = Vec::with_capacity(instruction_spans.len());
    for (id, block) in function.emission_order() {
        let facts = analysis
            .block(BlockId::new(id))
            .expect("sealed block facts");
        for (index, instruction) in block.instructions.iter().enumerate() {
            let offset = points.len();
            let span = block.instruction_spans[index];
            let kind = match instruction {
                Instruction::Call { .. } => Some(SafeDebugPointKind::CallBoundary),
                _ if span != Span::default() => Some(SafeDebugPointKind::Statement),
                _ => None,
            };
            if let Some(kind) = kind {
                push_debug_point(&mut safe_debug_points, offset, span, kind);
            }
            points.push(facts.instruction(index).expect("sealed instruction facts"));
        }
        let kind = match block.terminator.as_ref().expect("verified terminator") {
            Terminator::Jump(_) | Terminator::Branch { .. } => SafeDebugPointKind::BranchTarget,
            Terminator::Return(_) => SafeDebugPointKind::FunctionReturn,
            Terminator::Unreachable => SafeDebugPointKind::Trap,
        };
        push_debug_point(
            &mut safe_debug_points,
            points.len(),
            block.terminator_span.unwrap_or_default(),
            kind,
        );
        points.push(facts.terminator());
    }
    let local_live_ranges = collect_local_live_ranges(function, &points);
    let captured_bindings = function
        .debug
        .captured_bindings
        .iter()
        .map(|captured| CapturedBindingDebugInfo {
            name: captured.name.clone(),
            span: captured.span,
            ty: captured.ty,
        })
        .collect();

    BytecodeDebugMetadata {
        source_uri: function
            .debug
            .source
            .as_ref()
            .map(|source| source.uri.clone()),
        source_module,
        function_span: function.debug.source_span,
        source_spans,
        line_table,
        safe_debug_points,
        local_live_ranges,
        captured_bindings,
        frame_layout: FrameLayout {
            params: function.params.iter().map(|param| param.ty).collect(),
            locals: function.locals.iter().map(|local| local.ty).collect(),
            registers: function.temps.iter().map(|temp| temp.ty).collect(),
        },
    }
}

fn collect_local_live_ranges(
    function: &MirFunction<DefinitionId>,
    points: &[&PointAnalysis],
) -> Vec<LocalLiveRange> {
    let mut ranges = Vec::new();
    let locals = function
        .debug
        .locals
        .iter()
        .map(|local| (local.local, local))
        .collect::<HashMap<_, _>>();
    let mut open = HashMap::new();
    for offset in 0..=points.len() {
        let available = points.get(offset).map(|point| point.debug_available());
        open.retain(|local, start| {
            if available.is_some_and(|set| set.contains_local(*local)) {
                return true;
            }
            let info = locals[local];
            ranges.push(LocalLiveRange {
                local: lower_local(*local),
                name: info.name.clone(),
                span: info.span,
                start: *start,
                end: offset,
                ty: info.ty,
                is_parameter: info.is_parameter,
            });
            false
        });
        if let Some(available) = available {
            for local in available.locals() {
                open.entry(local).or_insert(offset);
            }
        }
    }
    ranges.sort_by_key(|range| (range.local.index(), range.start));
    ranges
}

pub(super) fn push_debug_point(
    points: &mut Vec<SafeDebugPoint>,
    instruction_offset: usize,
    span: Span,
    kind: SafeDebugPointKind,
) {
    if points
        .iter()
        .any(|point| point.instruction_offset == instruction_offset && point.kind == kind)
    {
        return;
    }
    let id = DebugPointId::new(points.len());
    points.push(SafeDebugPoint {
        id,
        instruction_offset,
        span,
        kind,
    });
}
