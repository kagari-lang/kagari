//! Explicit-offset debug lookup. Metadata is presentation-only, never a type oracle.

use crate::{
    disassembly::{
        body::ranges::RangeIndex,
        presentation::{physical, quoted},
    },
    module::{BytecodeDebugMetadata, SafeDebugPointKind},
};
use kagari_common::span::Span;
use std::{
    collections::BTreeMap,
    io::{self, Write},
};

#[derive(Default)]
struct Locations {
    lines: Vec<usize>,
    spans: Vec<usize>,
    points: Vec<usize>,
}

pub(super) struct DebugIndex<'a> {
    debug: &'a BytecodeDebugMetadata,
    locations: BTreeMap<usize, Locations>,
    ranges: RangeIndex<'a>,
}

impl<'a> DebugIndex<'a> {
    pub(super) fn new(debug: &'a BytecodeDebugMetadata) -> Self {
        let mut locations = BTreeMap::<usize, Locations>::new();
        for (index, line) in debug.line_table.iter().enumerate() {
            locations
                .entry(line.instruction_offset)
                .or_default()
                .lines
                .push(index);
        }
        for (index, span) in debug.source_spans.iter().enumerate() {
            locations
                .entry(span.instruction_offset)
                .or_default()
                .spans
                .push(index);
        }
        for (index, point) in debug.safe_debug_points.iter().enumerate() {
            locations
                .entry(point.instruction_offset)
                .or_default()
                .points
                .push(index);
        }
        Self {
            debug,
            locations,
            ranges: RangeIndex::new(&debug.local_live_ranges),
        }
    }

    pub(super) fn write_tables(&self, writer: &mut impl Write) -> io::Result<()> {
        let debug = self.debug;
        let frame = &debug.frame_layout;
        if debug.source_uri.is_none()
            && debug.source_module.is_none()
            && debug.function_span == Span::default()
            && self.locations.is_empty()
            && debug.local_live_ranges.is_empty()
            && debug.captured_bindings.is_empty()
            && frame.params.is_empty()
            && frame.locals.is_empty()
            && frame.registers.is_empty()
        {
            return writeln!(writer, "  debug: absent");
        }
        write!(writer, "  debug (nonauthoritative): uri=")?;
        match &debug.source_uri {
            Some(uri) => write!(writer, "{}", quoted(uri))?,
            None => write!(writer, "unknown")?,
        }
        if let Some(module) = debug.source_module {
            write!(writer, " source_module=@{}", module.index())?;
        }
        writeln!(writer, " function_span={}", span(debug.function_span))?;
        self.ranges.write_table(writer)?;
        for (index, capture) in debug.captured_bindings.iter().enumerate() {
            writeln!(
                writer,
                "    capture#{index}: name={} debug_ty={} span={} (nonauthoritative; no slot mapping)",
                quoted(&capture.name),
                physical(capture.ty),
                span(capture.span)
            )?;
        }
        // These copies can disagree with execution metadata, even after verification.
        for (kind, types) in [
            ("params", &frame.params),
            ("locals", &frame.locals),
            ("registers", &frame.registers),
        ] {
            if !types.is_empty() {
                write!(
                    writer,
                    "    debug frame_layout {kind} (nonauthoritative): ["
                )?;
                for (index, ty) in types.iter().enumerate() {
                    if index != 0 {
                        write!(writer, ", ")?;
                    }
                    write!(writer, "{}", physical(*ty))?;
                }
                writeln!(writer, "]")?;
            }
        }
        Ok(())
    }

    /// Called once for each instruction, in increasing stored order.
    pub(super) fn write_at(&mut self, writer: &mut impl Write, offset: usize) -> io::Result<()> {
        if let Some(locations) = self.locations.get(&offset) {
            if locations.lines.len() > 1 || locations.spans.len() > 1 {
                writeln!(
                    writer,
                    "    ; debug location ambiguous at {offset}: all matching records follow"
                )?;
            }
            for &index in &locations.lines {
                let line = &self.debug.line_table[index];
                write!(writer, "    ; debug line#{index}: instruction={offset}")?;
                if let Some(number) = line.line {
                    write!(writer, " line={number}")?;
                }
                if let Some(column) = line.column {
                    write!(writer, " column={column}")?;
                }
                writeln!(writer, " source_offset={}", line.source_offset)?;
            }
            for &index in &locations.spans {
                writeln!(
                    writer,
                    "    ; debug span#{index}: instruction={offset} source_span={}",
                    span(self.debug.source_spans[index].span)
                )?;
            }
            for &index in &locations.points {
                let point = &self.debug.safe_debug_points[index];
                writeln!(
                    writer,
                    "    ; debug point#{index}: id={} instruction={offset} kind={} span={}{}",
                    point.id.index(),
                    point_kind(point.kind),
                    span(point.span),
                    if locations.points.len() > 1 {
                        " (multiple matches; ambiguous)"
                    } else {
                        ""
                    }
                )?;
            }
        }
        self.ranges.write_at(writer, offset)
    }
}

pub(super) fn span(span: Span) -> String {
    if span == Span::default() {
        "unknown".into()
    } else {
        format!("[{}, {})", span.start, span.end)
    }
}

fn point_kind(kind: SafeDebugPointKind) -> &'static str {
    match kind {
        SafeDebugPointKind::FunctionEntry => "function_entry",
        SafeDebugPointKind::Statement => "statement",
        SafeDebugPointKind::BranchTarget => "branch_target",
        SafeDebugPointKind::CallBoundary => "call_boundary",
        SafeDebugPointKind::FunctionReturn => "function_return",
        SafeDebugPointKind::Trap => "trap",
    }
}
