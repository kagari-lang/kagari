//! Half-open local-name ranges indexed once, without per-instruction table scans.

use crate::{
    disassembly::{
        body::debug::span,
        presentation::{physical, quoted},
    },
    module::LocalLiveRange,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Write},
};

#[derive(Default)]
struct Events {
    starts: Vec<usize>,
    ends: Vec<usize>,
}

type Active = BTreeMap<usize, BTreeSet<usize>>;

pub(super) struct RangeIndex<'a> {
    ranges: &'a [LocalLiveRange],
    events: BTreeMap<usize, Events>,
    overlapping: BTreeSet<usize>,
    active: Active,
}

impl<'a> RangeIndex<'a> {
    pub(super) fn new(ranges: &'a [LocalLiveRange]) -> Self {
        let mut events = BTreeMap::<usize, Events>::new();
        for (index, range) in ranges.iter().enumerate() {
            // Empty ranges remain visible in the table, but are never active.
            if range.start < range.end {
                events.entry(range.start).or_default().starts.push(index);
                events.entry(range.end).or_default().ends.push(index);
            }
        }
        let mut active = Active::new();
        let mut overlapping = BTreeSet::new();
        for event in events.values() {
            remove_ended(&mut active, ranges, &event.ends);
            for &index in &event.starts {
                let names = active.entry(ranges[index].local.index()).or_default();
                if let Some(&other) = names.first() {
                    // If several are already active they have already been marked.
                    // One representative is enough; avoid enumerating overlap pairs.
                    overlapping.insert(other);
                    overlapping.insert(index);
                }
                names.insert(index);
            }
        }
        Self {
            ranges,
            events,
            overlapping,
            active: Active::new(),
        }
    }

    pub(super) fn write_table(&self, writer: &mut impl Write) -> io::Result<()> {
        if !self.ranges.is_empty() {
            writeln!(
                writer,
                "    local ranges (nonauthoritative debug_ty/is_parameter; half-open):"
            )?;
        }
        for (index, range) in self.ranges.iter().enumerate() {
            writeln!(
                writer,
                "      range#{index}: l{} name={} instructions=[{}, {}) debug_ty={} is_parameter={} span={}{}",
                range.local.index(),
                quoted(&range.name),
                range.start,
                range.end,
                physical(range.ty),
                range.is_parameter,
                span(range.span),
                if self.overlapping.contains(&index) {
                    " (overlap; ambiguous)"
                } else {
                    ""
                }
            )?;
        }
        Ok(())
    }

    pub(super) fn write_at(&mut self, writer: &mut impl Write, offset: usize) -> io::Result<()> {
        if let Some(event) = self.events.get(&offset) {
            remove_ended(&mut self.active, self.ranges, &event.ends);
            for &index in &event.starts {
                self.active
                    .entry(self.ranges[index].local.index())
                    .or_default()
                    .insert(index);
            }
        }
        for (local, names) in &self.active {
            write!(writer, "    ; debug local l{local} at {offset}: names=[")?;
            for (position, &index) in names.iter().enumerate() {
                if position != 0 {
                    write!(writer, ", ")?;
                }
                write!(writer, "range#{index} {}", quoted(&self.ranges[index].name))?;
            }
            writeln!(
                writer,
                "]{}",
                if names.len() > 1 { " (ambiguous)" } else { "" }
            )?;
        }
        Ok(())
    }
}

fn remove_ended(active: &mut Active, ranges: &[LocalLiveRange], ends: &[usize]) {
    for &index in ends {
        let local = ranges[index].local.index();
        if let Some(names) = active.get_mut(&local) {
            names.remove(&index);
            if names.is_empty() {
                active.remove(&local);
            }
        }
    }
}
