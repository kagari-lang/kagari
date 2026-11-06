//! A Rust-only execution-root adapter. Stable frame IDs survive bank relocation.

use kagari_gc::{
    access::Scope,
    error::GcError,
    object::{Edge, Gc},
    root_source::{RootAccess, RootSource},
    scan::{EdgeVisitor, ScanBudget, ScanStatus},
};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_SOURCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameId {
    owner: u64,
    index: usize,
    generation: u64,
}

struct Entry<T: 'static> {
    generation: u64,
    values: Option<Vec<Option<Edge<T>>>>,
}

pub struct Frames<T: 'static> {
    owner: u64,
    entries: Vec<Entry<T>>,
}

pub struct Cursor {
    end: usize,
    next: usize,
    active: Option<FrameId>,
    slot: usize,
    slots: usize,
}

/// Exclusive access to one admitted frame; no borrow of an individual slot escapes.
pub struct FrameAccess<'a, 'gc, T: 'static> {
    values: &'a mut Vec<Option<Edge<T>>>,
    cx: &'a mut Scope<'gc>,
}

impl<T: 'static> Frames<T> {
    pub fn enter(
        roots: &mut RootAccess<'_, Self>,
        cx: &Scope<'_>,
        slots: usize,
    ) -> Result<FrameId, GcError> {
        // SAFETY: same-heap admission; only empty slots are installed, never
        // extracted from another source. Existing frame coordinates stay stable.
        let source = unsafe { roots.source_mut(cx)? };
        let values = (0..slots).map(|_| None).collect();
        for (index, entry) in source.entries.iter_mut().enumerate() {
            if entry.values.is_none()
                && let Some(generation) = entry.generation.checked_add(1)
            {
                entry.generation = generation;
                entry.values = Some(values);
                return Ok(FrameId {
                    owner: source.owner,
                    index,
                    generation,
                });
            }
        }
        let id = FrameId {
            owner: source.owner,
            index: source.entries.len(),
            generation: 1,
        };
        source.entries.push(Entry {
            generation: 1,
            values: Some(values),
        });
        Ok(id)
    }

    pub fn leave(
        roots: &mut RootAccess<'_, Self>,
        cx: &Scope<'_>,
        id: FrameId,
    ) -> Result<(), GcError> {
        // SAFETY: removing roots does not dereference/free their targets. The
        // cursor validates occupancy and generation before every resumed slice.
        let source = unsafe { roots.source_mut(cx)? };
        let entry = source.entries.get_mut(id.index).ok_or(GcError::Index)?;
        if source.owner != id.owner || entry.generation != id.generation || entry.values.is_none() {
            return Err(GcError::Index);
        }
        entry.values = None;
        Ok(())
    }

    pub fn with_frame<'gc, O>(
        roots: &mut RootAccess<'_, Self>,
        cx: &mut Scope<'gc>,
        id: FrameId,
        operation: impl FnOnce(&mut FrameAccess<'_, 'gc, T>) -> Result<O, GcError>,
    ) -> Result<O, GcError> {
        // SAFETY: the source remains in its driver. Only the checked frame
        // facade escapes this method; all its reference writes publish first.
        let source = unsafe { roots.source_mut(cx)? };
        let entry = source.entries.get_mut(id.index).ok_or(GcError::Index)?;
        if source.owner != id.owner || entry.generation != id.generation {
            return Err(GcError::Index);
        }
        let values = entry.values.as_mut().ok_or(GcError::Index)?;
        operation(&mut FrameAccess { values, cx })
    }
}

impl<'gc, T: 'static> FrameAccess<'_, 'gc, T> {
    pub fn get(&mut self, index: usize) -> Result<Option<Gc<'gc, T>>, GcError> {
        let edge = self.values.get(index).ok_or(GcError::Index)?;
        // SAFETY: this slot is retained by the admitted same-driver source. Its
        // type was established on installation; no collection can intervene.
        edge.as_ref()
            .map(|edge| unsafe { self.cx.source_get(edge) })
            .transpose()
    }

    pub fn set(&mut self, index: usize, value: Option<Gc<'gc, T>>) -> Result<(), GcError> {
        let slot = self.values.get_mut(index).ok_or(GcError::Index)?;
        // SAFETY: publish before the no-fail store into the paired source. No
        // carrier escapes, including on error/unwind. Clearing needs no barrier.
        let edge = value
            .map(|value| unsafe { self.cx.source_edge(value) })
            .transpose()?;
        *slot = edge;
        Ok(())
    }

    pub fn copy(&mut self, destination: usize, source: usize) -> Result<(), GcError> {
        let value = self.get(source)?;
        self.set(destination, value)
    }

    /// Grow without changing existing logical coordinates. New slots start empty.
    pub fn grow(&mut self, slots: usize) {
        if slots > self.values.len() {
            self.values.resize_with(slots, || None);
        }
    }
}

// SAFETY: private slots are created empty and only edited through FrameAccess;
// writes publish and no operation exposes the source. Cursors hold coordinates,
// not buffer addresses. Dead/reused frames are skipped and new roots publish.
unsafe impl<T: 'static> RootSource for Frames<T> {
    type Cursor = Cursor;

    fn empty() -> Self {
        Self {
            owner: NEXT_SOURCE
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .expect("example source identity space"),
            entries: Vec::new(),
        }
    }

    fn begin_scan(&self) -> Result<Cursor, GcError> {
        Ok(Cursor {
            end: self.entries.len(),
            next: 0,
            active: None,
            slot: 0,
            slots: 0,
        })
    }

    fn scan(
        &self,
        cursor: &mut Cursor,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        while cursor.next < cursor.end {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            let entry = &self.entries[cursor.next];
            let id = FrameId {
                owner: self.owner,
                index: cursor.next,
                generation: entry.generation,
            };
            if let Some(values) = &entry.values
                && cursor.active.is_none_or(|active| active == id)
            {
                if cursor.active.is_none() {
                    cursor.active = Some(id);
                    cursor.slots = values.len();
                    cursor.slot = 0;
                    continue;
                }
                if cursor.slot < cursor.slots {
                    if let Some(edge) = &values[cursor.slot]
                        // SAFETY: retained same-heap source slot; retry Pending
                        // without advancing so the bitmap reset is not skipped.
                        && unsafe { visitor.try_visit_stored(edge, budget)? } == ScanStatus::Pending
                    {
                        return Ok(ScanStatus::Pending);
                    }
                    cursor.slot += 1;
                    continue;
                }
            }
            cursor.next += 1;
            cursor.active = None;
        }
        Ok(ScanStatus::Complete)
    }
}
