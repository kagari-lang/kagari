use crate::bytecode::lower_local;
use kagari_abi::ids::DebugPointId;
use kagari_bytecode::BytecodeDebugMetadata;
use kagari_bytecode::BytecodeInstruction;
use kagari_bytecode::CapturedBindingDebugInfo;
use kagari_bytecode::FrameLayout;
use kagari_bytecode::InstructionSourceSpan;
use kagari_bytecode::LineTableEntry;
use kagari_bytecode::LocalLiveRange;
use kagari_bytecode::ModuleRef;
use kagari_bytecode::SafeDebugPoint;
use kagari_bytecode::SafeDebugPointKind;
use kagari_common::Span;
use kagari_common::line_index::PositionEncoding;
use kagari_mir::function::MirFunction;
use kagari_mir::ids::LocalId;
use std::collections::HashMap;
pub(super) fn collect_debug_metadata(
    function: &MirFunction,
    instructions: &[BytecodeInstruction],
    instruction_spans: &[Span],
    instruction_scopes: &[usize],
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
                .and_then(|source| source.position(span.start, PositionEncoding::Utf8));
            LineTableEntry {
                instruction_offset,
                source_offset: span.start,
                line: position.and_then(|p| u32::try_from(p.line + 1).ok()),
                column: position.and_then(|p| u32::try_from(p.character + 1).ok()),
            }
        })
        .collect::<Vec<_>>();

    let mut safe_debug_points = Vec::new();
    if !instructions.is_empty() {
        push_debug_point(
            &mut safe_debug_points,
            0,
            instruction_spans.first().copied().unwrap_or_default(),
            SafeDebugPointKind::FunctionEntry,
        );
    }
    for (instruction_offset, instruction) in instructions.iter().enumerate() {
        let span = instruction_spans
            .get(instruction_offset)
            .copied()
            .unwrap_or_default();
        match instruction {
            BytecodeInstruction::Call { .. } => push_debug_point(
                &mut safe_debug_points,
                instruction_offset,
                span,
                SafeDebugPointKind::CallBoundary,
            ),
            BytecodeInstruction::Jump { .. } | BytecodeInstruction::Branch { .. } => {
                push_debug_point(
                    &mut safe_debug_points,
                    instruction_offset,
                    span,
                    SafeDebugPointKind::BranchTarget,
                );
            }
            BytecodeInstruction::Return(_) => push_debug_point(
                &mut safe_debug_points,
                instruction_offset,
                span,
                SafeDebugPointKind::FunctionReturn,
            ),
            BytecodeInstruction::Unreachable => push_debug_point(
                &mut safe_debug_points,
                instruction_offset,
                span,
                SafeDebugPointKind::Trap,
            ),
            _ if span != Span::default() => push_debug_point(
                &mut safe_debug_points,
                instruction_offset,
                span,
                SafeDebugPointKind::Statement,
            ),
            _ => {}
        }
    }

    let local_live_ranges = collect_local_live_ranges(function, instructions, instruction_scopes);
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
            .map(|source| source.name().to_owned()),
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

pub(super) fn collect_local_live_ranges(
    function: &MirFunction,
    instructions: &[BytecodeInstruction],
    instruction_scopes: &[usize],
) -> Vec<LocalLiveRange> {
    let end = instructions.len();
    let mut ranges = Vec::new();
    let locals = function
        .debug
        .locals
        .iter()
        .map(|local| (local.local, local))
        .collect::<HashMap<_, _>>();
    for local in function
        .debug
        .locals
        .iter()
        .filter(|local| local.is_parameter)
    {
        ranges.push(LocalLiveRange {
            local: lower_local(local.local),
            name: local.name.clone(),
            span: local.span,
            start: 0,
            end,
            ty: local.ty,
            is_parameter: true,
        });
    }

    let scopes = &function.debug.lexical_scopes;
    let mut previous_path = Vec::<usize>::new();
    let mut open = HashMap::<LocalId, usize>::new();
    for offset in 0..=end {
        let mut next_path = Vec::<usize>::new();
        if offset < end && !scopes.is_empty() {
            let mut scope = instruction_scopes.get(offset).copied().unwrap_or(0);
            while let Some(entry) = scopes.get(scope) {
                next_path.push(scope);
                if next_path.len() >= scopes.len() {
                    break;
                }
                let Some(parent) = entry.parent else { break };
                scope = parent;
            }
            next_path.reverse();
        }
        let common = previous_path
            .iter()
            .zip(&next_path)
            .take_while(|(left, right)| left == right)
            .count();
        for scope in previous_path[common..].iter().rev() {
            if let Some(local) = scopes[*scope].local
                && let Some(start) = open.remove(&local)
                && start < offset
                && let Some(info) = locals.get(&local)
            {
                ranges.push(LocalLiveRange {
                    local: lower_local(local),
                    name: info.name.clone(),
                    span: info.span,
                    start,
                    end: offset,
                    ty: info.ty,
                    is_parameter: false,
                });
            }
        }
        for scope in &next_path[common..] {
            if let Some(local) = scopes[*scope].local {
                open.insert(local, offset);
            }
        }
        previous_path = next_path;
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
